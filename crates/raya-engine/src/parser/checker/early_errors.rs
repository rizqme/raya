//! Early-error legality pass
//!
//! This pass runs after parsing but before binding/type checking. It enforces
//! syntax-context rules that should be classified as parse/early errors instead
//! of checker errors.

use super::TypeSystemMode;
use crate::parser::ast::*;
use crate::parser::{Interner, ParseError, Symbol};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EarlyErrorOptions {
    pub mode: TypeSystemMode,
    pub allow_top_level_return: bool,
    pub allow_await_outside_async: bool,
    pub allow_new_target: bool,
    pub allow_super_property: bool,
}

impl EarlyErrorOptions {
    pub fn for_mode(mode: TypeSystemMode) -> Self {
        Self {
            mode,
            allow_top_level_return: !matches!(mode, TypeSystemMode::Js),
            allow_await_outside_async: false,
            allow_new_target: false,
            allow_super_property: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FunctionContext {
    is_async: bool,
    is_generator: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct LexicalContext {
    super_property_allowed: bool,
    super_call_allowed: bool,
    strict: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LabelContext {
    name: Symbol,
    is_iteration_target: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScopeKind {
    Module,
    Function,
    Parameter,
    FunctionBody,
    Block,
    Catch,
    Class,
    StaticBlock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParameterListKind {
    OrdinaryFunction,
    Arrow,
}

#[derive(Debug, Clone)]
struct ScopeFrame {
    kind: ScopeKind,
    lexical: Vec<(Symbol, crate::parser::Span)>,
    vars: Vec<(Symbol, crate::parser::Span)>,
    params: Vec<(Symbol, crate::parser::Span)>,
    catch_params: Vec<(Symbol, crate::parser::Span)>,
}

impl Default for ScopeFrame {
    fn default() -> Self {
        Self {
            kind: ScopeKind::Block,
            lexical: Vec::new(),
            vars: Vec::new(),
            params: Vec::new(),
            catch_params: Vec::new(),
        }
    }
}

pub fn check_early_errors(
    module: &Module,
    interner: &Interner,
    mode: TypeSystemMode,
) -> Result<(), Vec<ParseError>> {
    check_early_errors_with_options(module, interner, EarlyErrorOptions::for_mode(mode))
}

pub fn check_early_errors_with_options(
    module: &Module,
    interner: &Interner,
    options: EarlyErrorOptions,
) -> Result<(), Vec<ParseError>> {
    let mut pass = EarlyErrorPass::new(interner, options);
    pass.check_module(module);
    if pass.errors.is_empty() {
        Ok(())
    } else {
        Err(pass.errors)
    }
}

struct EarlyErrorPass<'a> {
    interner: &'a Interner,
    #[allow(dead_code)]
    mode: TypeSystemMode,
    allow_top_level_return: bool,
    allow_await_outside_async: bool,
    allow_new_target: bool,
    allow_super_property: bool,
    errors: Vec<ParseError>,
    function_stack: Vec<FunctionContext>,
    lexical_stack: Vec<LexicalContext>,
    label_stack: Vec<LabelContext>,
    scope_stack: Vec<ScopeFrame>,
    loop_depth: usize,
    breakable_depth: usize,
}

impl<'a> EarlyErrorPass<'a> {
    fn new(interner: &'a Interner, options: EarlyErrorOptions) -> Self {
        Self {
            interner,
            mode: options.mode,
            allow_top_level_return: options.allow_top_level_return,
            allow_await_outside_async: options.allow_await_outside_async,
            allow_new_target: options.allow_new_target,
            allow_super_property: options.allow_super_property,
            errors: Vec::new(),
            function_stack: Vec::new(),
            lexical_stack: vec![LexicalContext::default()],
            label_stack: Vec::new(),
            scope_stack: vec![ScopeFrame {
                kind: ScopeKind::Module,
                ..ScopeFrame::default()
            }],
            loop_depth: 0,
            breakable_depth: 0,
        }
    }

    fn check_module(&mut self, module: &Module) {
        if let Some(root) = self.lexical_stack.first_mut() {
            root.strict = Self::directive_prologue_is_strict(&module.statements, self.interner);
        }
        for stmt in &module.statements {
            self.check_stmt(stmt);
        }
    }

    fn current_function(&self) -> Option<FunctionContext> {
        self.function_stack.last().copied()
    }

    fn current_lexical(&self) -> LexicalContext {
        self.lexical_stack.last().copied().unwrap_or_default()
    }

    fn current_strict(&self) -> bool {
        self.current_lexical().strict
    }

    fn push_function<T>(
        &mut self,
        is_async: bool,
        is_generator: bool,
        lexical: LexicalContext,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let saved_loop_depth = self.loop_depth;
        let saved_breakable_depth = self.breakable_depth;
        let saved_label_len = self.label_stack.len();
        self.function_stack.push(FunctionContext {
            is_async,
            is_generator,
        });
        self.lexical_stack.push(lexical);
        self.scope_stack.push(ScopeFrame {
            kind: ScopeKind::Function,
            ..ScopeFrame::default()
        });
        self.loop_depth = 0;
        self.breakable_depth = 0;
        let result = f(self);
        self.function_stack.pop();
        self.lexical_stack.pop();
        self.scope_stack.pop();
        self.loop_depth = saved_loop_depth;
        self.breakable_depth = saved_breakable_depth;
        self.label_stack.truncate(saved_label_len);
        result
    }

    fn push_lexical<T>(&mut self, lexical: LexicalContext, f: impl FnOnce(&mut Self) -> T) -> T {
        self.lexical_stack.push(lexical);
        let result = f(self);
        self.lexical_stack.pop();
        result
    }

    fn push_label<T>(
        &mut self,
        name: Symbol,
        is_iteration_target: bool,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        self.label_stack.push(LabelContext {
            name,
            is_iteration_target,
        });
        let result = f(self);
        self.label_stack.pop();
        result
    }

    fn push_loop<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.loop_depth += 1;
        self.breakable_depth += 1;
        let result = f(self);
        self.loop_depth -= 1;
        self.breakable_depth -= 1;
        result
    }

    fn push_breakable<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.breakable_depth += 1;
        let result = f(self);
        self.breakable_depth -= 1;
        result
    }

    fn error(&mut self, message: impl Into<String>, span: crate::parser::Span) {
        let message = message.into();
        self.errors.push(ParseError::invalid_syntax(message, span));
    }

    fn is_use_strict_directive(stmt: &Statement, interner: &Interner) -> bool {
        matches!(
            stmt,
            Statement::Expression(ExpressionStatement {
                expression: Expression::StringLiteral(StringLiteral {
                    value,
                    raw_literal: true,
                    ..
                }),
                ..
            }) if interner.resolve(*value) == "use strict"
        )
    }

    fn directive_prologue_is_strict(statements: &[Statement], interner: &Interner) -> bool {
        for stmt in statements {
            match stmt {
                Statement::Expression(ExpressionStatement {
                    expression: Expression::StringLiteral(_),
                    ..
                }) => {
                    if Self::is_use_strict_directive(stmt, interner) {
                        return true;
                    }
                }
                _ => break,
            }
        }
        false
    }

    fn is_restricted_strict_binding_name(&self, ident: &Identifier) -> bool {
        matches!(
            self.interner.resolve(ident.name),
            "eval"
                | "arguments"
                | "implements"
                | "interface"
                | "let"
                | "package"
                | "private"
                | "protected"
                | "public"
                | "static"
                | "yield"
        )
    }

    fn check_strict_binding_name(&mut self, ident: &Identifier) {
        if self.is_restricted_strict_binding_name(ident) {
            self.error(
                format!(
                    "Binding name '{}' is not allowed in strict mode",
                    self.interner.resolve(ident.name)
                ),
                ident.span,
            );
        }
    }

    fn collect_bound_identifiers<'b>(pattern: &'b Pattern, out: &mut Vec<&'b Identifier>) {
        match pattern {
            Pattern::Identifier(id) => out.push(id),
            Pattern::Array(array) => {
                for elem in array.elements.iter().flatten() {
                    Self::collect_bound_identifiers(&elem.pattern, out);
                }
                if let Some(rest) = &array.rest {
                    Self::collect_bound_identifiers(rest, out);
                }
            }
            Pattern::Object(obj) => {
                for prop in &obj.properties {
                    Self::collect_bound_identifiers(&prop.value, out);
                }
                if let Some(rest) = &obj.rest {
                    out.push(rest);
                }
            }
            Pattern::Rest(rest) => Self::collect_bound_identifiers(&rest.argument, out),
        }
    }

    fn check_pattern_bindings(&mut self, pattern: &Pattern, strict: bool, check_duplicates: bool) {
        let mut bound = Vec::new();
        Self::collect_bound_identifiers(pattern, &mut bound);
        let mut seen = Vec::new();
        for ident in bound {
            if strict {
                self.check_strict_binding_name(ident);
            }
            if check_duplicates && seen.contains(&ident.name) {
                self.error(
                    format!(
                        "Duplicate binding '{}' in pattern",
                        self.interner.resolve(ident.name)
                    ),
                    ident.span,
                );
            } else {
                seen.push(ident.name);
            }
        }
    }

    fn is_simple_parameter(param: &Parameter) -> bool {
        matches!(param.pattern, Pattern::Identifier(_))
            && !param.is_rest
            && param.default_value.is_none()
    }

    fn is_simple_parameter_list(params: &[Parameter]) -> bool {
        params.iter().all(Self::is_simple_parameter)
    }

    fn check_parameter_list(
        &mut self,
        params: &[Parameter],
        binding_strict: bool,
        body_has_use_strict: bool,
        span: crate::parser::Span,
        kind: ParameterListKind,
    ) {
        let simple = Self::is_simple_parameter_list(params);
        if body_has_use_strict && !simple {
            self.error(
                "Illegal 'use strict' directive in function with non-simple parameter list",
                span,
            );
        }

        let check_duplicates = match kind {
            ParameterListKind::OrdinaryFunction => binding_strict || !simple,
            ParameterListKind::Arrow => true,
        };
        let mut seen = Vec::new();
        for param in params {
            self.check_pattern_bindings(&param.pattern, binding_strict, false);
            self.check_pattern(&param.pattern);

            let mut names = Vec::new();
            Self::collect_bound_identifiers(&param.pattern, &mut names);
            for ident in names {
                if kind == ParameterListKind::Arrow
                    && self.interner.resolve(ident.name) == "enum"
                {
                    self.error(
                        "Binding name 'enum' is reserved in arrow parameters",
                        ident.span,
                    );
                }
                if check_duplicates && seen.contains(&ident.name) {
                    self.error(
                        format!(
                            "Duplicate parameter name '{}' is not allowed here",
                            self.interner.resolve(ident.name)
                        ),
                        ident.span,
                    );
                } else {
                    seen.push(ident.name);
                }
            }

            if let Some(default) = &param.default_value {
                if kind == ParameterListKind::Arrow {
                    self.check_arrow_parameter_default(default, binding_strict);
                }
                self.check_expr(default);
            }
        }
    }

    fn check_arrow_parameter_default(&mut self, expr: &Expression, binding_strict: bool) {
        if let Some(yield_ident) = self.find_identifier_reference_named(expr, "yield") {
            if self.current_function().is_some_and(|ctx| ctx.is_generator) {
                self.error(
                    "Yield is not allowed in arrow parameter defaults within generator code",
                    yield_ident.span,
                );
            } else if binding_strict {
                self.error(
                    "Identifier 'yield' is not allowed in strict arrow parameter defaults",
                    yield_ident.span,
                );
            }
        }
    }

    fn find_identifier_reference_named<'b>(
        &self,
        expr: &'b Expression,
        target: &str,
    ) -> Option<&'b Identifier> {
        match expr {
            Expression::Identifier(ident) => {
                (self.interner.resolve(ident.name) == target).then_some(ident)
            }
            Expression::Array(array) => array.elements.iter().flatten().find_map(|elem| match elem {
                ArrayElement::Expression(expr) | ArrayElement::Spread(expr) => {
                    self.find_identifier_reference_named(expr, target)
                }
            }),
            Expression::Object(obj) => obj.properties.iter().find_map(|prop| match prop {
                ObjectProperty::Property(prop) => {
                    if let PropertyKey::Computed(expr) = &prop.key {
                        self.find_identifier_reference_named(expr, target)
                            .or_else(|| self.find_identifier_reference_named(&prop.value, target))
                    } else {
                        self.find_identifier_reference_named(&prop.value, target)
                    }
                }
                ObjectProperty::Spread(spread) => {
                    self.find_identifier_reference_named(&spread.argument, target)
                }
            }),
            Expression::TemplateLiteral(tpl) => tpl.parts.iter().find_map(|part| match part {
                TemplatePart::Expression(expr) => self.find_identifier_reference_named(expr, target),
                TemplatePart::String(_) => None,
            }),
            Expression::Unary(unary) => self.find_identifier_reference_named(&unary.operand, target),
            Expression::Binary(binary) => self
                .find_identifier_reference_named(&binary.left, target)
                .or_else(|| self.find_identifier_reference_named(&binary.right, target)),
            Expression::Assignment(assign) => self
                .find_identifier_reference_named(&assign.left, target)
                .or_else(|| self.find_identifier_reference_named(&assign.right, target)),
            Expression::Logical(logical) => self
                .find_identifier_reference_named(&logical.left, target)
                .or_else(|| self.find_identifier_reference_named(&logical.right, target)),
            Expression::Conditional(cond) => self
                .find_identifier_reference_named(&cond.test, target)
                .or_else(|| self.find_identifier_reference_named(&cond.consequent, target))
                .or_else(|| self.find_identifier_reference_named(&cond.alternate, target)),
            Expression::Call(call) => self
                .find_identifier_reference_named(&call.callee, target)
                .or_else(|| {
                    call.arguments
                        .iter()
                        .find_map(|arg| self.find_identifier_reference_named(arg.expression(), target))
                }),
            Expression::AsyncCall(call) => self
                .find_identifier_reference_named(&call.callee, target)
                .or_else(|| {
                    call.arguments
                        .iter()
                        .find_map(|arg| self.find_identifier_reference_named(arg.expression(), target))
                }),
            Expression::Member(member) => {
                self.find_identifier_reference_named(&member.object, target)
            }
            Expression::Index(index) => self
                .find_identifier_reference_named(&index.object, target)
                .or_else(|| self.find_identifier_reference_named(&index.index, target)),
            Expression::New(new_expr) => self
                .find_identifier_reference_named(&new_expr.callee, target)
                .or_else(|| {
                    new_expr
                        .arguments
                        .iter()
                        .find_map(|arg| self.find_identifier_reference_named(arg.expression(), target))
                }),
            Expression::Await(await_expr) => {
                self.find_identifier_reference_named(&await_expr.argument, target)
            }
            Expression::Yield(yield_expr) => yield_expr
                .value
                .as_deref()
                .and_then(|value| self.find_identifier_reference_named(value, target)),
            Expression::Typeof(typeof_expr) => {
                self.find_identifier_reference_named(&typeof_expr.argument, target)
            }
            Expression::Parenthesized(paren) => {
                self.find_identifier_reference_named(&paren.expression, target)
            }
            Expression::JsxElement(elem) => {
                for attr in &elem.opening.attributes {
                    match attr {
                        JsxAttribute::Attribute { value, .. } => {
                            if let Some(value) = value {
                                if let Some(found) =
                                    self.find_identifier_reference_named_in_jsx_attr_value(
                                        value, target,
                                    )
                                {
                                    return Some(found);
                                }
                            }
                        }
                        JsxAttribute::Spread { argument, .. } => {
                            if let Some(found) =
                                self.find_identifier_reference_named(argument, target)
                            {
                                return Some(found);
                            }
                        }
                    }
                }
                elem.children
                    .iter()
                    .find_map(|child| self.find_identifier_reference_named_in_jsx_child(child, target))
            }
            Expression::JsxFragment(fragment) => fragment
                .children
                .iter()
                .find_map(|child| self.find_identifier_reference_named_in_jsx_child(child, target)),
            Expression::InstanceOf(instanceof) => self
                .find_identifier_reference_named(&instanceof.object, target),
            Expression::In(in_expr) => self
                .find_identifier_reference_named(&in_expr.property, target)
                .or_else(|| self.find_identifier_reference_named(&in_expr.object, target)),
            Expression::TypeCast(cast) => {
                self.find_identifier_reference_named(&cast.object, target)
            }
            Expression::TaggedTemplate(tagged) => self
                .find_identifier_reference_named(&tagged.tag, target)
                .or_else(|| {
                    tagged.template.parts.iter().find_map(|part| match part {
                        TemplatePart::Expression(expr) => {
                            self.find_identifier_reference_named(expr, target)
                        }
                        TemplatePart::String(_) => None,
                    })
                }),
            Expression::DynamicImport(import) => {
                self.find_identifier_reference_named(&import.source, target)
            }
            Expression::Arrow(_)
            | Expression::Function(_)
            | Expression::IntLiteral(_)
            | Expression::FloatLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::This(_)
            | Expression::NewTarget(_)
            | Expression::Super(_)
            | Expression::RegexLiteral(_) => None,
        }
    }

    fn find_identifier_reference_named_in_jsx_attr_value<'b>(
        &self,
        value: &'b JsxAttributeValue,
        target: &str,
    ) -> Option<&'b Identifier> {
        match value {
            JsxAttributeValue::StringLiteral(_) => None,
            JsxAttributeValue::Expression(expr) => {
                self.find_identifier_reference_named(expr, target)
            }
            JsxAttributeValue::JsxElement(elem) => elem
                .children
                .iter()
                .find_map(|child| self.find_identifier_reference_named_in_jsx_child(child, target)),
            JsxAttributeValue::JsxFragment(fragment) => fragment
                .children
                .iter()
                .find_map(|child| self.find_identifier_reference_named_in_jsx_child(child, target)),
        }
    }

    fn find_identifier_reference_named_in_jsx_child<'b>(
        &self,
        child: &'b JsxChild,
        target: &str,
    ) -> Option<&'b Identifier> {
        match child {
            JsxChild::Text(_) => None,
            JsxChild::Expression(expr) => expr
                .expression
                .as_ref()
                .and_then(|expr| self.find_identifier_reference_named(expr, target)),
            JsxChild::Element(elem) => {
                for attr in &elem.opening.attributes {
                    match attr {
                        JsxAttribute::Attribute { value, .. } => {
                            if let Some(value) = value {
                                if let Some(found) =
                                    self.find_identifier_reference_named_in_jsx_attr_value(
                                        value, target,
                                    )
                                {
                                    return Some(found);
                                }
                            }
                        }
                        JsxAttribute::Spread { argument, .. } => {
                            if let Some(found) =
                                self.find_identifier_reference_named(argument, target)
                            {
                                return Some(found);
                            }
                        }
                    }
                }
                elem.children
                    .iter()
                    .find_map(|nested| self.find_identifier_reference_named_in_jsx_child(nested, target))
            }
            JsxChild::Fragment(fragment) => fragment
                .children
                .iter()
                .find_map(|nested| self.find_identifier_reference_named_in_jsx_child(nested, target)),
        }
    }

    fn current_scope_index(&self) -> usize {
        self.scope_stack.len() - 1
    }

    fn current_scope(&self) -> &ScopeFrame {
        self.scope_stack
            .last()
            .expect("scope stack should never be empty")
    }

    fn current_scope_mut(&mut self) -> &mut ScopeFrame {
        self.scope_stack
            .last_mut()
            .expect("scope stack should never be empty")
    }

    fn push_scope<T>(&mut self, kind: ScopeKind, f: impl FnOnce(&mut Self) -> T) -> T {
        self.scope_stack.push(ScopeFrame {
            kind,
            ..ScopeFrame::default()
        });
        let result = f(self);
        self.scope_stack.pop();
        result
    }

    fn lookup_decl(
        entries: &[(Symbol, crate::parser::Span)],
        name: Symbol,
    ) -> Option<crate::parser::Span> {
        entries
            .iter()
            .find_map(|(symbol, span)| (*symbol == name).then_some(*span))
    }

    fn nearest_hoist_scope_index(&self) -> usize {
        self.scope_stack
            .iter()
            .enumerate()
            .rev()
            .find_map(|(idx, scope)| {
                matches!(scope.kind, ScopeKind::Module | ScopeKind::Function).then_some(idx)
            })
            .unwrap_or(0)
    }

    fn declare_param_identifier(&mut self, ident: &Identifier) {
        self.current_scope_mut()
            .params
            .push((ident.name, ident.span));
    }

    fn declare_catch_identifier(&mut self, ident: &Identifier) {
        self.current_scope_mut()
            .catch_params
            .push((ident.name, ident.span));
    }

    fn current_scope_is_function_body(&self) -> bool {
        self.current_scope().kind == ScopeKind::FunctionBody
    }

    fn declare_lexical_identifier(&mut self, ident: &Identifier, label: &str) {
        let current_idx = self.current_scope_index();
        let current = &self.scope_stack[current_idx];

        if Self::lookup_decl(&current.lexical, ident.name).is_some() {
            self.error(
                format!(
                    "Duplicate {} binding '{}'",
                    label,
                    self.interner.resolve(ident.name)
                ),
                ident.span,
            );
            return;
        }

        if current.kind == ScopeKind::Catch
            && Self::lookup_decl(&current.catch_params, ident.name).is_some()
        {
            self.error(
                format!(
                    "Duplicate {} binding '{}' conflicts with catch parameter",
                    label,
                    self.interner.resolve(ident.name)
                ),
                ident.span,
            );
            return;
        }

        if Self::lookup_decl(&current.vars, ident.name).is_some() {
            self.error(
                format!(
                    "Duplicate {} binding '{}' conflicts with var/function declaration",
                    label,
                    self.interner.resolve(ident.name)
                ),
                ident.span,
            );
            return;
        }

        if self.current_scope_is_function_body() {
            if let Some(parameter_scope) = self.scope_stack.get(current_idx.saturating_sub(1)) {
                if parameter_scope.kind == ScopeKind::Parameter
                    && Self::lookup_decl(&parameter_scope.params, ident.name).is_some()
                {
                    self.error(
                        format!(
                            "Duplicate {} binding '{}' conflicts with parameter",
                            label,
                            self.interner.resolve(ident.name)
                        ),
                        ident.span,
                    );
                    return;
                }
            }

            if let Some(function_scope) = self.scope_stack.get(current_idx.saturating_sub(2)) {
                if function_scope.kind == ScopeKind::Function
                    && Self::lookup_decl(&function_scope.vars, ident.name).is_some()
                {
                    self.error(
                        format!(
                            "Duplicate {} binding '{}' conflicts with var/function declaration",
                            label,
                            self.interner.resolve(ident.name)
                        ),
                        ident.span,
                    );
                    return;
                }
            }
        }

        if current.kind == ScopeKind::Module
            && Self::lookup_decl(&current.vars, ident.name).is_some()
        {
            self.error(
                format!(
                    "Duplicate {} binding '{}' conflicts with var/function declaration",
                    label,
                    self.interner.resolve(ident.name)
                ),
                ident.span,
            );
            return;
        }

        self.current_scope_mut()
            .lexical
            .push((ident.name, ident.span));
    }

    fn declare_var_identifier(&mut self, ident: &Identifier) {
        let hoist_idx = self.nearest_hoist_scope_index();
        for scope in self.scope_stack.iter().skip(hoist_idx) {
            if scope.kind == ScopeKind::Parameter {
                continue;
            }
            if Self::lookup_decl(&scope.lexical, ident.name).is_some()
                || Self::lookup_decl(&scope.catch_params, ident.name).is_some()
            {
                self.error(
                    format!(
                        "Var binding '{}' conflicts with lexical declaration",
                        self.interner.resolve(ident.name)
                    ),
                    ident.span,
                );
                return;
            }
        }

        let current_idx = self.current_scope_index();
        for (_, frame) in self
            .scope_stack
            .iter_mut()
            .enumerate()
            .skip(hoist_idx)
            .take(current_idx.saturating_sub(hoist_idx) + 1)
        {
            if frame.kind == ScopeKind::Parameter {
                continue;
            }
            if Self::lookup_decl(&frame.vars, ident.name).is_none() {
                frame.vars.push((ident.name, ident.span));
            }
        }

        let hoist_scope = &mut self.scope_stack[hoist_idx];
        if Self::lookup_decl(&hoist_scope.vars, ident.name).is_none() {
            hoist_scope.vars.push((ident.name, ident.span));
        }
    }

    fn declare_pattern_with(
        &mut self,
        pattern: &Pattern,
        mut record: impl FnMut(&mut Self, &Identifier),
    ) {
        let mut bound = Vec::new();
        Self::collect_bound_identifiers(pattern, &mut bound);
        for ident in bound {
            record(self, ident);
        }
    }

    fn is_iteration_statement(stmt: &Statement) -> bool {
        matches!(
            stmt,
            Statement::While(_)
                | Statement::DoWhile(_)
                | Statement::For(_)
                | Statement::ForOf(_)
                | Statement::ForIn(_)
        )
    }

    fn has_label(&self, name: Symbol) -> bool {
        self.label_stack
            .iter()
            .rev()
            .any(|label| label.name == name)
    }

    fn has_iteration_label(&self, name: Symbol) -> bool {
        self.label_stack
            .iter()
            .rev()
            .any(|label| label.name == name && label.is_iteration_target)
    }

    fn check_embedded_statement(&mut self, stmt: &Statement) {
        if matches!(stmt, Statement::FunctionDecl(_)) {
            self.error(
                "Function declarations are not allowed in this statement position",
                *stmt.span(),
            );
        }
        self.check_stmt(stmt);
    }

    fn check_stmt(&mut self, stmt: &Statement) {
        match stmt {
            Statement::VariableDecl(decl) => self.check_var_decl(decl),
            Statement::FunctionDecl(func) => {
                if matches!(
                    self.current_scope().kind,
                    ScopeKind::Block | ScopeKind::Catch
                ) {
                    self.declare_lexical_identifier(&func.name, "function");
                } else {
                    self.declare_var_identifier(&func.name);
                }
                self.check_function_decl(func);
            }
            Statement::ClassDecl(class) => {
                self.declare_lexical_identifier(&class.name, "class");
                self.check_class_decl(class);
            }
            Statement::TypeAliasDecl(_) | Statement::Empty(_) => {}
            Statement::ImportDecl(import) => self.check_import_decl(import),
            Statement::ExportDecl(export) => self.check_export_decl(export),
            Statement::Expression(expr) => self.check_expr(&expr.expression),
            Statement::If(if_stmt) => {
                self.check_expr(&if_stmt.condition);
                self.check_embedded_statement(&if_stmt.then_branch);
                if let Some(else_branch) = &if_stmt.else_branch {
                    self.check_embedded_statement(else_branch);
                }
            }
            Statement::Switch(switch_stmt) => {
                self.check_expr(&switch_stmt.discriminant);
                self.push_breakable(|this| {
                    for case in &switch_stmt.cases {
                        if let Some(test) = &case.test {
                            this.check_expr(test);
                        }
                        for stmt in &case.consequent {
                            this.check_stmt(stmt);
                        }
                    }
                });
            }
            Statement::While(while_stmt) => {
                self.check_expr(&while_stmt.condition);
                self.push_loop(|this| this.check_embedded_statement(&while_stmt.body));
            }
            Statement::DoWhile(do_while) => {
                self.push_loop(|this| {
                    this.check_embedded_statement(&do_while.body);
                    this.check_expr(&do_while.condition);
                });
            }
            Statement::With(with_stmt) => self.check_with(with_stmt),
            Statement::For(for_stmt) => {
                self.push_scope(ScopeKind::Block, |this| {
                    if let Some(init) = &for_stmt.init {
                        match init {
                            ForInit::VariableDecl(decl) => this_check_var_decl(this, decl),
                            ForInit::Expression(expr) => this.check_expr(expr),
                        }
                    }
                    if let Some(test) = &for_stmt.test {
                        this.check_expr(test);
                    }
                    if let Some(update) = &for_stmt.update {
                        this.check_expr(update);
                    }
                    this.push_loop(|this| this.check_embedded_statement(&for_stmt.body));
                });
            }
            Statement::ForOf(for_of) => {
                self.push_scope(ScopeKind::Block, |this| {
                    this.check_for_left(&for_of.left);
                    this.check_expr(&for_of.right);
                    this.push_loop(|this| this.check_embedded_statement(&for_of.body));
                });
            }
            Statement::ForIn(for_in) => {
                self.push_scope(ScopeKind::Block, |this| {
                    this.check_for_left(&for_in.left);
                    this.check_expr(&for_in.right);
                    this.push_loop(|this| this.check_embedded_statement(&for_in.body));
                });
            }
            Statement::Break(brk) => self.check_break(brk),
            Statement::Continue(cont) => self.check_continue(cont),
            Statement::Return(ret) => self.check_return(ret),
            Statement::Yield(yld) => self.check_yield(yld),
            Statement::Throw(thr) => self.check_expr(&thr.value),
            Statement::Try(try_stmt) => {
                self.check_block(&try_stmt.body);
                if let Some(catch) = &try_stmt.catch_clause {
                    self.push_scope(ScopeKind::Catch, |this| {
                        if let Some(param) = &catch.param {
                            this.check_pattern_bindings(param, this.current_strict(), true);
                            this.declare_pattern_with(param, |pass, ident| {
                                pass.declare_catch_identifier(ident);
                            });
                            this.check_pattern(param);
                        }
                        this.check_block_statements(&catch.body.statements);
                    });
                }
                if let Some(finally) = &try_stmt.finally_clause {
                    self.check_block(finally);
                }
            }
            Statement::Block(block) => self.check_block(block),
            Statement::Debugger(_) => {}
            Statement::Labeled(labeled) => {
                let is_iteration_target = Self::is_iteration_statement(&labeled.body);
                self.push_label(labeled.label.name, is_iteration_target, |this| {
                    this.check_embedded_statement(&labeled.body);
                });
            }
        }
    }

    fn check_export_decl(&mut self, export: &ExportDecl) {
        match export {
            ExportDecl::Declaration(stmt) => self.check_stmt(stmt),
            ExportDecl::Named { .. } | ExportDecl::All { .. } => {}
            ExportDecl::Default { expression, .. } => self.check_expr(expression),
        }
    }

    fn check_block(&mut self, block: &BlockStatement) {
        self.push_scope(ScopeKind::Block, |this| {
            this.check_block_statements(&block.statements)
        });
    }

    fn check_with(&mut self, with_stmt: &WithStatement) {
        if self.current_strict() {
            self.error("`with` is not allowed in strict mode", with_stmt.span);
        }
        self.check_expr(&with_stmt.object);
        self.check_stmt(&with_stmt.body);
    }

    fn check_block_statements(&mut self, statements: &[Statement]) {
        for stmt in statements {
            self.check_stmt(stmt);
        }
    }

    fn check_import_decl(&mut self, import: &ImportDecl) {
        for specifier in &import.specifiers {
            match specifier {
                ImportSpecifier::Named { alias, name } => {
                    self.declare_lexical_identifier(alias.as_ref().unwrap_or(name), "import");
                }
                ImportSpecifier::Namespace(name) | ImportSpecifier::Default(name) => {
                    self.declare_lexical_identifier(name, "import");
                }
            }
        }
    }

    fn check_function_decl(&mut self, func: &FunctionDecl) {
        let body_has_use_strict =
            Self::directive_prologue_is_strict(&func.body.statements, self.interner);
        let inherited_lexical = self.current_lexical();
        let body_strict = inherited_lexical.strict || body_has_use_strict;
        if body_strict {
            self.check_strict_binding_name(&func.name);
        }
        self.push_function(
            func.is_async,
            func.is_generator,
            LexicalContext {
                super_property_allowed: inherited_lexical.super_property_allowed
                    || self.allow_super_property,
                super_call_allowed: inherited_lexical.super_call_allowed,
                strict: body_strict,
            },
            |this| {
                this.push_scope(ScopeKind::Parameter, |this| {
                    this.check_parameter_list(
                        &func.params,
                        body_strict,
                        body_has_use_strict,
                        func.span,
                        ParameterListKind::OrdinaryFunction,
                    );
                    for param in &func.params {
                        this.declare_pattern_with(&param.pattern, |pass, ident| {
                            pass.declare_param_identifier(ident);
                        });
                    }

                    this.push_scope(ScopeKind::FunctionBody, |this| {
                        this.check_block_statements(&func.body.statements);
                    });
                });
            },
        );
    }

    fn check_function_expr(&mut self, func: &FunctionExpression) {
        let body_has_use_strict =
            Self::directive_prologue_is_strict(&func.body.statements, self.interner);
        let lexical = LexicalContext {
            super_property_allowed: self.current_lexical().super_property_allowed
                || self.allow_super_property
                || func.is_method,
            ..self.current_lexical()
        };
        let body_strict = lexical.strict || body_has_use_strict;
        if let Some(name) = &func.name {
            if body_strict {
                self.check_strict_binding_name(name);
            }
        }
        self.push_function(func.is_async, func.is_generator, lexical, |this| {
            if let Some(current) = this.lexical_stack.last_mut() {
                current.strict = body_strict;
            }
            this.push_scope(ScopeKind::Parameter, |this| {
                this.check_parameter_list(
                    &func.params,
                    body_strict,
                    body_has_use_strict,
                    func.span,
                    ParameterListKind::OrdinaryFunction,
                );
                for param in &func.params {
                    this.declare_pattern_with(&param.pattern, |pass, ident| {
                        pass.declare_param_identifier(ident);
                    });
                }

                this.push_scope(ScopeKind::FunctionBody, |this| {
                    this.check_block_statements(&func.body.statements);
                });
            });
        });
    }

    fn check_arrow_function(&mut self, arrow: &ArrowFunction) {
        let body_has_use_strict = match &arrow.body {
            ArrowBody::Block(block) => {
                Self::directive_prologue_is_strict(&block.statements, self.interner)
            }
            ArrowBody::Expression(_) => false,
        };
        let body_strict = self.current_strict() || body_has_use_strict;
        self.check_parameter_list(
            &arrow.params,
            body_strict,
            body_has_use_strict,
            arrow.span,
            ParameterListKind::Arrow,
        );
        self.push_function(
            arrow.is_async,
            false,
            LexicalContext {
                strict: body_strict,
                ..self.current_lexical()
            },
            |this| {
                this.push_scope(ScopeKind::Parameter, |this| {
                    for param in &arrow.params {
                        this.declare_pattern_with(&param.pattern, |pass, ident| {
                            pass.declare_param_identifier(ident);
                        });
                    }

                    match &arrow.body {
                        ArrowBody::Expression(expr) => this.check_expr(expr),
                        ArrowBody::Block(block) => {
                            this.push_scope(ScopeKind::FunctionBody, |this| {
                                this.check_block_statements(&block.statements);
                            });
                        }
                    }
                });
            },
        );
    }

    fn check_class_decl(&mut self, class: &ClassDecl) {
        let has_super_class = class.extends.is_some();
        let mut constructor_count = 0usize;
        if let Some(extends) = &class.extends {
            self.check_type_annotation_exprs(extends);
        }
        for implement in &class.implements {
            self.check_type_annotation_exprs(implement);
        }
        self.push_scope(ScopeKind::Class, |this| {
            for member in &class.members {
                match member {
                    ClassMember::Field(field) => {
                        if let Some(initializer) = &field.initializer {
                            this.push_lexical(
                                LexicalContext {
                                    super_property_allowed: true,
                                    super_call_allowed: false,
                                    strict: true,
                                },
                                |this| this.check_expr(initializer),
                            );
                        }
                    }
                    ClassMember::Method(method) => {
                        match method.kind {
                            MethodKind::Getter if !method.params.is_empty() => {
                                this.error("Getter must not declare parameters", method.span)
                            }
                            MethodKind::Setter if method.params.len() != 1 => {
                                this.error("Setter must declare exactly one parameter", method.span)
                            }
                            _ => {}
                        }
                        this.check_parameter_list(
                            &method.params,
                            true,
                            false,
                            method.span,
                            ParameterListKind::OrdinaryFunction,
                        );
                        if let Some(body) = &method.body {
                            this.push_function(
                                method.is_async,
                                method.is_generator,
                                LexicalContext {
                                    super_property_allowed: true,
                                    super_call_allowed: false,
                                    strict: true,
                                },
                                |this| {
                                    this.push_scope(ScopeKind::Parameter, |this| {
                                        for param in &method.params {
                                            this.declare_pattern_with(
                                                &param.pattern,
                                                |pass, ident| {
                                                    pass.declare_param_identifier(ident);
                                                },
                                            );
                                        }
                                        this.push_scope(ScopeKind::FunctionBody, |this| {
                                            this.check_block_statements(&body.statements);
                                        });
                                    });
                                },
                            );
                        }
                    }
                    ClassMember::Constructor(ctor) => {
                        constructor_count += 1;
                        if constructor_count > 1 {
                            this.error("Class must not declare multiple constructors", ctor.span);
                        }
                        this.check_parameter_list(
                            &ctor.params,
                            true,
                            false,
                            ctor.span,
                            ParameterListKind::OrdinaryFunction,
                        );
                        this.push_function(
                            false,
                            false,
                            LexicalContext {
                                super_property_allowed: true,
                                super_call_allowed: has_super_class,
                                strict: true,
                            },
                            |this| {
                                this.push_scope(ScopeKind::Parameter, |this| {
                                    for param in &ctor.params {
                                        this.declare_pattern_with(&param.pattern, |pass, ident| {
                                            pass.declare_param_identifier(ident);
                                        });
                                    }
                                    this.push_scope(ScopeKind::FunctionBody, |this| {
                                        this.check_block_statements(&ctor.body.statements);
                                    });
                                });
                            },
                        );
                    }
                    ClassMember::StaticBlock(block) => this.push_lexical(
                        LexicalContext {
                            super_property_allowed: true,
                            super_call_allowed: false,
                            strict: true,
                        },
                        |this| {
                            this.push_scope(ScopeKind::StaticBlock, |this| {
                                this.check_block_statements(&block.statements);
                            });
                        },
                    ),
                }
            }
        });
    }

    fn check_var_decl(&mut self, decl: &VariableDecl) {
        self.check_pattern_bindings(&decl.pattern, self.current_strict(), true);
        match decl.kind {
            VariableKind::Var => self.declare_pattern_with(&decl.pattern, |pass, ident| {
                pass.declare_var_identifier(ident);
            }),
            VariableKind::Let | VariableKind::Const => {
                self.declare_pattern_with(&decl.pattern, |pass, ident| {
                    pass.declare_lexical_identifier(ident, "variable");
                })
            }
        }
        self.check_pattern(&decl.pattern);
        if let Some(init) = &decl.initializer {
            self.check_expr(init);
        }
    }

    fn check_for_left(&mut self, left: &ForOfLeft) {
        match left {
            ForOfLeft::VariableDecl(decl) => self.check_var_decl(decl),
            ForOfLeft::Pattern(pattern) => {
                self.check_pattern_bindings(pattern, self.current_strict(), true);
                self.check_pattern(pattern);
            }
        }
    }

    fn check_pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Identifier(_) => {}
            Pattern::Array(array) => {
                for elem in array.elements.iter().flatten() {
                    self.check_pattern(&elem.pattern);
                    if let Some(default) = &elem.default {
                        self.check_expr(default);
                    }
                }
                if let Some(rest) = &array.rest {
                    self.check_pattern(rest);
                }
            }
            Pattern::Object(obj) => {
                for prop in &obj.properties {
                    if let PropertyKey::Computed(expr) = &prop.key {
                        self.check_expr(expr);
                    }
                    self.check_pattern(&prop.value);
                    if let Some(default) = &prop.default {
                        self.check_expr(default);
                    }
                }
            }
            Pattern::Rest(rest) => self.check_pattern(&rest.argument),
        }
    }

    fn check_break(&mut self, brk: &BreakStatement) {
        if let Some(label) = &brk.label {
            if !self.has_label(label.name) {
                self.error(
                    format!(
                        "Undefined label '{}' for break statement",
                        self.interner.resolve(label.name)
                    ),
                    brk.span,
                );
            }
        } else if self.breakable_depth == 0 {
            self.error("Break statement outside of loop or switch", brk.span);
        }
    }

    fn check_continue(&mut self, cont: &ContinueStatement) {
        if let Some(label) = &cont.label {
            if !self.has_iteration_label(label.name) {
                self.error(
                    format!(
                        "Continue target '{}' is not an iteration label",
                        self.interner.resolve(label.name)
                    ),
                    cont.span,
                );
            }
        } else if self.loop_depth == 0 {
            self.error("Continue statement outside of loop", cont.span);
        }
    }

    fn check_return(&mut self, ret: &ReturnStatement) {
        if self.current_function().is_none() && !self.allow_top_level_return {
            self.error("Return statement outside of function", ret.span);
        }
        if let Some(value) = &ret.value {
            self.check_expr(value);
        }
    }

    fn check_yield(&mut self, yld: &YieldStatement) {
        if self.mode != TypeSystemMode::Raya
            && !self.current_function().is_some_and(|ctx| ctx.is_generator)
        {
            self.error("Yield statement outside of generator function", yld.span);
        }
        if let Some(value) = &yld.value {
            self.check_expr(value);
        }
    }

    fn check_yield_expression(&mut self, yld: &YieldExpression) {
        if self.mode != TypeSystemMode::Raya
            && !self.current_function().is_some_and(|ctx| ctx.is_generator)
        {
            self.error("Yield expression outside of generator function", yld.span);
        }
        if let Some(value) = &yld.value {
            self.check_expr(value);
        }
    }

    fn check_assignment_target(&mut self, expr: &Expression) {
        if !Self::is_valid_assignment_target(expr, false) {
            self.error("Invalid assignment target", *expr.span());
            return;
        }

        match expr {
            Expression::Identifier(ident) => {
                if self.current_strict() && self.is_restricted_strict_binding_name(ident) {
                    self.error(
                        format!(
                            "Assignment to '{}' is not allowed in strict mode",
                            self.interner.resolve(ident.name)
                        ),
                        ident.span,
                    );
                }
            }
            Expression::Parenthesized(paren) => self.check_assignment_target(&paren.expression),
            Expression::Assignment(assign) => {
                if assign.operator != AssignmentOperator::Assign {
                    self.error("Invalid assignment target", assign.span);
                    return;
                }
                self.check_assignment_pattern_target(&assign.left);
                self.check_expr(&assign.right);
            }
            Expression::Array(array) => {
                for (index, elem) in array.elements.iter().enumerate() {
                    let Some(elem) = elem else {
                        continue;
                    };
                    match elem {
                        ArrayElement::Expression(expr) => self.check_assignment_pattern_target(expr),
                        ArrayElement::Spread(expr) => {
                            if matches!(expr, Expression::Assignment(_)) {
                                self.error(
                                    "Rest element must not have an initializer",
                                    *expr.span(),
                                );
                            }
                            if index + 1 != array.elements.len() {
                                self.error(
                                    "Rest element must be the last element in an assignment pattern",
                                    *expr.span(),
                                );
                            }
                            self.check_assignment_pattern_target(expr);
                        }
                    }
                }
            }
            Expression::Object(obj) => {
                for (index, prop) in obj.properties.iter().enumerate() {
                    match prop {
                        ObjectProperty::Property(prop) => {
                            self.check_assignment_pattern_target(&prop.value)
                        }
                        ObjectProperty::Spread(spread) => {
                            if index + 1 != obj.properties.len() {
                                self.error(
                                    "Rest property must be the last property in an assignment pattern",
                                    spread.span,
                                );
                            }
                            self.check_assignment_pattern_target(&spread.argument);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn check_assignment_pattern_target(&mut self, expr: &Expression) {
        if !Self::is_valid_assignment_target(expr, true) {
            self.error("Invalid assignment target", *expr.span());
            return;
        }
        match expr {
            Expression::Identifier(ident) => {
                if self.current_strict() && self.is_restricted_strict_binding_name(ident) {
                    self.error(
                        format!(
                            "Assignment to '{}' is not allowed in strict mode",
                            self.interner.resolve(ident.name)
                        ),
                        ident.span,
                    );
                }
            }
            Expression::Parenthesized(paren) => {
                self.check_assignment_pattern_target(&paren.expression)
            }
            Expression::Assignment(assign) => {
                if assign.operator != AssignmentOperator::Assign {
                    self.error("Invalid assignment target", assign.span);
                    return;
                }
                self.check_assignment_pattern_target(&assign.left);
                self.check_expr(&assign.right);
            }
            Expression::Array(array) => {
                for (index, elem) in array.elements.iter().enumerate() {
                    let Some(elem) = elem else {
                        continue;
                    };
                    match elem {
                        ArrayElement::Expression(expr) => {
                            self.check_assignment_pattern_target(expr)
                        }
                        ArrayElement::Spread(expr) => {
                            if matches!(expr, Expression::Assignment(_)) {
                                self.error(
                                    "Rest element must not have an initializer",
                                    *expr.span(),
                                );
                            }
                            if index + 1 != array.elements.len() {
                                self.error(
                                    "Rest element must be the last element in an assignment pattern",
                                    *expr.span(),
                                );
                            }
                            self.check_assignment_pattern_target(expr);
                        }
                    }
                }
            }
            Expression::Object(obj) => {
                for (index, prop) in obj.properties.iter().enumerate() {
                    match prop {
                        ObjectProperty::Property(prop) => {
                            self.check_assignment_pattern_target(&prop.value)
                        }
                        ObjectProperty::Spread(spread) => {
                            if index + 1 != obj.properties.len() {
                                self.error(
                                    "Rest property must be the last property in an assignment pattern",
                                    spread.span,
                                );
                            }
                            self.check_assignment_pattern_target(&spread.argument);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn is_valid_assignment_target(expr: &Expression, allow_pattern_defaults: bool) -> bool {
        match expr {
            Expression::Identifier(_) => true,
            Expression::Member(member) => !member.optional,
            Expression::Index(index) => !index.optional,
            Expression::Parenthesized(paren) => {
                Self::is_valid_assignment_target(&paren.expression, allow_pattern_defaults)
            }
            Expression::Assignment(assign) => {
                allow_pattern_defaults
                    && assign.operator == AssignmentOperator::Assign
                    && Self::is_valid_assignment_target(&assign.left, true)
            }
            Expression::Array(array) => array.elements.iter().flatten().all(|elem| match elem {
                ArrayElement::Expression(expr) | ArrayElement::Spread(expr) => {
                    Self::is_valid_assignment_target(expr, true)
                }
            }),
            Expression::Object(obj) => obj.properties.iter().all(|prop| match prop {
                ObjectProperty::Property(prop) => {
                    Self::is_valid_assignment_target(&prop.value, true)
                }
                ObjectProperty::Spread(spread) => {
                    Self::is_valid_assignment_target(&spread.argument, true)
                }
            }),
            _ => false,
        }
    }

    fn is_unqualified_identifier_reference(expr: &Expression) -> bool {
        match expr {
            Expression::Identifier(_) => true,
            Expression::Parenthesized(paren) => {
                Self::is_unqualified_identifier_reference(&paren.expression)
            }
            _ => false,
        }
    }

    fn check_expr(&mut self, expr: &Expression) {
        self.check_expr_with_super_policy(expr, false);
    }

    fn check_expr_allowing_super_operand(&mut self, expr: &Expression) {
        self.check_expr_with_super_policy(expr, true);
    }

    fn check_expr_with_super_policy(&mut self, expr: &Expression, allow_super_operand: bool) {
        match expr {
            Expression::Identifier(ident) => {
                let name = self.interner.resolve(ident.name);
                if name == "yield"
                    && self.mode != TypeSystemMode::Raya
                    && (self.current_strict()
                        || self.current_function().is_some_and(|ctx| ctx.is_generator))
                {
                    self.error(
                        "Identifier 'yield' is not allowed in this context",
                        ident.span,
                    );
                }
            }
            Expression::IntLiteral(_)
            | Expression::FloatLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::This(_)
            | Expression::RegexLiteral(_) => {}
            Expression::NewTarget(span) => {
                if !self.allow_new_target && self.current_function().is_none() {
                    self.error(
                        "`new.target` is only valid inside non-arrow function code",
                        *span,
                    );
                }
            }
            Expression::Super(span) => {
                if !allow_super_operand {
                    self.error(
                        "Bare `super` is only valid in property access or super(...) calls",
                        *span,
                    );
                }
            }
            Expression::TemplateLiteral(tpl) => {
                for part in &tpl.parts {
                    if let TemplatePart::Expression(expr) = part {
                        self.check_expr(expr);
                    }
                }
            }
            Expression::Array(array) => {
                for elem in array.elements.iter().flatten() {
                    match elem {
                        ArrayElement::Expression(expr) | ArrayElement::Spread(expr) => {
                            self.check_expr(expr);
                        }
                    }
                }
            }
            Expression::Object(obj) => {
                for prop in &obj.properties {
                    match prop {
                        ObjectProperty::Property(prop) => {
                            if let PropertyKey::Computed(expr) = &prop.key {
                                self.check_expr(expr);
                            }
                            self.check_expr(&prop.value);
                        }
                        ObjectProperty::Spread(spread) => self.check_expr(&spread.argument),
                    }
                }
            }
            Expression::Unary(unary) => {
                if unary.operator == UnaryOperator::Delete
                    && self.current_strict()
                    && Self::is_unqualified_identifier_reference(&unary.operand)
                {
                    self.error(
                        "Deleting an unqualified identifier is not allowed in strict mode",
                        unary.span,
                    );
                }
                if matches!(
                    unary.operator,
                    UnaryOperator::PrefixIncrement
                        | UnaryOperator::PrefixDecrement
                        | UnaryOperator::PostfixIncrement
                        | UnaryOperator::PostfixDecrement
                ) {
                    self.check_assignment_target(&unary.operand);
                }
                self.check_expr(&unary.operand);
            }
            Expression::Binary(binary) => {
                self.check_expr(&binary.left);
                self.check_expr(&binary.right);
            }
            Expression::Assignment(assign) => {
                self.check_assignment_target(&assign.left);
                self.check_expr(&assign.left);
                self.check_expr(&assign.right);
            }
            Expression::Logical(logical) => {
                self.check_expr(&logical.left);
                self.check_expr(&logical.right);
            }
            Expression::Conditional(cond) => {
                self.check_expr(&cond.test);
                self.check_expr(&cond.consequent);
                self.check_expr(&cond.alternate);
            }
            Expression::Call(call) => {
                if matches!(call.callee.as_ref(), Expression::Super(_))
                    && !self.current_lexical().super_call_allowed
                {
                    self.error(
                        "`super(...)` is only valid in a derived class constructor",
                        call.span,
                    );
                }
                self.check_expr_allowing_super_operand(&call.callee);
                for arg in &call.arguments {
                    self.check_expr(arg.expression());
                }
            }
            Expression::AsyncCall(call) => {
                self.check_expr_allowing_super_operand(&call.callee);
                for arg in &call.arguments {
                    self.check_expr(arg.expression());
                }
            }
            Expression::Member(member) => {
                if matches!(member.object.as_ref(), Expression::Super(_))
                    && !self.current_lexical().super_property_allowed
                    && !self.allow_super_property
                {
                    self.error(
                        "`super` property access is only valid inside derived class members",
                        member.span,
                    );
                }
                self.check_expr_allowing_super_operand(&member.object);
            }
            Expression::Index(index) => {
                if matches!(index.object.as_ref(), Expression::Super(_))
                    && !self.current_lexical().super_property_allowed
                    && !self.allow_super_property
                {
                    self.error(
                        "`super` property access is only valid inside derived class members",
                        index.span,
                    );
                }
                self.check_expr_allowing_super_operand(&index.object);
                self.check_expr(&index.index);
            }
            Expression::New(new_expr) => {
                self.check_expr(&new_expr.callee);
                for arg in &new_expr.arguments {
                    self.check_expr(arg.expression());
                }
            }
            Expression::Arrow(arrow) => self.check_arrow_function(arrow),
            Expression::Function(func) => self.check_function_expr(func),
            Expression::Await(await_expr) => {
                if !self.current_function().is_some_and(|ctx| ctx.is_async)
                    && !self.allow_await_outside_async
                {
                    self.error(
                        "Await expression outside of async function",
                        await_expr.span,
                    );
                }
                self.check_expr(&await_expr.argument);
            }
            Expression::Yield(yield_expr) => self.check_yield_expression(yield_expr),
            Expression::Typeof(typeof_expr) => self.check_expr(&typeof_expr.argument),
            Expression::Parenthesized(paren) => self.check_expr(&paren.expression),
            Expression::JsxElement(elem) => {
                for attr in &elem.opening.attributes {
                    match attr {
                        JsxAttribute::Attribute { value, .. } => {
                            if let Some(value) = value {
                                self.check_jsx_attr_value(value);
                            }
                        }
                        JsxAttribute::Spread { argument, .. } => self.check_expr(argument),
                    }
                }
                for child in &elem.children {
                    self.check_jsx_child(child);
                }
            }
            Expression::JsxFragment(fragment) => {
                for child in &fragment.children {
                    self.check_jsx_child(child);
                }
            }
            Expression::InstanceOf(instanceof) => self.check_expr(&instanceof.object),
            Expression::In(in_expr) => {
                self.check_expr(&in_expr.property);
                self.check_expr(&in_expr.object);
            }
            Expression::TypeCast(cast) => self.check_expr(&cast.object),
            Expression::TaggedTemplate(tagged) => {
                self.check_expr(&tagged.tag);
                for part in &tagged.template.parts {
                    if let TemplatePart::Expression(expr) = part {
                        self.check_expr(expr);
                    }
                }
            }
            Expression::DynamicImport(import) => self.check_expr(&import.source),
        }
    }

    fn check_jsx_child(&mut self, child: &JsxChild) {
        match child {
            JsxChild::Element(elem) => self.check_expr(&Expression::JsxElement(elem.clone())),
            JsxChild::Fragment(fragment) => {
                self.check_expr(&Expression::JsxFragment(fragment.clone()))
            }
            JsxChild::Expression(expr) => {
                if let Some(expr) = &expr.expression {
                    self.check_expr(expr);
                }
            }
            JsxChild::Text(_) => {}
        }
    }

    fn check_jsx_attr_value(&mut self, value: &JsxAttributeValue) {
        match value {
            JsxAttributeValue::StringLiteral(_) => {}
            JsxAttributeValue::Expression(expr) => self.check_expr(expr),
            JsxAttributeValue::JsxElement(elem) => {
                self.check_expr(&Expression::JsxElement((**elem).clone()))
            }
            JsxAttributeValue::JsxFragment(fragment) => {
                self.check_expr(&Expression::JsxFragment((**fragment).clone()))
            }
        }
    }

    fn check_type_annotation_exprs(&mut self, _ty: &TypeAnnotation) {}
}

fn this_check_var_decl(pass: &mut EarlyErrorPass<'_>, decl: &VariableDecl) {
    pass.check_var_decl(decl);
}

#[cfg(test)]
mod tests {
    use super::{check_early_errors, check_early_errors_with_options, EarlyErrorOptions};
    use crate::parser::checker::TypeSystemMode;
    use crate::parser::Parser;

    fn parse_module(source: &str) -> (crate::parser::ast::Module, crate::parser::Interner) {
        let parser = Parser::new(source).expect("should lex");
        parser.parse().expect("should parse")
    }

    #[test]
    fn test_return_outside_function_is_early_error() {
        let (module, interner) = parse_module("return 1;");
        let errors = check_early_errors_with_options(
            &module,
            &interner,
            EarlyErrorOptions {
                mode: TypeSystemMode::Ts,
                allow_top_level_return: false,
                allow_await_outside_async: false,
            },
        )
        .expect_err("expected early error");
        assert!(errors[0]
            .message
            .contains("Return statement outside of function"));
    }

    #[test]
    fn test_top_level_return_allowed_in_raya_entry_mode() {
        let (module, interner) = parse_module("return 1;");
        check_early_errors(&module, &interner, TypeSystemMode::Raya).expect("should pass");
    }

    #[test]
    fn test_top_level_return_rejected_in_js_mode() {
        let (module, interner) = parse_module("return 1;");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Js)
            .expect_err("expected early error");
        assert!(errors[0]
            .message
            .contains("Return statement outside of function"));
    }

    #[test]
    fn test_top_level_await_rejected_by_default() {
        let (module, interner) = parse_module("await value;");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Js)
            .expect_err("expected early error");
        assert!(errors[0]
            .message
            .contains("Await expression outside of async function"));
    }

    #[test]
    fn test_top_level_await_allowed_with_entry_options() {
        let (module, interner) = parse_module("await value;");
        check_early_errors_with_options(
            &module,
            &interner,
            EarlyErrorOptions {
                mode: TypeSystemMode::Js,
                allow_top_level_return: true,
                allow_await_outside_async: true,
            },
        )
        .expect("should pass");
    }

    #[test]
    fn test_break_unknown_label_is_early_error() {
        let (module, interner) = parse_module("while (true) { break missing; }");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Ts)
            .expect_err("expected early error");
        assert!(errors[0].message.contains("Undefined label"));
    }

    #[test]
    fn test_continue_non_iteration_label_is_early_error() {
        let (module, interner) = parse_module("target: { continue target; }");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Ts)
            .expect_err("expected early error");
        assert!(errors[0]
            .message
            .contains("Continue target 'target' is not an iteration label"));
    }

    #[test]
    fn test_yield_outside_generator_is_early_error() {
        let (module, interner) = parse_module("function f() { yield 1; }");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Ts)
            .expect_err("expected early error");
        assert!(errors[0]
            .message
            .contains("Yield statement outside of generator function"));
    }

    #[test]
    fn test_await_outside_async_is_early_error() {
        let (module, interner) = parse_module("function f() { return await x; }");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Ts)
            .expect_err("expected early error");
        assert!(errors[0]
            .message
            .contains("Await expression outside of async function"));
    }

    #[test]
    fn test_labeled_break_and_continue_in_loop_are_allowed() {
        let (module, interner) =
            parse_module("outer: while (true) { continue outer; break outer; }");
        check_early_errors(&module, &interner, TypeSystemMode::Ts).expect("should pass");
    }

    #[test]
    fn test_invalid_assignment_target_is_early_error() {
        let (module, interner) = parse_module("1 = value;");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Ts)
            .expect_err("expected early error");
        assert!(errors[0].message.contains("Invalid assignment target"));
    }

    #[test]
    fn test_invalid_update_target_is_early_error() {
        let (module, interner) = parse_module("call()++;");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Ts)
            .expect_err("expected early error");
        assert!(errors[0].message.contains("Invalid assignment target"));
    }

    #[test]
    fn test_super_call_outside_derived_constructor_is_early_error() {
        let (module, interner) =
            parse_module("class Base {} class Child extends Base { method() { super(); } }");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Ts)
            .expect_err("expected early error");
        assert!(errors[0]
            .message
            .contains("`super(...)` is only valid in a derived class constructor"));
    }

    #[test]
    fn test_super_property_outside_derived_member_is_early_error() {
        let (module, interner) = parse_module("super.value;");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Ts)
            .expect_err("expected early error");
        assert!(errors[0]
            .message
            .contains("`super` property access is only valid inside derived class members"));
    }

    #[test]
    fn test_super_call_in_derived_constructor_is_allowed() {
        let (module, interner) =
            parse_module("class Base {} class Child extends Base { constructor() { super(); } }");
        check_early_errors(&module, &interner, TypeSystemMode::Ts).expect("should pass");
    }

    #[test]
    fn test_super_property_in_derived_method_is_allowed() {
        let (module, interner) =
            parse_module("class Base { value() {} } class Child extends Base { method() { return super.value; } }");
        check_early_errors(&module, &interner, TypeSystemMode::Ts).expect("should pass");
    }

    #[test]
    fn test_multiple_constructors_is_early_error() {
        let (module, interner) =
            parse_module("class Example { constructor() {} constructor(value: number) {} }");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Ts)
            .expect_err("expected early error");
        assert!(errors[0]
            .message
            .contains("Class must not declare multiple constructors"));
    }

    #[test]
    fn test_getter_with_parameter_is_early_error() {
        let (module, interner) =
            parse_module("class Example { get value(x: number) { return x; } }");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Ts)
            .expect_err("expected early error");
        assert!(errors[0]
            .message
            .contains("Getter must not declare parameters"));
    }

    #[test]
    fn test_setter_without_single_parameter_is_early_error() {
        let (module, interner) = parse_module("class Example { set value() {} }");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Ts)
            .expect_err("expected early error");
        assert!(errors[0]
            .message
            .contains("Setter must declare exactly one parameter"));
    }

    #[test]
    fn test_use_strict_forbids_eval_binding() {
        let (module, interner) = parse_module("\"use strict\"; let eval = 1;");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Ts)
            .expect_err("expected early error");
        assert!(errors[0]
            .message
            .contains("Binding name 'eval' is not allowed in strict mode"));
    }

    #[test]
    fn test_use_strict_forbids_assignment_to_arguments() {
        let (module, interner) = parse_module("function f() { \"use strict\"; arguments = 1; }");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Ts)
            .expect_err("expected early error");
        assert!(errors.iter().any(|error| error
            .message
            .contains("Assignment to 'arguments' is not allowed in strict mode")));
    }

    #[test]
    fn test_strict_function_expression_inherits_strictness_for_arguments_assignment() {
        let (module, interner) =
            parse_module("\"use strict\"; (function named() { arguments = 1; })();");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Ts)
            .expect_err("expected early error");
        assert!(errors.iter().any(|error| error
            .message
            .contains("Assignment to 'arguments' is not allowed in strict mode")));
    }

    #[test]
    fn test_use_strict_forbids_duplicate_parameter_names() {
        let (module, interner) = parse_module("function f(a, a) { \"use strict\"; }");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Ts)
            .expect_err("expected early error");
        assert!(errors.iter().any(|error| error
            .message
            .contains("Duplicate parameter name 'a' is not allowed here")));
    }

    #[test]
    fn test_use_strict_forbids_non_simple_parameter_list() {
        let (module, interner) = parse_module("function f(a = 1) { \"use strict\"; }");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Ts)
            .expect_err("expected early error");
        assert!(errors.iter().any(|error| error.message.contains(
            "Illegal 'use strict' directive in function with non-simple parameter list"
        )));
    }

    #[test]
    fn test_strict_delete_identifier_is_early_error() {
        let (module, interner) = parse_module("function f(x) { \"use strict\"; delete x; }");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Ts)
            .expect_err("expected early error");
        assert!(errors.iter().any(|error| error
            .message
            .contains("Deleting an unqualified identifier is not allowed in strict mode")));
    }

    #[test]
    fn test_class_methods_are_strict_for_parameter_names() {
        let (module, interner) = parse_module("class Example { method(arguments) {} }");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Ts)
            .expect_err("expected early error");
        assert!(errors.iter().any(|error| error
            .message
            .contains("Binding name 'arguments' is not allowed in strict mode")));
    }

    #[test]
    fn test_duplicate_let_is_early_error() {
        let (module, interner) = parse_module("let x = 1; let x = 2;");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Ts)
            .expect_err("expected early error");
        assert!(errors
            .iter()
            .any(|error| error.message.contains("Duplicate variable binding 'x'")));
    }

    #[test]
    fn test_var_let_collision_across_block_is_early_error() {
        let (module, interner) = parse_module("let x = 1; { var x = 2; }");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Ts)
            .expect_err("expected early error");
        assert!(errors.iter().any(|error| error
            .message
            .contains("Var binding 'x' conflicts with lexical declaration")));
    }

    #[test]
    fn test_function_body_let_conflicts_with_parameter() {
        let (module, interner) = parse_module("function f(a) { let a = 1; }");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Ts)
            .expect_err("expected early error");
        assert!(errors
            .iter()
            .any(|error| error.message.contains("conflicts with parameter")));
    }

    #[test]
    fn test_nested_block_let_can_shadow_parameter() {
        let (module, interner) = parse_module("function f(a) { { let a = 1; } }");
        check_early_errors(&module, &interner, TypeSystemMode::Ts).expect("should pass");
    }

    #[test]
    fn test_sibling_for_loop_let_bindings_are_isolated() {
        let (module, interner) = parse_module(
            "for (let i = 0; i < 1; i = i + 1) {} for (let i = 0; i < 1; i = i + 1) {}",
        );
        check_early_errors(&module, &interner, TypeSystemMode::Ts).expect("should pass");
    }

    #[test]
    fn test_catch_binding_conflict_is_early_error() {
        let (module, interner) = parse_module("try {} catch (err) { let err = 1; }");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Ts)
            .expect_err("expected early error");
        assert!(errors
            .iter()
            .any(|error| error.message.contains("conflicts with catch parameter")));
    }
}

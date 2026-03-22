//! Early-error legality pass
//!
//! This pass runs after parsing but before binding/type checking. It enforces
//! syntax-context rules that should be classified as parse/early errors instead
//! of checker errors.

use super::TypeSystemMode;
use crate::parser::ast::*;
use crate::parser::{Interner, ParseError, Symbol};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FunctionContext {
    is_async: bool,
    is_generator: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct LexicalContext {
    super_property_allowed: bool,
    super_call_allowed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LabelContext {
    name: Symbol,
    is_iteration_target: bool,
}

pub fn check_early_errors(
    module: &Module,
    interner: &Interner,
    mode: TypeSystemMode,
) -> Result<(), Vec<ParseError>> {
    let mut pass = EarlyErrorPass::new(interner, mode);
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
    errors: Vec<ParseError>,
    function_stack: Vec<FunctionContext>,
    lexical_stack: Vec<LexicalContext>,
    label_stack: Vec<LabelContext>,
    loop_depth: usize,
    breakable_depth: usize,
}

impl<'a> EarlyErrorPass<'a> {
    fn new(interner: &'a Interner, mode: TypeSystemMode) -> Self {
        Self {
            interner,
            mode,
            errors: Vec::new(),
            function_stack: Vec::new(),
            lexical_stack: vec![LexicalContext::default()],
            label_stack: Vec::new(),
            loop_depth: 0,
            breakable_depth: 0,
        }
    }

    fn check_module(&mut self, module: &Module) {
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
        self.loop_depth = 0;
        self.breakable_depth = 0;
        let result = f(self);
        self.function_stack.pop();
        self.lexical_stack.pop();
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
        self.label_stack.iter().rev().any(|label| label.name == name)
    }

    fn has_iteration_label(&self, name: Symbol) -> bool {
        self.label_stack
            .iter()
            .rev()
            .any(|label| label.name == name && label.is_iteration_target)
    }

    fn check_stmt(&mut self, stmt: &Statement) {
        match stmt {
            Statement::VariableDecl(decl) => self.check_var_decl(decl),
            Statement::FunctionDecl(func) => self.check_function_decl(func),
            Statement::ClassDecl(class) => self.check_class_decl(class),
            Statement::TypeAliasDecl(_) | Statement::ImportDecl(_) | Statement::Empty(_) => {}
            Statement::ExportDecl(export) => self.check_export_decl(export),
            Statement::Expression(expr) => self.check_expr(&expr.expression),
            Statement::If(if_stmt) => {
                self.check_expr(&if_stmt.condition);
                self.check_stmt(&if_stmt.then_branch);
                if let Some(else_branch) = &if_stmt.else_branch {
                    self.check_stmt(else_branch);
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
                self.push_loop(|this| this.check_stmt(&while_stmt.body));
            }
            Statement::DoWhile(do_while) => {
                self.push_loop(|this| {
                    this.check_stmt(&do_while.body);
                    this.check_expr(&do_while.condition);
                });
            }
            Statement::For(for_stmt) => {
                if let Some(init) = &for_stmt.init {
                    match init {
                        ForInit::VariableDecl(decl) => this_check_var_decl(self, decl),
                        ForInit::Expression(expr) => self.check_expr(expr),
                    }
                }
                if let Some(test) = &for_stmt.test {
                    self.check_expr(test);
                }
                if let Some(update) = &for_stmt.update {
                    self.check_expr(update);
                }
                self.push_loop(|this| this.check_stmt(&for_stmt.body));
            }
            Statement::ForOf(for_of) => {
                self.check_for_left(&for_of.left);
                self.check_expr(&for_of.right);
                self.push_loop(|this| this.check_stmt(&for_of.body));
            }
            Statement::ForIn(for_in) => {
                self.check_for_left(&for_in.left);
                self.check_expr(&for_in.right);
                self.push_loop(|this| this.check_stmt(&for_in.body));
            }
            Statement::Break(brk) => self.check_break(brk),
            Statement::Continue(cont) => self.check_continue(cont),
            Statement::Return(ret) => self.check_return(ret),
            Statement::Yield(yld) => self.check_yield(yld),
            Statement::Throw(thr) => self.check_expr(&thr.value),
            Statement::Try(try_stmt) => {
                self.check_block(&try_stmt.body);
                if let Some(catch) = &try_stmt.catch_clause {
                    if let Some(param) = &catch.param {
                        self.check_pattern(param);
                    }
                    self.check_block(&catch.body);
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
                    this.check_stmt(&labeled.body);
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
        for stmt in &block.statements {
            self.check_stmt(stmt);
        }
    }

    fn check_function_decl(&mut self, func: &FunctionDecl) {
        for param in &func.params {
            self.check_parameter(param);
        }
        self.push_function(
            func.is_async,
            func.is_generator,
            LexicalContext::default(),
            |this| this.check_block(&func.body),
        );
    }

    fn check_function_expr(&mut self, func: &FunctionExpression) {
        for param in &func.params {
            self.check_parameter(param);
        }
        let lexical = if func.is_method {
            self.current_lexical()
        } else {
            LexicalContext::default()
        };
        self.push_function(func.is_async, func.is_generator, lexical, |this| {
            this.check_block(&func.body);
        });
    }

    fn check_arrow_function(&mut self, arrow: &ArrowFunction) {
        for param in &arrow.params {
            self.check_parameter(param);
        }
        self.push_function(arrow.is_async, false, self.current_lexical(), |this| {
            match &arrow.body {
                ArrowBody::Expression(expr) => this.check_expr(expr),
                ArrowBody::Block(block) => this.check_block(block),
            }
        });
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
        for member in &class.members {
            match member {
                ClassMember::Field(field) => {
                    if let Some(initializer) = &field.initializer {
                        self.push_lexical(
                            LexicalContext {
                                super_property_allowed: has_super_class,
                                super_call_allowed: false,
                            },
                            |this| this.check_expr(initializer),
                        );
                    }
                }
                ClassMember::Method(method) => {
                    match method.kind {
                        MethodKind::Getter if !method.params.is_empty() => self.error(
                            "Getter must not declare parameters",
                            method.span,
                        ),
                        MethodKind::Setter if method.params.len() != 1 => self.error(
                            "Setter must declare exactly one parameter",
                            method.span,
                        ),
                        _ => {}
                    }
                    for param in &method.params {
                        self.check_parameter(param);
                    }
                    if let Some(body) = &method.body {
                        self.push_function(
                            method.is_async,
                            method.is_generator,
                            LexicalContext {
                                super_property_allowed: has_super_class,
                                super_call_allowed: false,
                            },
                            |this| this.check_block(body),
                        );
                    }
                }
                ClassMember::Constructor(ctor) => {
                    constructor_count += 1;
                    if constructor_count > 1 {
                        self.error("Class must not declare multiple constructors", ctor.span);
                    }
                    for param in &ctor.params {
                        self.check_parameter(param);
                    }
                    self.push_function(
                        false,
                        false,
                        LexicalContext {
                            super_property_allowed: has_super_class,
                            super_call_allowed: has_super_class,
                        },
                        |this| this.check_block(&ctor.body),
                    );
                }
                ClassMember::StaticBlock(block) => self.push_lexical(
                    LexicalContext {
                        super_property_allowed: has_super_class,
                        super_call_allowed: false,
                    },
                    |this| this.check_block(block),
                ),
            }
        }
    }

    fn check_var_decl(&mut self, decl: &VariableDecl) {
        self.check_pattern(&decl.pattern);
        if let Some(init) = &decl.initializer {
            self.check_expr(init);
        }
    }

    fn check_for_left(&mut self, left: &ForOfLeft) {
        match left {
            ForOfLeft::VariableDecl(decl) => self.check_var_decl(decl),
            ForOfLeft::Pattern(pattern) => self.check_pattern(pattern),
        }
    }

    fn check_parameter(&mut self, param: &Parameter) {
        self.check_pattern(&param.pattern);
        if let Some(default) = &param.default_value {
            self.check_expr(default);
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
        if self.current_function().is_none() {
            self.error("Return statement outside of function", ret.span);
        }
        if let Some(value) = &ret.value {
            self.check_expr(value);
        }
    }

    fn check_yield(&mut self, yld: &YieldStatement) {
        if !self.current_function().is_some_and(|ctx| ctx.is_generator) {
            self.error("Yield statement outside of generator function", yld.span);
        }
        if let Some(value) = &yld.value {
            self.check_expr(value);
        }
    }

    fn check_assignment_target(&mut self, expr: &Expression) {
        if !Self::is_valid_assignment_target(expr) {
            self.error("Invalid assignment target", *expr.span());
            return;
        }

        match expr {
            Expression::Parenthesized(paren) => self.check_assignment_target(&paren.expression),
            Expression::Array(array) => {
                for (index, elem) in array.elements.iter().flatten().enumerate() {
                    match elem {
                        ArrayElement::Expression(expr) => self.check_assignment_target(expr),
                        ArrayElement::Spread(expr) => {
                            if index + 1 != array.elements.len() {
                                self.error(
                                    "Rest element must be the last element in an assignment pattern",
                                    *expr.span(),
                                );
                            }
                            self.check_assignment_target(expr);
                        }
                    }
                }
            }
            Expression::Object(obj) => {
                for (index, prop) in obj.properties.iter().enumerate() {
                    match prop {
                        ObjectProperty::Property(prop) => self.check_assignment_target(&prop.value),
                        ObjectProperty::Spread(spread) => {
                            if index + 1 != obj.properties.len() {
                                self.error(
                                    "Rest property must be the last property in an assignment pattern",
                                    spread.span,
                                );
                            }
                            self.check_assignment_target(&spread.argument);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn is_valid_assignment_target(expr: &Expression) -> bool {
        match expr {
            Expression::Identifier(_)
            | Expression::Member(_)
            | Expression::Index(_) => true,
            Expression::Parenthesized(paren) => Self::is_valid_assignment_target(&paren.expression),
            Expression::Array(array) => array.elements.iter().flatten().all(|elem| match elem {
                ArrayElement::Expression(expr) | ArrayElement::Spread(expr) => {
                    Self::is_valid_assignment_target(expr)
                }
            }),
            Expression::Object(obj) => obj.properties.iter().all(|prop| match prop {
                ObjectProperty::Property(prop) => Self::is_valid_assignment_target(&prop.value),
                ObjectProperty::Spread(spread) => Self::is_valid_assignment_target(&spread.argument),
            }),
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
            Expression::Identifier(_)
            | Expression::IntLiteral(_)
            | Expression::FloatLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::This(_)
            | Expression::RegexLiteral(_) => {}
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
                if !self.current_function().is_some_and(|ctx| ctx.is_async) {
                    self.error(
                        "Await expression outside of async function",
                        await_expr.span,
                    );
                }
                self.check_expr(&await_expr.argument);
            }
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
    use super::check_early_errors;
    use crate::parser::checker::TypeSystemMode;
    use crate::parser::Parser;

    fn parse_module(source: &str) -> (crate::parser::ast::Module, crate::parser::Interner) {
        let parser = Parser::new(source).expect("should lex");
        parser.parse().expect("should parse")
    }

    #[test]
    fn test_return_outside_function_is_early_error() {
        let (module, interner) = parse_module("return 1;");
        let errors = check_early_errors(&module, &interner, TypeSystemMode::Ts)
            .expect_err("expected early error");
        assert!(errors[0].message.contains("Return statement outside of function"));
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
        let (module, interner) = parse_module("outer: while (true) { continue outer; break outer; }");
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
        let (module, interner) = parse_module("class Base {} class Child extends Base { method() { super(); } }");
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
        let (module, interner) = parse_module("class Example { get value(x: number) { return x; } }");
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
}

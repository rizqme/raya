//! Shared semantic profile and inspection types.
//!
//! This module centralizes source-kind-driven semantic decisions so parser,
//! checker, lowering, and runtime entrypoints can derive behavior from one
//! profile instead of scattering booleans across layers.

use crate::parser::ast::{
    self, Expression, FunctionDecl, MethodDecl, Pattern, Statement, VariableKind,
};
use crate::parser::checker::{CheckerPolicy, EarlyErrorOptions, TsTypeFlags, TypeSystemMode};
use crate::parser::{Interner, Symbol};
use rustc_hash::{FxHashMap, FxHashSet};
use std::path::Path;

/// Source language family inferred from file extension or explicit configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SourceKind {
    /// Standard JavaScript source.
    Js,
    /// TypeScript source with strict type checking.
    Ts,
    /// Raya source with coroutine-first extensions.
    #[default]
    Raya,
}

impl SourceKind {
    /// Infer the source kind from a path extension.
    pub fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("js" | "mjs" | "cjs" | "jsx") => Self::Js,
            Some("ts" | "mts" | "cts" | "tsx") => Self::Ts,
            _ => Self::Raya,
        }
    }
}

/// Runtime semantic base for the shared frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RuntimeSemanticsBase {
    /// ECMAScript object/call/descriptor semantics.
    #[default]
    EcmaScript,
}

/// Static typing discipline layered on top of the shared semantic core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TypingDiscipline {
    /// JS dynamic typing and fallback behavior.
    DynamicJs,
    /// Strict TypeScript policy.
    StrictTs,
    /// Raya's stricter-than-TS policy.
    #[default]
    RayaStrict,
}

/// Async/concurrency behavior profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ConcurrencySemantics {
    /// Spec-JS async/generator semantics only.
    StandardJsAsync,
    /// Coroutine-first lowering and suspension support.
    #[default]
    CoroutineFirst,
}

/// Lowering/optimization emphasis for a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum OptimizationProfile {
    /// Favor compatibility and minimal lowering assumptions.
    Compatibility,
    /// Favor coroutine-first lowering and optimization.
    #[default]
    OptimizedCoroutineFirst,
}

/// Shared semantic profile used across parse/check/lower stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SemanticProfile {
    /// Source kind for syntax and diagnostics.
    pub source_kind: SourceKind,
    /// Runtime semantic base.
    pub runtime: RuntimeSemanticsBase,
    /// Static typing discipline.
    pub typing: TypingDiscipline,
    /// Async/coroutine behavior.
    pub concurrency: ConcurrencySemantics,
    /// Lowering/optimization emphasis.
    pub optimization: OptimizationProfile,
    /// Whether method extraction follows JS unbound-receiver rules.
    pub js_this_binding_compat: bool,
    /// Whether unresolved members may fall back to runtime dynamic lookup.
    pub allow_unresolved_runtime_fallback: bool,
    /// Whether top-level completion values are observable.
    pub track_top_level_completion: bool,
    /// Whether top-level bindings publish to `globalThis`.
    pub emit_script_global_bindings: bool,
    /// Whether published globals are configurable.
    pub script_global_bindings_configurable: bool,
    /// Whether top-level `return` is legal.
    pub allow_top_level_return: bool,
    /// Whether `await` is legal outside `async` functions.
    pub allow_await_outside_async: bool,
    /// Whether TS syntax should be accepted.
    pub allow_typescript_syntax: bool,
    /// Whether Raya-only syntax/extensions should be accepted.
    pub allow_raya_syntax: bool,
}

impl Default for SemanticProfile {
    fn default() -> Self {
        Self::raya()
    }
}

/// Lowering-relevant semantic switches derived from a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LoweringSemantics {
    /// Whether method extraction follows JS unbound-receiver rules.
    pub js_this_binding_compat: bool,
    /// Whether unresolved members may fall back to runtime dynamic lookup.
    pub allow_unresolved_runtime_fallback: bool,
    /// Whether top-level completion values are observable.
    pub track_top_level_completion: bool,
    /// Whether top-level bindings publish to `globalThis`.
    pub emit_script_global_bindings: bool,
    /// Whether published globals are configurable.
    pub script_global_bindings_configurable: bool,
}

impl SemanticProfile {
    /// Standard JS compatibility profile.
    pub const fn js() -> Self {
        Self {
            source_kind: SourceKind::Js,
            runtime: RuntimeSemanticsBase::EcmaScript,
            typing: TypingDiscipline::DynamicJs,
            concurrency: ConcurrencySemantics::StandardJsAsync,
            optimization: OptimizationProfile::Compatibility,
            js_this_binding_compat: true,
            allow_unresolved_runtime_fallback: true,
            track_top_level_completion: true,
            emit_script_global_bindings: true,
            script_global_bindings_configurable: true,
            allow_top_level_return: false,
            allow_await_outside_async: false,
            allow_typescript_syntax: false,
            allow_raya_syntax: false,
        }
    }

    /// Strict TS profile layered on top of the JS semantic core.
    pub const fn ts_strict() -> Self {
        Self {
            source_kind: SourceKind::Ts,
            runtime: RuntimeSemanticsBase::EcmaScript,
            typing: TypingDiscipline::StrictTs,
            concurrency: ConcurrencySemantics::StandardJsAsync,
            optimization: OptimizationProfile::Compatibility,
            js_this_binding_compat: true,
            allow_unresolved_runtime_fallback: true,
            track_top_level_completion: true,
            emit_script_global_bindings: true,
            script_global_bindings_configurable: true,
            allow_top_level_return: false,
            allow_await_outside_async: false,
            allow_typescript_syntax: true,
            allow_raya_syntax: false,
        }
    }

    /// Raya profile sharing the JS core but enabling coroutine-first behavior.
    pub const fn raya() -> Self {
        Self {
            source_kind: SourceKind::Raya,
            runtime: RuntimeSemanticsBase::EcmaScript,
            typing: TypingDiscipline::RayaStrict,
            concurrency: ConcurrencySemantics::CoroutineFirst,
            optimization: OptimizationProfile::OptimizedCoroutineFirst,
            js_this_binding_compat: false,
            allow_unresolved_runtime_fallback: false,
            track_top_level_completion: false,
            emit_script_global_bindings: true,
            script_global_bindings_configurable: false,
            allow_top_level_return: true,
            allow_await_outside_async: true,
            allow_typescript_syntax: false,
            allow_raya_syntax: true,
        }
    }

    /// Derive the default profile for a source kind.
    pub const fn for_source_kind(source_kind: SourceKind) -> Self {
        match source_kind {
            SourceKind::Js => Self::js(),
            SourceKind::Ts => Self::ts_strict(),
            SourceKind::Raya => Self::raya(),
        }
    }

    /// Infer a default profile from a path extension.
    pub fn from_path(path: &Path) -> Self {
        Self::for_source_kind(SourceKind::from_path(path))
    }

    /// Parser/checker mode used for syntax-specific parsing.
    pub const fn type_system_mode(self) -> TypeSystemMode {
        match self.source_kind {
            SourceKind::Js => TypeSystemMode::Js,
            SourceKind::Ts => TypeSystemMode::Ts,
            SourceKind::Raya => TypeSystemMode::Raya,
        }
    }

    /// Effective checker policy for the profile.
    pub fn checker_policy(self, ts_flags: Option<TsTypeFlags>) -> CheckerPolicy {
        match self.typing {
            TypingDiscipline::DynamicJs => CheckerPolicy::for_mode(TypeSystemMode::Js),
            TypingDiscipline::StrictTs => {
                CheckerPolicy::for_ts(ts_flags.unwrap_or_else(TsTypeFlags::default))
            }
            TypingDiscipline::RayaStrict => CheckerPolicy::for_mode(TypeSystemMode::Raya),
        }
    }

    /// Early-error options derived from the profile.
    pub fn early_error_options(self) -> EarlyErrorOptions {
        let mut options = EarlyErrorOptions::for_mode(self.type_system_mode());
        options.allow_top_level_return = self.allow_top_level_return;
        options.allow_await_outside_async = self.allow_await_outside_async;
        options
    }

    /// Lowering switches derived from the profile.
    pub const fn lowering_semantics(self) -> LoweringSemantics {
        LoweringSemantics {
            js_this_binding_compat: self.js_this_binding_compat,
            allow_unresolved_runtime_fallback: self.allow_unresolved_runtime_fallback,
            track_top_level_completion: self.track_top_level_completion,
            emit_script_global_bindings: self.emit_script_global_bindings,
            script_global_bindings_configurable: self.script_global_bindings_configurable,
        }
    }
}

/// Callable semantic kind recorded in the semantic HIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallableKind {
    SyncFunction,
    AsyncFunction,
    GeneratorFunction,
    AsyncGeneratorFunction,
    SyncMethod,
    AsyncMethod,
    GeneratorMethod,
    AsyncGeneratorMethod,
    Constructor,
}

/// Binding semantic kind recorded in the semantic HIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindingKind {
    Lexical,
    Var,
    Function,
    Parameter,
    Class,
}

/// Suspension semantic kind recorded in the semantic HIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SuspensionKind {
    Await,
    Yield,
    YieldStar,
}

/// Semantic callable summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticCallable {
    pub name: Option<String>,
    pub kind: CallableKind,
    pub span_start: usize,
}

/// Semantic binding summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticBinding {
    pub name: String,
    pub kind: BindingKind,
    pub top_level: bool,
}

/// Suspension point summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuspensionPoint {
    pub kind: SuspensionKind,
    pub span_start: usize,
}

/// Inspectable semantic HIR summary for a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticHirModule {
    pub profile: SemanticProfile,
    pub callables: Vec<SemanticCallable>,
    pub bindings: Vec<SemanticBinding>,
    pub suspension_points: Vec<SuspensionPoint>,
    pub uses_direct_eval: bool,
}

/// Top-level callable declaration tracked for lowering decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticTopLevelCallable {
    pub name: Symbol,
    pub kind: CallableKind,
    pub span_start: usize,
}

/// Semantic lowering plan derived from a module before IR lowering starts.
#[derive(Debug, Clone)]
pub struct SemanticLoweringPlan {
    pub hir: SemanticHirModule,
    callable_kinds_by_span: FxHashMap<usize, CallableKind>,
    top_level_callables: Vec<SemanticTopLevelCallable>,
    top_level_vars: FxHashSet<Symbol>,
    top_level_lexicals: FxHashSet<Symbol>,
    top_level_const_lexicals: FxHashSet<Symbol>,
    top_level_classes: FxHashSet<Symbol>,
}

impl SemanticLoweringPlan {
    pub fn empty(profile: SemanticProfile) -> Self {
        Self {
            hir: SemanticHirModule {
                profile,
                callables: Vec::new(),
                bindings: Vec::new(),
                suspension_points: Vec::new(),
                uses_direct_eval: false,
            },
            callable_kinds_by_span: FxHashMap::default(),
            top_level_callables: Vec::new(),
            top_level_vars: FxHashSet::default(),
            top_level_lexicals: FxHashSet::default(),
            top_level_const_lexicals: FxHashSet::default(),
            top_level_classes: FxHashSet::default(),
        }
    }

    pub fn profile(&self) -> SemanticProfile {
        self.hir.profile
    }

    pub fn lowering_semantics(&self) -> LoweringSemantics {
        self.hir.profile.lowering_semantics()
    }

    pub fn callable_kind_at_span(&self, span_start: usize) -> Option<CallableKind> {
        self.callable_kinds_by_span.get(&span_start).copied()
    }

    pub fn top_level_callables(&self) -> &[SemanticTopLevelCallable] {
        &self.top_level_callables
    }

    pub fn is_top_level_var(&self, symbol: Symbol) -> bool {
        self.top_level_vars.contains(&symbol)
    }

    pub fn is_top_level_lexical(&self, symbol: Symbol) -> bool {
        self.top_level_lexicals.contains(&symbol)
    }

    pub fn is_top_level_const_lexical(&self, symbol: Symbol) -> bool {
        self.top_level_const_lexicals.contains(&symbol)
    }

    pub fn is_top_level_class(&self, symbol: Symbol) -> bool {
        self.top_level_classes.contains(&symbol)
    }
}

/// Build a semantic HIR summary from an AST module.
pub fn build_semantic_hir(
    module: &ast::Module,
    interner: &Interner,
    profile: SemanticProfile,
) -> SemanticHirModule {
    build_semantic_lowering_plan(module, interner, profile).hir
}

/// Build a lowering-oriented semantic plan from an AST module.
pub fn build_semantic_lowering_plan(
    module: &ast::Module,
    interner: &Interner,
    profile: SemanticProfile,
) -> SemanticLoweringPlan {
    let mut builder = SemanticHirBuilder {
        interner,
        callables: Vec::new(),
        bindings: Vec::new(),
        suspension_points: Vec::new(),
        uses_direct_eval: false,
        function_depth: 0,
        top_level_callables: Vec::new(),
        top_level_vars: FxHashSet::default(),
        top_level_lexicals: FxHashSet::default(),
        top_level_const_lexicals: FxHashSet::default(),
        top_level_classes: FxHashSet::default(),
    };
    for stmt in &module.statements {
        builder.visit_stmt(stmt);
    }
    let hir = SemanticHirModule {
        profile,
        callables: builder
            .callables
            .iter()
            .map(|callable| SemanticCallable {
                name: callable.name.clone(),
                kind: callable.kind,
                span_start: callable.span_start,
            })
            .collect(),
        bindings: builder
            .bindings
            .iter()
            .map(|binding| SemanticBinding {
                name: interner.resolve(binding.name).to_string(),
                kind: binding.kind,
                top_level: binding.top_level,
            })
            .collect(),
        suspension_points: builder.suspension_points,
        uses_direct_eval: builder.uses_direct_eval,
    };
    let callable_kinds_by_span = builder
        .callables
        .iter()
        .map(|callable| (callable.span_start, callable.kind))
        .collect();
    SemanticLoweringPlan {
        hir,
        callable_kinds_by_span,
        top_level_callables: builder.top_level_callables,
        top_level_vars: builder.top_level_vars,
        top_level_lexicals: builder.top_level_lexicals,
        top_level_const_lexicals: builder.top_level_const_lexicals,
        top_level_classes: builder.top_level_classes,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SemanticCallableInfo {
    name: Option<String>,
    kind: CallableKind,
    span_start: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SemanticBindingInfo {
    name: Symbol,
    kind: BindingKind,
    top_level: bool,
}

struct SemanticHirBuilder<'a> {
    interner: &'a Interner,
    callables: Vec<SemanticCallableInfo>,
    bindings: Vec<SemanticBindingInfo>,
    suspension_points: Vec<SuspensionPoint>,
    uses_direct_eval: bool,
    function_depth: usize,
    top_level_callables: Vec<SemanticTopLevelCallable>,
    top_level_vars: FxHashSet<Symbol>,
    top_level_lexicals: FxHashSet<Symbol>,
    top_level_const_lexicals: FxHashSet<Symbol>,
    top_level_classes: FxHashSet<Symbol>,
}

impl<'a> SemanticHirBuilder<'a> {
    fn symbol(&self, symbol: Symbol) -> String {
        self.interner.resolve(symbol).to_string()
    }

    fn identifier(&self, ident: &ast::Identifier) -> String {
        self.symbol(ident.name)
    }

    fn property_key_name(&self, key: &ast::PropertyKey) -> Option<String> {
        match key {
            ast::PropertyKey::Identifier(id) => Some(self.identifier(id)),
            ast::PropertyKey::StringLiteral(lit) => Some(self.symbol(lit.value)),
            ast::PropertyKey::IntLiteral(lit) => Some(lit.value.to_string()),
            ast::PropertyKey::Computed(_) => None,
        }
    }

    fn record_binding(&mut self, symbol: Symbol, kind: BindingKind) {
        self.bindings.push(SemanticBindingInfo {
            name: symbol,
            kind,
            top_level: self.function_depth == 0,
        });
    }

    fn collect_pattern_symbols(pattern: &Pattern, out: &mut Vec<Symbol>) {
        match pattern {
            Pattern::Identifier(id) => out.push(id.name),
            Pattern::Array(arr) => {
                for elem in arr.elements.iter().flatten() {
                    Self::collect_pattern_symbols(&elem.pattern, out);
                }
                if let Some(rest) = &arr.rest {
                    Self::collect_pattern_symbols(rest, out);
                }
            }
            Pattern::Object(obj) => {
                for prop in &obj.properties {
                    Self::collect_pattern_symbols(&prop.value, out);
                }
                if let Some(rest) = &obj.rest {
                    out.push(rest.name);
                }
            }
            Pattern::Rest(rest) => Self::collect_pattern_symbols(&rest.argument, out),
        }
    }

    fn record_pattern(&mut self, pattern: &Pattern, kind: BindingKind) {
        match pattern {
            Pattern::Identifier(id) => self.record_binding(id.name, kind),
            Pattern::Array(arr) => {
                for elem in arr.elements.iter().flatten() {
                    self.record_pattern(&elem.pattern, kind);
                    if let Some(default) = &elem.default {
                        self.visit_expr(default);
                    }
                }
                if let Some(rest) = &arr.rest {
                    self.record_pattern(rest, kind);
                }
            }
            Pattern::Object(obj) => {
                for prop in &obj.properties {
                    self.record_pattern(&prop.value, kind);
                    if let Some(default) = &prop.default {
                        self.visit_expr(default);
                    }
                }
                if let Some(rest) = &obj.rest {
                    self.record_binding(rest.name, kind);
                }
            }
            Pattern::Rest(rest) => self.record_pattern(&rest.argument, kind),
        }
    }

    fn visit_function_like(
        &mut self,
        name: Option<String>,
        is_async: bool,
        is_generator: bool,
        params: &[ast::Parameter],
        body: Option<&ast::BlockStatement>,
        span_start: usize,
        callable_kind: Option<CallableKind>,
    ) {
        let kind = callable_kind.unwrap_or(match (is_async, is_generator) {
            (false, false) => CallableKind::SyncFunction,
            (true, false) => CallableKind::AsyncFunction,
            (false, true) => CallableKind::GeneratorFunction,
            (true, true) => CallableKind::AsyncGeneratorFunction,
        });
        self.callables.push(SemanticCallableInfo {
            name,
            kind,
            span_start,
        });
        self.function_depth += 1;
        for param in params {
            self.record_pattern(&param.pattern, BindingKind::Parameter);
            if let Some(default) = &param.default_value {
                self.visit_expr(default);
            }
        }
        if let Some(body) = body {
            for stmt in &body.statements {
                self.visit_stmt(stmt);
            }
        }
        self.function_depth = self.function_depth.saturating_sub(1);
    }

    fn visit_method(&mut self, method: &MethodDecl) {
        let callable_kind = match (method.is_async, method.is_generator) {
            (false, false) => CallableKind::SyncMethod,
            (true, false) => CallableKind::AsyncMethod,
            (false, true) => CallableKind::GeneratorMethod,
            (true, true) => CallableKind::AsyncGeneratorMethod,
        };
        let name = self
            .property_key_name(&method.name)
            .or_else(|| Some("<computed>".to_string()));
        self.visit_function_like(
            name,
            method.is_async,
            method.is_generator,
            &method.params,
            method.body.as_ref(),
            method.span.start,
            Some(callable_kind),
        );
    }

    fn visit_stmt(&mut self, stmt: &Statement) {
        match stmt {
            Statement::FunctionDecl(FunctionDecl {
                name,
                params,
                body,
                is_async,
                is_generator,
                span,
                ..
            }) => {
                self.record_binding(name.name, BindingKind::Function);
                if self.function_depth == 0 {
                    let kind = match (*is_async, *is_generator) {
                        (false, false) => CallableKind::SyncFunction,
                        (true, false) => CallableKind::AsyncFunction,
                        (false, true) => CallableKind::GeneratorFunction,
                        (true, true) => CallableKind::AsyncGeneratorFunction,
                    };
                    self.top_level_callables.push(SemanticTopLevelCallable {
                        name: name.name,
                        kind,
                        span_start: span.start,
                    });
                }
                self.visit_function_like(
                    Some(self.identifier(name)),
                    *is_async,
                    *is_generator,
                    params,
                    Some(body),
                    span.start,
                    None,
                );
            }
            Statement::ClassDecl(class_decl) => {
                self.record_binding(class_decl.name.name, BindingKind::Class);
                if self.function_depth == 0 {
                    self.top_level_classes.insert(class_decl.name.name);
                    self.top_level_lexicals.insert(class_decl.name.name);
                }
                for member in &class_decl.members {
                    match member {
                        ast::ClassMember::Method(method) => self.visit_method(method),
                        ast::ClassMember::Constructor(ctor) => {
                            self.visit_function_like(
                                Some("constructor".to_string()),
                                false,
                                false,
                                &ctor.params,
                                Some(&ctor.body),
                                ctor.span.start,
                                Some(CallableKind::Constructor),
                            );
                        }
                        ast::ClassMember::Field(field) => {
                            if let Some(initializer) = &field.initializer {
                                self.visit_expr(initializer);
                            }
                        }
                        ast::ClassMember::StaticBlock(block) => {
                            for stmt in &block.statements {
                                self.visit_stmt(stmt);
                            }
                        }
                    }
                }
            }
            Statement::VariableDecl(var_decl) => {
                let kind = match var_decl.kind {
                    VariableKind::Var => BindingKind::Var,
                    VariableKind::Const | VariableKind::Let => BindingKind::Lexical,
                };
                if self.function_depth == 0 {
                    let mut names = Vec::new();
                    Self::collect_pattern_symbols(&var_decl.pattern, &mut names);
                    for name in names {
                        match var_decl.kind {
                            VariableKind::Var => {
                                self.top_level_vars.insert(name);
                            }
                            VariableKind::Const => {
                                self.top_level_lexicals.insert(name);
                                self.top_level_const_lexicals.insert(name);
                            }
                            VariableKind::Let => {
                                self.top_level_lexicals.insert(name);
                            }
                        }
                    }
                }
                self.record_pattern(&var_decl.pattern, kind);
                if let Some(init) = &var_decl.initializer {
                    self.visit_expr(init);
                }
            }
            Statement::Block(block) => {
                for stmt in &block.statements {
                    self.visit_stmt(stmt);
                }
            }
            Statement::If(if_stmt) => {
                self.visit_expr(&if_stmt.condition);
                self.visit_stmt(&if_stmt.then_branch);
                if let Some(else_branch) = &if_stmt.else_branch {
                    self.visit_stmt(else_branch);
                }
            }
            Statement::While(while_stmt) => {
                self.visit_expr(&while_stmt.condition);
                self.visit_stmt(&while_stmt.body);
            }
            Statement::For(for_stmt) => {
                if let Some(init) = &for_stmt.init {
                    match init {
                        ast::ForInit::Expression(expr) => self.visit_expr(expr),
                        ast::ForInit::VariableDecl(decl) => {
                            self.record_pattern(
                                &decl.pattern,
                                match decl.kind {
                                    VariableKind::Var => BindingKind::Var,
                                    VariableKind::Const | VariableKind::Let => BindingKind::Lexical,
                                },
                            );
                            if let Some(init) = &decl.initializer {
                                self.visit_expr(init);
                            }
                        }
                    }
                }
                if let Some(test) = &for_stmt.test {
                    self.visit_expr(test);
                }
                if let Some(update) = &for_stmt.update {
                    self.visit_expr(update);
                }
                self.visit_stmt(&for_stmt.body);
            }
            Statement::Expression(expr_stmt) => self.visit_expr(&expr_stmt.expression),
            Statement::Return(ret) => {
                if let Some(arg) = &ret.value {
                    self.visit_expr(arg);
                }
            }
            Statement::Throw(thr) => self.visit_expr(&thr.value),
            Statement::Try(try_stmt) => {
                for stmt in &try_stmt.body.statements {
                    self.visit_stmt(stmt);
                }
                if let Some(handler) = &try_stmt.catch_clause {
                    if let Some(param) = &handler.param {
                        self.record_pattern(param, BindingKind::Lexical);
                    }
                    for stmt in &handler.body.statements {
                        self.visit_stmt(stmt);
                    }
                }
                if let Some(finalizer) = &try_stmt.finally_clause {
                    for stmt in &finalizer.statements {
                        self.visit_stmt(stmt);
                    }
                }
            }
            Statement::Switch(switch_stmt) => {
                self.visit_expr(&switch_stmt.discriminant);
                for case in &switch_stmt.cases {
                    if let Some(test) = &case.test {
                        self.visit_expr(test);
                    }
                    for stmt in &case.consequent {
                        self.visit_stmt(stmt);
                    }
                }
            }
            Statement::ForIn(for_in) => {
                match &for_in.left {
                    ast::ForOfLeft::VariableDecl(decl) => self.record_pattern(
                        &decl.pattern,
                        match decl.kind {
                            VariableKind::Var => BindingKind::Var,
                            VariableKind::Const | VariableKind::Let => BindingKind::Lexical,
                        },
                    ),
                    ast::ForOfLeft::Pattern(pattern) => {
                        self.record_pattern(pattern, BindingKind::Lexical)
                    }
                }
                self.visit_expr(&for_in.right);
                self.visit_stmt(&for_in.body);
            }
            Statement::ForOf(for_of) => {
                match &for_of.left {
                    ast::ForOfLeft::VariableDecl(decl) => self.record_pattern(
                        &decl.pattern,
                        match decl.kind {
                            VariableKind::Var => BindingKind::Var,
                            VariableKind::Const | VariableKind::Let => BindingKind::Lexical,
                        },
                    ),
                    ast::ForOfLeft::Pattern(pattern) => {
                        self.record_pattern(pattern, BindingKind::Lexical)
                    }
                }
                self.visit_expr(&for_of.right);
                self.visit_stmt(&for_of.body);
            }
            Statement::ExportDecl(export) => match export {
                ast::ExportDecl::Default { expression, .. } => self.visit_expr(expression),
                ast::ExportDecl::Declaration(decl) => self.visit_stmt(decl),
                _ => {}
            },
            Statement::DoWhile(do_while) => {
                self.visit_stmt(&do_while.body);
                self.visit_expr(&do_while.condition);
            }
            Statement::Labeled(label) => self.visit_stmt(&label.body),
            Statement::Yield(yld) => {
                self.suspension_points.push(SuspensionPoint {
                    kind: if yld.is_delegate {
                        SuspensionKind::YieldStar
                    } else {
                        SuspensionKind::Yield
                    },
                    span_start: yld.span.start,
                });
                if let Some(argument) = &yld.value {
                    self.visit_expr(argument);
                }
            }
            Statement::Empty(_)
            | Statement::Break(_)
            | Statement::Continue(_)
            | Statement::Debugger(_)
            | Statement::ImportDecl(_)
            | Statement::TypeAliasDecl(_)
            | Statement::With(_) => {}
        }
    }

    fn visit_expr(&mut self, expr: &Expression) {
        match expr {
            Expression::Call(call) => {
                if let Expression::Identifier(ident) = &*call.callee {
                    if self.interner.resolve(ident.name) == "eval" {
                        self.uses_direct_eval = true;
                    }
                }
                self.visit_expr(&call.callee);
                for arg in &call.arguments {
                    self.visit_expr(arg.expression());
                }
            }
            Expression::AsyncCall(call) => {
                self.visit_expr(&call.callee);
                for arg in &call.arguments {
                    self.visit_expr(arg.expression());
                }
            }
            Expression::Await(await_expr) => {
                self.suspension_points.push(SuspensionPoint {
                    kind: SuspensionKind::Await,
                    span_start: await_expr.span.start,
                });
                self.visit_expr(&await_expr.argument);
            }
            Expression::Yield(yield_expr) => {
                self.suspension_points.push(SuspensionPoint {
                    kind: if yield_expr.is_delegate {
                        SuspensionKind::YieldStar
                    } else {
                        SuspensionKind::Yield
                    },
                    span_start: yield_expr.span.start,
                });
                if let Some(argument) = &yield_expr.value {
                    self.visit_expr(argument);
                }
            }
            Expression::Function(func) => {
                self.visit_function_like(
                    func.name.as_ref().map(|ident| self.identifier(ident)),
                    func.is_async,
                    func.is_generator,
                    &func.params,
                    Some(&func.body),
                    func.span.start,
                    None,
                );
            }
            Expression::Arrow(arrow) => {
                self.callables.push(SemanticCallableInfo {
                    name: None,
                    kind: if arrow.is_async {
                        CallableKind::AsyncFunction
                    } else {
                        CallableKind::SyncFunction
                    },
                    span_start: arrow.span.start,
                });
                self.function_depth += 1;
                for param in &arrow.params {
                    self.record_pattern(&param.pattern, BindingKind::Parameter);
                    if let Some(default) = &param.default_value {
                        self.visit_expr(default);
                    }
                }
                match &arrow.body {
                    ast::ArrowBody::Expression(expr) => self.visit_expr(expr),
                    ast::ArrowBody::Block(block) => {
                        for stmt in &block.statements {
                            self.visit_stmt(stmt);
                        }
                    }
                }
                self.function_depth = self.function_depth.saturating_sub(1);
            }
            Expression::Member(member) => {
                self.visit_expr(&member.object);
            }
            Expression::Index(index) => {
                self.visit_expr(&index.object);
                self.visit_expr(&index.index);
            }
            Expression::New(new_expr) => {
                self.visit_expr(&new_expr.callee);
                for arg in &new_expr.arguments {
                    self.visit_expr(arg.expression());
                }
            }
            Expression::Assignment(assign) => {
                self.visit_expr(&assign.left);
                self.visit_expr(&assign.right);
            }
            Expression::Binary(binary) => {
                self.visit_expr(&binary.left);
                self.visit_expr(&binary.right);
            }
            Expression::Logical(logical) => {
                self.visit_expr(&logical.left);
                self.visit_expr(&logical.right);
            }
            Expression::Unary(unary) => self.visit_expr(&unary.operand),
            Expression::Conditional(cond) => {
                self.visit_expr(&cond.test);
                self.visit_expr(&cond.consequent);
                self.visit_expr(&cond.alternate);
            }
            Expression::Array(arr) => {
                for elem in &arr.elements {
                    match elem {
                        Some(ast::ArrayElement::Expression(expr))
                        | Some(ast::ArrayElement::Spread(expr)) => self.visit_expr(expr),
                        None => {}
                    }
                }
            }
            Expression::Object(obj) => {
                for prop in &obj.properties {
                    match prop {
                        ast::ObjectProperty::Property(prop) => self.visit_expr(&prop.value),
                        ast::ObjectProperty::Spread(spread) => self.visit_expr(&spread.argument),
                    }
                }
            }
            Expression::Parenthesized(paren) => self.visit_expr(&paren.expression),
            Expression::TaggedTemplate(tagged) => {
                self.visit_expr(&tagged.tag);
                for part in &tagged.template.parts {
                    if let ast::TemplatePart::Expression(expr) = part {
                        self.visit_expr(expr);
                    }
                }
            }
            Expression::TemplateLiteral(template) => {
                for part in &template.parts {
                    if let ast::TemplatePart::Expression(expr) = part {
                        self.visit_expr(expr);
                    }
                }
            }
            Expression::TypeCast(type_cast) => self.visit_expr(&type_cast.object),
            Expression::JsxElement(elem) => {
                for attr in &elem.opening.attributes {
                    match attr {
                        ast::JsxAttribute::Attribute { value, .. } => {
                            if let Some(value) = value {
                                match value {
                                    ast::JsxAttributeValue::Expression(expr) => {
                                        self.visit_expr(expr)
                                    }
                                    ast::JsxAttributeValue::JsxElement(el) => {
                                        self.visit_expr(&Expression::JsxElement(*el.clone()))
                                    }
                                    ast::JsxAttributeValue::JsxFragment(fragment) => {
                                        self.visit_expr(&Expression::JsxFragment(*fragment.clone()))
                                    }
                                    ast::JsxAttributeValue::StringLiteral(_) => {}
                                }
                            }
                        }
                        ast::JsxAttribute::Spread { argument, .. } => self.visit_expr(argument),
                    }
                }
                for child in &elem.children {
                    self.visit_jsx_child(child);
                }
            }
            Expression::JsxFragment(fragment) => {
                for child in &fragment.children {
                    self.visit_jsx_child(child);
                }
            }
            Expression::Identifier(_)
            | Expression::IntLiteral(_)
            | Expression::FloatLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::RegexLiteral(_)
            | Expression::This(_)
            | Expression::NewTarget(_)
            | Expression::Super(_)
            | Expression::Typeof(_)
            | Expression::InstanceOf(_)
            | Expression::In(_)
            | Expression::DynamicImport(_) => {}
        }
    }

    fn visit_jsx_child(&mut self, child: &ast::JsxChild) {
        match child {
            ast::JsxChild::Element(elem) => self.visit_expr(&Expression::JsxElement(elem.clone())),
            ast::JsxChild::Fragment(fragment) => {
                self.visit_expr(&Expression::JsxFragment(fragment.clone()))
            }
            ast::JsxChild::Expression(expr) => {
                if let Some(expr) = &expr.expression {
                    self.visit_expr(expr);
                }
            }
            ast::JsxChild::Text(_) => {}
        }
    }
}

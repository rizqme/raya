//! Shared semantic profile and inspection types.
//!
//! This module centralizes source-kind-driven semantic decisions so parser,
//! checker, lowering, and runtime entrypoints can derive behavior from one
//! profile instead of scattering booleans across layers.

use crate::parser::ast::{
    self, Expression, FunctionDecl, MethodDecl, Pattern, Statement, UnaryOperator, VariableKind,
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
            script_global_bindings_configurable: false,
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
            script_global_bindings_configurable: false,
            allow_top_level_return: false,
            allow_await_outside_async: false,
            allow_typescript_syntax: true,
            allow_raya_syntax: false,
        }
    }

    /// Node-compat inline/profile default: JS runtime semantics with dynamic typing,
    /// while still accepting TS syntax commonly used in embedded snippets/tests.
    pub const fn node_compat() -> Self {
        Self {
            source_kind: SourceKind::Ts,
            runtime: RuntimeSemanticsBase::EcmaScript,
            typing: TypingDiscipline::DynamicJs,
            concurrency: ConcurrencySemantics::StandardJsAsync,
            optimization: OptimizationProfile::Compatibility,
            js_this_binding_compat: true,
            allow_unresolved_runtime_fallback: true,
            track_top_level_completion: true,
            emit_script_global_bindings: true,
            script_global_bindings_configurable: false,
            allow_top_level_return: false,
            allow_await_outside_async: true,
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
            emit_script_global_bindings: false,
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

    /// Parser mode used for syntax-specific parsing.
    pub const fn parser_mode(self) -> TypeSystemMode {
        match self.source_kind {
            SourceKind::Js => TypeSystemMode::Js,
            SourceKind::Ts => TypeSystemMode::Ts,
            SourceKind::Raya => TypeSystemMode::Raya,
        }
    }

    /// Binder/checker mode used for semantic analysis.
    pub const fn checker_mode(self) -> TypeSystemMode {
        match self.typing {
            TypingDiscipline::DynamicJs => TypeSystemMode::Js,
            TypingDiscipline::StrictTs => TypeSystemMode::Ts,
            TypingDiscipline::RayaStrict => TypeSystemMode::Raya,
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
        let mut options = EarlyErrorOptions::for_mode(self.parser_mode());
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

    /// Whether async callables in this profile should use ECMAScript Promise-style
    /// eager-start/runtime settlement semantics instead of scheduler-first coroutines.
    pub const fn uses_js_async_runtime_semantics(self) -> bool {
        matches!(self.concurrency, ConcurrencySemantics::StandardJsAsync)
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

/// Shared environment record shape carried through semantic planning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvRecordKind {
    Declarative,
    ObjectWith,
    Global,
    DirectEval,
}

/// Lightweight semantic handle for an environment record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnvHandle {
    pub kind: EnvRecordKind,
}

/// Reference expression kind recorded before lowering.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReferenceExprKind {
    Identifier,
    PropertyNamed,
    PropertyComputed,
    SuperNamed,
    SuperComputed,
}

/// Semantic reference expression summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticReferenceExpr {
    pub span_start: usize,
    pub kind: ReferenceExprKind,
    pub name: Option<String>,
}

/// Resolved plain-identifier classification used by lowering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResolvedIdentifierKind {
    LocalBinding,
    CaptureBinding,
    ScriptGlobalBinding,
    RuntimeEnvLookup,
    BuiltinGlobal,
}

/// Semantic identifier-resolution summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticResolvedIdentifier {
    pub span_start: usize,
    pub name: String,
    pub kind: ResolvedIdentifierKind,
    pub binding_kind: Option<BindingKind>,
    pub top_level: bool,
    pub in_tdz: bool,
}

/// Binding operation kind recorded before lowering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindingOpKind {
    CreateMutable,
    CreateImmutable,
    Initialize,
    Assign,
    Delete,
    HasBinding,
}

/// Semantic binding operation summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticBindingOp {
    pub span_start: usize,
    pub kind: BindingOpKind,
    pub name: Option<String>,
    pub reference_span_start: Option<usize>,
}

/// Prefix/postfix update operation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UpdateOpKind {
    PrefixIncrement,
    PrefixDecrement,
    PostfixIncrement,
    PostfixDecrement,
}

/// Semantic update operation summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticUpdateOp {
    pub span_start: usize,
    pub kind: UpdateOpKind,
    pub reference_span_start: usize,
}

/// Call operation kind recorded before lowering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallOpKind {
    Ordinary,
    Method,
    Constructor,
    DirectEval,
    IndirectEval,
}

/// Semantic call operation summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticCallOp {
    pub span_start: usize,
    pub kind: CallOpKind,
    pub callee_span_start: usize,
}

/// Semantic function behavior summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionSemantics {
    pub span_start: usize,
    pub kind: CallableKind,
    pub uses_js_this: bool,
}

/// Ordered destructuring plan summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DestructuringPlan {
    pub span_start: usize,
    pub binding_names: Vec<String>,
    pub has_computed_keys: bool,
    pub has_defaults: bool,
    pub step_count: usize,
}

/// Per-loop lexical scope plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopScopePlan {
    pub span_start: usize,
    pub creates_per_iteration_env: bool,
    pub binding_names: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScopeSnapshotBinding {
    pub symbol: Symbol,
    pub kind: BindingKind,
    pub top_level: bool,
    pub runtime_env: bool,
    pub in_tdz: bool,
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
    pub function_semantics: Vec<FunctionSemantics>,
    pub bindings: Vec<SemanticBinding>,
    pub references: Vec<SemanticReferenceExpr>,
    pub resolved_identifiers: Vec<SemanticResolvedIdentifier>,
    pub binding_ops: Vec<SemanticBindingOp>,
    pub update_ops: Vec<SemanticUpdateOp>,
    pub call_ops: Vec<SemanticCallOp>,
    pub destructuring_plans: Vec<DestructuringPlan>,
    pub loop_scopes: Vec<LoopScopePlan>,
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
    references_by_span: FxHashMap<usize, SemanticReferenceExpr>,
    resolved_identifiers_by_span: FxHashMap<usize, SemanticResolvedIdentifier>,
    binding_ops_by_span: FxHashMap<usize, SemanticBindingOp>,
    update_ops_by_span: FxHashMap<usize, SemanticUpdateOp>,
    call_ops_by_span: FxHashMap<usize, SemanticCallOp>,
    destructuring_by_span: FxHashMap<usize, DestructuringPlan>,
    loop_scopes_by_span: FxHashMap<usize, LoopScopePlan>,
    scope_snapshots_by_span: FxHashMap<usize, Vec<ScopeSnapshotBinding>>,
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
                function_semantics: Vec::new(),
                bindings: Vec::new(),
                references: Vec::new(),
                resolved_identifiers: Vec::new(),
                binding_ops: Vec::new(),
                update_ops: Vec::new(),
                call_ops: Vec::new(),
                destructuring_plans: Vec::new(),
                loop_scopes: Vec::new(),
                suspension_points: Vec::new(),
                uses_direct_eval: false,
            },
            callable_kinds_by_span: FxHashMap::default(),
            references_by_span: FxHashMap::default(),
            resolved_identifiers_by_span: FxHashMap::default(),
            binding_ops_by_span: FxHashMap::default(),
            update_ops_by_span: FxHashMap::default(),
            call_ops_by_span: FxHashMap::default(),
            destructuring_by_span: FxHashMap::default(),
            loop_scopes_by_span: FxHashMap::default(),
            scope_snapshots_by_span: FxHashMap::default(),
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

    pub fn uses_js_async_runtime_semantics(&self) -> bool {
        self.hir.profile.uses_js_async_runtime_semantics()
    }

    pub fn callable_kind_at_span(&self, span_start: usize) -> Option<CallableKind> {
        self.callable_kinds_by_span.get(&span_start).copied()
    }

    pub fn reference_at_span(&self, span_start: usize) -> Option<&SemanticReferenceExpr> {
        self.references_by_span.get(&span_start)
    }

    pub fn resolved_identifier_at_span(
        &self,
        span_start: usize,
    ) -> Option<&SemanticResolvedIdentifier> {
        self.resolved_identifiers_by_span.get(&span_start)
    }

    pub fn binding_op_at_span(&self, span_start: usize) -> Option<&SemanticBindingOp> {
        self.binding_ops_by_span.get(&span_start)
    }

    pub fn update_op_at_span(&self, span_start: usize) -> Option<&SemanticUpdateOp> {
        self.update_ops_by_span.get(&span_start)
    }

    pub fn call_op_at_span(&self, span_start: usize) -> Option<&SemanticCallOp> {
        self.call_ops_by_span.get(&span_start)
    }

    pub fn destructuring_plan_at_span(&self, span_start: usize) -> Option<&DestructuringPlan> {
        self.destructuring_by_span.get(&span_start)
    }

    pub fn loop_scope_plan_at_span(&self, span_start: usize) -> Option<&LoopScopePlan> {
        self.loop_scopes_by_span.get(&span_start)
    }

    pub(crate) fn scope_snapshot_at_span(
        &self,
        span_start: usize,
    ) -> Option<&[ScopeSnapshotBinding]> {
        self.scope_snapshots_by_span
            .get(&span_start)
            .map(|bindings| bindings.as_slice())
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
        function_semantics: Vec::new(),
        bindings: Vec::new(),
        references: Vec::new(),
        resolved_identifiers: Vec::new(),
        binding_ops: Vec::new(),
        update_ops: Vec::new(),
        call_ops: Vec::new(),
        destructuring_plans: Vec::new(),
        loop_scopes: Vec::new(),
        suspension_points: Vec::new(),
        uses_direct_eval: false,
        function_depth: 0,
        with_depth: 0,
        scopes: vec![ScopeFrame::new(ScopeFrameKind::Function)],
        tdz_scopes: vec![FxHashSet::default()],
        arguments_binding_depths: Vec::new(),
        top_level_callables: Vec::new(),
        top_level_vars: FxHashSet::default(),
        top_level_lexicals: FxHashSet::default(),
        top_level_const_lexicals: FxHashSet::default(),
        top_level_classes: FxHashSet::default(),
        scope_snapshots_by_span: FxHashMap::default(),
    };
    builder.predeclare_stmt_list(&module.statements);
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
        function_semantics: builder.function_semantics,
        bindings: builder
            .bindings
            .iter()
            .map(|binding| SemanticBinding {
                name: interner.resolve(binding.name).to_string(),
                kind: binding.kind,
                top_level: binding.top_level,
            })
            .collect(),
        references: builder.references,
        resolved_identifiers: builder.resolved_identifiers,
        binding_ops: builder.binding_ops,
        update_ops: builder.update_ops,
        call_ops: builder.call_ops,
        destructuring_plans: builder.destructuring_plans,
        loop_scopes: builder.loop_scopes,
        suspension_points: builder.suspension_points,
        uses_direct_eval: builder.uses_direct_eval,
    };
    let callable_kinds_by_span = builder
        .callables
        .iter()
        .map(|callable| (callable.span_start, callable.kind))
        .collect();
    let references_by_span = hir
        .references
        .iter()
        .cloned()
        .map(|reference| (reference.span_start, reference))
        .collect();
    let resolved_identifiers_by_span = hir
        .resolved_identifiers
        .iter()
        .cloned()
        .map(|resolved| (resolved.span_start, resolved))
        .collect();
    let binding_ops_by_span = hir
        .binding_ops
        .iter()
        .cloned()
        .map(|op| (op.span_start, op))
        .collect();
    let update_ops_by_span = hir
        .update_ops
        .iter()
        .cloned()
        .map(|op| (op.span_start, op))
        .collect();
    let call_ops_by_span = hir
        .call_ops
        .iter()
        .cloned()
        .map(|op| (op.span_start, op))
        .collect();
    let destructuring_by_span = hir
        .destructuring_plans
        .iter()
        .cloned()
        .map(|plan| (plan.span_start, plan))
        .collect();
    let loop_scopes_by_span = hir
        .loop_scopes
        .iter()
        .cloned()
        .map(|plan| (plan.span_start, plan))
        .collect();
    SemanticLoweringPlan {
        hir,
        callable_kinds_by_span,
        references_by_span,
        resolved_identifiers_by_span,
        binding_ops_by_span,
        update_ops_by_span,
        call_ops_by_span,
        destructuring_by_span,
        loop_scopes_by_span,
        scope_snapshots_by_span: builder.scope_snapshots_by_span,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScopeBindingInfo {
    kind: BindingKind,
    declared_function_depth: usize,
    top_level: bool,
    runtime_env: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScopeFrameKind {
    Function,
    Block,
}

#[derive(Debug, Clone)]
struct ScopeFrame {
    kind: ScopeFrameKind,
    bindings: FxHashMap<Symbol, ScopeBindingInfo>,
}

impl ScopeFrame {
    fn new(kind: ScopeFrameKind) -> Self {
        Self {
            kind,
            bindings: FxHashMap::default(),
        }
    }
}

struct SemanticHirBuilder<'a> {
    interner: &'a Interner,
    callables: Vec<SemanticCallableInfo>,
    function_semantics: Vec<FunctionSemantics>,
    bindings: Vec<SemanticBindingInfo>,
    references: Vec<SemanticReferenceExpr>,
    resolved_identifiers: Vec<SemanticResolvedIdentifier>,
    binding_ops: Vec<SemanticBindingOp>,
    update_ops: Vec<SemanticUpdateOp>,
    call_ops: Vec<SemanticCallOp>,
    destructuring_plans: Vec<DestructuringPlan>,
    loop_scopes: Vec<LoopScopePlan>,
    suspension_points: Vec<SuspensionPoint>,
    uses_direct_eval: bool,
    function_depth: usize,
    with_depth: usize,
    scopes: Vec<ScopeFrame>,
    tdz_scopes: Vec<FxHashSet<Symbol>>,
    arguments_binding_depths: Vec<usize>,
    top_level_callables: Vec<SemanticTopLevelCallable>,
    top_level_vars: FxHashSet<Symbol>,
    top_level_lexicals: FxHashSet<Symbol>,
    top_level_const_lexicals: FxHashSet<Symbol>,
    top_level_classes: FxHashSet<Symbol>,
    scope_snapshots_by_span: FxHashMap<usize, Vec<ScopeSnapshotBinding>>,
}

impl<'a> SemanticHirBuilder<'a> {
    fn push_function_scope(&mut self) {
        self.scopes.push(ScopeFrame::new(ScopeFrameKind::Function));
        self.tdz_scopes.push(FxHashSet::default());
    }

    fn push_block_scope(&mut self) {
        self.scopes.push(ScopeFrame::new(ScopeFrameKind::Block));
        self.tdz_scopes.push(FxHashSet::default());
    }

    fn pop_scope(&mut self) {
        let _ = self.scopes.pop();
        let _ = self.tdz_scopes.pop();
    }

    fn mark_binding_tdz(&mut self, symbol: Symbol) {
        if let Some(scope) = self.tdz_scopes.last_mut() {
            scope.insert(symbol);
        }
    }

    fn clear_binding_tdz(&mut self, symbol: Symbol) {
        if let Some(scope) = self.tdz_scopes.last_mut() {
            scope.remove(&symbol);
        }
    }

    fn clear_pattern_tdz(&mut self, pattern: &Pattern) {
        let mut names = Vec::new();
        Self::collect_pattern_symbols(pattern, &mut names);
        for symbol in names {
            self.clear_binding_tdz(symbol);
        }
    }

    fn binding_is_in_tdz(&self, symbol: Symbol) -> bool {
        for (scope, tdz_scope) in self.scopes.iter().rev().zip(self.tdz_scopes.iter().rev()) {
            if scope.bindings.contains_key(&symbol) {
                return tdz_scope.contains(&symbol);
            }
        }
        false
    }

    fn declare_binding_in_scope_with_runtime_env(
        &mut self,
        symbol: Symbol,
        kind: BindingKind,
        runtime_env: bool,
    ) {
        let info = ScopeBindingInfo {
            kind,
            declared_function_depth: self.function_depth,
            top_level: self.function_depth == 0,
            runtime_env,
        };
        match kind {
            BindingKind::Var | BindingKind::Function => {
                if let Some(scope) = self
                    .scopes
                    .iter_mut()
                    .rev()
                    .find(|scope| scope.kind == ScopeFrameKind::Function)
                {
                    scope.bindings.insert(symbol, info);
                }
            }
            BindingKind::Lexical | BindingKind::Parameter | BindingKind::Class => {
                if let Some(scope) = self.scopes.last_mut() {
                    scope.bindings.insert(symbol, info);
                }
            }
        }
    }

    fn declare_binding_in_scope(&mut self, symbol: Symbol, kind: BindingKind) {
        self.declare_binding_in_scope_with_runtime_env(symbol, kind, false);
    }

    fn predeclare_stmt_list(&mut self, statements: &[Statement]) {
        for stmt in statements {
            self.predeclare_stmt(stmt);
        }
    }

    fn predeclare_stmt(&mut self, stmt: &Statement) {
        match stmt {
            Statement::FunctionDecl(func) => {
                self.declare_binding_in_scope(func.name.name, BindingKind::Function);
            }
            Statement::ClassDecl(class_decl) => {
                self.declare_binding_in_scope(class_decl.name.name, BindingKind::Class);
                self.mark_binding_tdz(class_decl.name.name);
            }
            Statement::VariableDecl(var_decl) => {
                let mut names = Vec::new();
                Self::collect_pattern_symbols(&var_decl.pattern, &mut names);
                let kind = match var_decl.kind {
                    VariableKind::Var => BindingKind::Var,
                    VariableKind::Const | VariableKind::Let => BindingKind::Lexical,
                };
                for name in names {
                    self.declare_binding_in_scope(name, kind);
                    if matches!(kind, BindingKind::Lexical) {
                        self.mark_binding_tdz(name);
                    }
                }
            }
            _ => {}
        }
    }

    fn resolve_scope_binding(&self, symbol: Symbol) -> Option<ScopeBindingInfo> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.bindings.get(&symbol).copied())
    }

    fn record_resolved_identifier(&mut self, ident: &ast::Identifier) {
        let name = self.identifier(ident);
        let implicit_arguments_binding_depth = (name == "arguments")
            .then(|| self.arguments_binding_depths.last().copied())
            .flatten();
        let kind = if matches!(name.as_str(), "Infinity" | "NaN" | "undefined") {
            ResolvedIdentifierKind::BuiltinGlobal
        } else {
            let binding = self.resolve_scope_binding(ident.name);
            if self.with_depth > 0 || binding.is_some_and(|binding| binding.runtime_env) {
                ResolvedIdentifierKind::RuntimeEnvLookup
            } else if let Some(binding) = binding {
                if binding.top_level {
                    ResolvedIdentifierKind::ScriptGlobalBinding
                } else if binding.declared_function_depth == self.function_depth {
                    ResolvedIdentifierKind::LocalBinding
                } else {
                    ResolvedIdentifierKind::CaptureBinding
                }
            } else if let Some(arguments_depth) = implicit_arguments_binding_depth {
                if arguments_depth == self.function_depth {
                    ResolvedIdentifierKind::LocalBinding
                } else {
                    ResolvedIdentifierKind::CaptureBinding
                }
            } else {
                ResolvedIdentifierKind::RuntimeEnvLookup
            }
        };
        let binding = self.resolve_scope_binding(ident.name);
        self.resolved_identifiers.push(SemanticResolvedIdentifier {
            span_start: ident.span.start,
            name,
            kind,
            binding_kind: binding.map(|binding| binding.kind),
            top_level: binding.is_some_and(|binding| binding.top_level),
            in_tdz: self.binding_is_in_tdz(ident.name),
        });
    }

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

    fn record_binding_with_runtime_env(
        &mut self,
        symbol: Symbol,
        kind: BindingKind,
        runtime_env: bool,
    ) {
        self.bindings.push(SemanticBindingInfo {
            name: symbol,
            kind,
            top_level: self.function_depth == 0,
        });
        self.declare_binding_in_scope_with_runtime_env(symbol, kind, runtime_env);
    }

    fn record_binding(&mut self, symbol: Symbol, kind: BindingKind) {
        self.record_binding_with_runtime_env(symbol, kind, false);
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

    fn record_pattern_with_runtime_env(
        &mut self,
        pattern: &Pattern,
        kind: BindingKind,
        runtime_env: bool,
    ) {
        match pattern {
            Pattern::Identifier(id) => self.record_binding_with_runtime_env(id.name, kind, runtime_env),
            Pattern::Array(arr) => {
                self.record_destructuring_plan(pattern);
                for elem in arr.elements.iter().flatten() {
                    self.record_pattern_with_runtime_env(&elem.pattern, kind, runtime_env);
                    if let Some(default) = &elem.default {
                        self.visit_expr(default);
                    }
                }
                if let Some(rest) = &arr.rest {
                    self.record_pattern_with_runtime_env(rest, kind, runtime_env);
                }
            }
            Pattern::Object(obj) => {
                self.record_destructuring_plan(pattern);
                for prop in &obj.properties {
                    if let ast::PropertyKey::Computed(expr) = &prop.key {
                        self.visit_expr(expr);
                    }
                    self.record_pattern_with_runtime_env(&prop.value, kind, runtime_env);
                    if let Some(default) = &prop.default {
                        self.visit_expr(default);
                    }
                }
                if let Some(rest) = &obj.rest {
                    self.record_binding_with_runtime_env(rest.name, kind, runtime_env);
                }
            }
            Pattern::Rest(rest) => self.record_pattern_with_runtime_env(&rest.argument, kind, runtime_env),
        }
    }

    fn record_pattern(&mut self, pattern: &Pattern, kind: BindingKind) {
        self.record_pattern_with_runtime_env(pattern, kind, false);
    }

    fn collect_pattern_names_in_order(
        pattern: &Pattern,
        out: &mut Vec<String>,
        interner: &Interner,
    ) {
        match pattern {
            Pattern::Identifier(id) => out.push(interner.resolve(id.name).to_string()),
            Pattern::Array(arr) => {
                for elem in arr.elements.iter().flatten() {
                    Self::collect_pattern_names_in_order(&elem.pattern, out, interner);
                }
                if let Some(rest) = &arr.rest {
                    Self::collect_pattern_names_in_order(rest, out, interner);
                }
            }
            Pattern::Object(obj) => {
                for prop in &obj.properties {
                    Self::collect_pattern_names_in_order(&prop.value, out, interner);
                }
                if let Some(rest) = &obj.rest {
                    out.push(interner.resolve(rest.name).to_string());
                }
            }
            Pattern::Rest(rest) => {
                Self::collect_pattern_names_in_order(&rest.argument, out, interner)
            }
        }
    }

    fn record_scope_snapshot(&mut self, span_start: usize) {
        let mut seen = FxHashSet::default();
        let mut bindings = Vec::new();

        for scope in self.scopes.iter().rev() {
            for (&symbol, &info) in &scope.bindings {
                if seen.insert(symbol) {
                    bindings.push(ScopeSnapshotBinding {
                        symbol,
                        kind: info.kind,
                        top_level: info.top_level,
                        runtime_env: info.runtime_env || self.with_depth > 0,
                        in_tdz: self.binding_is_in_tdz(symbol),
                    });
                }
            }
        }

        bindings.sort_by_key(|binding| self.interner.resolve(binding.symbol).to_string());
        self.scope_snapshots_by_span.insert(span_start, bindings);
    }

    fn pattern_has_computed_keys(pattern: &Pattern) -> bool {
        match pattern {
            Pattern::Identifier(_) => false,
            Pattern::Array(arr) => arr
                .elements
                .iter()
                .flatten()
                .any(|elem| Self::pattern_has_computed_keys(&elem.pattern))
                || arr
                    .rest
                    .as_deref()
                    .is_some_and(Self::pattern_has_computed_keys),
            Pattern::Object(obj) => obj.properties.iter().any(|prop| {
                matches!(prop.key, ast::PropertyKey::Computed(_))
                    || Self::pattern_has_computed_keys(&prop.value)
            }),
            Pattern::Rest(rest) => Self::pattern_has_computed_keys(&rest.argument),
        }
    }

    fn pattern_has_defaults(pattern: &Pattern) -> bool {
        match pattern {
            Pattern::Identifier(_) => false,
            Pattern::Array(arr) => arr
                .elements
                .iter()
                .flatten()
                .any(|elem| elem.default.is_some() || Self::pattern_has_defaults(&elem.pattern)),
            Pattern::Object(obj) => obj
                .properties
                .iter()
                .any(|prop| prop.default.is_some() || Self::pattern_has_defaults(&prop.value)),
            Pattern::Rest(rest) => Self::pattern_has_defaults(&rest.argument),
        }
    }

    fn pattern_step_count(pattern: &Pattern) -> usize {
        match pattern {
            Pattern::Identifier(_) => 1,
            Pattern::Array(arr) => {
                arr.elements.iter().flatten().count()
                    + usize::from(arr.rest.is_some())
                    + arr
                        .elements
                        .iter()
                        .flatten()
                        .map(|elem| Self::pattern_step_count(&elem.pattern).saturating_sub(1))
                        .sum::<usize>()
            }
            Pattern::Object(obj) => {
                obj.properties.len()
                    + usize::from(obj.rest.is_some())
                    + obj
                        .properties
                        .iter()
                        .map(|prop| Self::pattern_step_count(&prop.value).saturating_sub(1))
                        .sum::<usize>()
            }
            Pattern::Rest(rest) => Self::pattern_step_count(&rest.argument),
        }
    }

    fn record_destructuring_plan(&mut self, pattern: &Pattern) {
        if !matches!(pattern, Pattern::Array(_) | Pattern::Object(_)) {
            return;
        }
        let mut binding_names = Vec::new();
        Self::collect_pattern_names_in_order(pattern, &mut binding_names, self.interner);
        self.destructuring_plans.push(DestructuringPlan {
            span_start: pattern.span().start,
            binding_names,
            has_computed_keys: Self::pattern_has_computed_keys(pattern),
            has_defaults: Self::pattern_has_defaults(pattern),
            step_count: Self::pattern_step_count(pattern),
        });
    }

    fn record_reference_expr(&mut self, expr: &Expression) {
        let reference = match expr {
            Expression::Identifier(ident) => SemanticReferenceExpr {
                span_start: ident.span.start,
                kind: ReferenceExprKind::Identifier,
                name: Some(self.identifier(ident)),
            },
            Expression::Member(member) => SemanticReferenceExpr {
                span_start: member.span.start,
                kind: if matches!(&*member.object, Expression::Super(_)) {
                    ReferenceExprKind::SuperNamed
                } else {
                    ReferenceExprKind::PropertyNamed
                },
                name: Some(self.identifier(&member.property)),
            },
            Expression::Index(index) => SemanticReferenceExpr {
                span_start: index.span.start,
                kind: if matches!(&*index.object, Expression::Super(_)) {
                    ReferenceExprKind::SuperComputed
                } else {
                    ReferenceExprKind::PropertyComputed
                },
                name: None,
            },
            Expression::Parenthesized(paren) => {
                self.record_reference_expr(&paren.expression);
                return;
            }
            _ => return,
        };
        self.references.push(reference);
    }

    fn record_call_op(&mut self, call: &ast::CallExpression) {
        let kind = match &*call.callee {
            Expression::Identifier(ident) if self.interner.resolve(ident.name) == "eval" => {
                self.uses_direct_eval = true;
                CallOpKind::DirectEval
            }
            Expression::Member(_) | Expression::Index(_) => CallOpKind::Method,
            Expression::Parenthesized(paren)
                if matches!(
                    &*paren.expression,
                    Expression::Identifier(ident) if self.interner.resolve(ident.name) == "eval"
                ) =>
            {
                CallOpKind::IndirectEval
            }
            _ => CallOpKind::Ordinary,
        };
        self.call_ops.push(SemanticCallOp {
            span_start: call.span.start,
            kind,
            callee_span_start: call.callee.span().start,
        });
        if kind == CallOpKind::DirectEval {
            self.record_scope_snapshot(call.span.start);
        }
    }

    fn record_loop_scope_plan(
        &mut self,
        span_start: usize,
        creates_per_iteration_env: bool,
        pattern: Option<&Pattern>,
    ) {
        let Some(pattern) = pattern else {
            return;
        };
        let mut binding_names = Vec::new();
        Self::collect_pattern_names_in_order(pattern, &mut binding_names, self.interner);
        self.loop_scopes.push(LoopScopePlan {
            span_start,
            creates_per_iteration_env,
            binding_names,
        });
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
        self.function_semantics.push(FunctionSemantics {
            span_start,
            kind,
            uses_js_this: matches!(
                kind,
                CallableKind::SyncMethod
                    | CallableKind::AsyncMethod
                    | CallableKind::GeneratorMethod
                    | CallableKind::AsyncGeneratorMethod
                    | CallableKind::Constructor
            ),
        });
        self.function_depth += 1;
        self.arguments_binding_depths.push(self.function_depth);
        self.push_function_scope();
        for param in params {
            self.record_pattern(&param.pattern, BindingKind::Parameter);
            if let Some(default) = &param.default_value {
                self.visit_expr(default);
            }
        }
        if let Some(body) = body {
            self.predeclare_stmt_list(&body.statements);
            self.record_scope_snapshot(span_start);
            for stmt in &body.statements {
                self.visit_stmt(stmt);
            }
        }
        self.pop_scope();
        let _ = self.arguments_binding_depths.pop();
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
                            self.push_block_scope();
                            self.predeclare_stmt_list(&block.statements);
                            for stmt in &block.statements {
                                self.visit_stmt(stmt);
                            }
                            self.pop_scope();
                        }
                    }
                }
                self.clear_binding_tdz(class_decl.name.name);
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
                self.binding_ops.push(SemanticBindingOp {
                    span_start: var_decl.span.start,
                    kind: BindingOpKind::Initialize,
                    name: None,
                    reference_span_start: Some(var_decl.pattern.span().start),
                });
                if let Some(init) = &var_decl.initializer {
                    self.visit_expr(init);
                }
                if matches!(kind, BindingKind::Lexical) {
                    self.clear_pattern_tdz(&var_decl.pattern);
                }
            }
            Statement::Block(block) => {
                self.push_block_scope();
                self.predeclare_stmt_list(&block.statements);
                for stmt in &block.statements {
                    self.visit_stmt(stmt);
                }
                self.pop_scope();
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
                let lexical_loop_scope = matches!(
                    &for_stmt.init,
                    Some(ast::ForInit::VariableDecl(decl))
                        if matches!(decl.kind, VariableKind::Let | VariableKind::Const)
                );
                if lexical_loop_scope {
                    self.push_block_scope();
                }
                if let Some(ast::ForInit::VariableDecl(decl)) = &for_stmt.init {
                    self.record_loop_scope_plan(
                        for_stmt.span.start,
                        matches!(decl.kind, VariableKind::Let | VariableKind::Const),
                        Some(&decl.pattern),
                    );
                }
                if let Some(init) = &for_stmt.init {
                    match init {
                        ast::ForInit::Expression(expr) => self.visit_expr(expr),
                        ast::ForInit::VariableDecl(decl) => {
                            if lexical_loop_scope {
                                let mut names = Vec::new();
                                Self::collect_pattern_symbols(&decl.pattern, &mut names);
                                for name in names {
                                    self.mark_binding_tdz(name);
                                }
                            }
                            self.record_pattern_with_runtime_env(
                                &decl.pattern,
                                match decl.kind {
                                    VariableKind::Var => BindingKind::Var,
                                    VariableKind::Const | VariableKind::Let => BindingKind::Lexical,
                                },
                                lexical_loop_scope,
                            );
                            if let Some(init) = &decl.initializer {
                                self.visit_expr(init);
                            }
                            if lexical_loop_scope {
                                self.clear_pattern_tdz(&decl.pattern);
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
                if lexical_loop_scope {
                    self.pop_scope();
                }
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
                    self.push_block_scope();
                    if let Some(param) = &handler.param {
                        self.record_pattern(param, BindingKind::Lexical);
                    }
                    self.predeclare_stmt_list(&handler.body.statements);
                    for stmt in &handler.body.statements {
                        self.visit_stmt(stmt);
                    }
                    self.pop_scope();
                }
                if let Some(finalizer) = &try_stmt.finally_clause {
                    self.push_block_scope();
                    self.predeclare_stmt_list(&finalizer.statements);
                    for stmt in &finalizer.statements {
                        self.visit_stmt(stmt);
                    }
                    self.pop_scope();
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
                let lexical_loop_scope = matches!(
                    &for_in.left,
                    ast::ForOfLeft::VariableDecl(decl)
                        if matches!(decl.kind, VariableKind::Let | VariableKind::Const)
                );
                if lexical_loop_scope {
                    self.push_block_scope();
                }
                match &for_in.left {
                    ast::ForOfLeft::VariableDecl(decl) => self.record_loop_scope_plan(
                        for_in.span.start,
                        matches!(decl.kind, VariableKind::Let | VariableKind::Const),
                        Some(&decl.pattern),
                    ),
                    ast::ForOfLeft::Pattern(pattern) => {
                        self.record_loop_scope_plan(for_in.span.start, false, Some(pattern))
                    }
                }
                match &for_in.left {
                    ast::ForOfLeft::VariableDecl(decl) => {
                        if lexical_loop_scope {
                            let mut names = Vec::new();
                            Self::collect_pattern_symbols(&decl.pattern, &mut names);
                            for name in names {
                                self.mark_binding_tdz(name);
                            }
                        }
                        self.record_pattern_with_runtime_env(
                            &decl.pattern,
                            match decl.kind {
                                VariableKind::Var => BindingKind::Var,
                                VariableKind::Const | VariableKind::Let => BindingKind::Lexical,
                            },
                            lexical_loop_scope,
                        )
                    }
                    ast::ForOfLeft::Pattern(pattern) => {
                        self.record_pattern(pattern, BindingKind::Lexical)
                    }
                }
                self.visit_expr(&for_in.right);
                if lexical_loop_scope {
                    if let ast::ForOfLeft::VariableDecl(decl) = &for_in.left {
                        self.clear_pattern_tdz(&decl.pattern);
                    }
                }
                self.visit_stmt(&for_in.body);
                if lexical_loop_scope {
                    self.pop_scope();
                }
            }
            Statement::ForOf(for_of) => {
                let lexical_loop_scope = matches!(
                    &for_of.left,
                    ast::ForOfLeft::VariableDecl(decl)
                        if matches!(decl.kind, VariableKind::Let | VariableKind::Const)
                );
                if lexical_loop_scope {
                    self.push_block_scope();
                }
                match &for_of.left {
                    ast::ForOfLeft::VariableDecl(decl) => self.record_loop_scope_plan(
                        for_of.span.start,
                        matches!(decl.kind, VariableKind::Let | VariableKind::Const),
                        Some(&decl.pattern),
                    ),
                    ast::ForOfLeft::Pattern(pattern) => {
                        self.record_loop_scope_plan(for_of.span.start, false, Some(pattern))
                    }
                }
                match &for_of.left {
                    ast::ForOfLeft::VariableDecl(decl) => {
                        if lexical_loop_scope {
                            let mut names = Vec::new();
                            Self::collect_pattern_symbols(&decl.pattern, &mut names);
                            for name in names {
                                self.mark_binding_tdz(name);
                            }
                        }
                        self.record_pattern_with_runtime_env(
                            &decl.pattern,
                            match decl.kind {
                                VariableKind::Var => BindingKind::Var,
                                VariableKind::Const | VariableKind::Let => BindingKind::Lexical,
                            },
                            lexical_loop_scope,
                        )
                    }
                    ast::ForOfLeft::Pattern(pattern) => {
                        self.record_pattern(pattern, BindingKind::Lexical)
                    }
                }
                self.visit_expr(&for_of.right);
                if lexical_loop_scope {
                    if let ast::ForOfLeft::VariableDecl(decl) = &for_of.left {
                        self.clear_pattern_tdz(&decl.pattern);
                    }
                }
                self.visit_stmt(&for_of.body);
                if lexical_loop_scope {
                    self.pop_scope();
                }
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
            Statement::With(with_stmt) => {
                self.visit_expr(&with_stmt.object);
                self.with_depth += 1;
                self.visit_stmt(&with_stmt.body);
                self.with_depth = self.with_depth.saturating_sub(1);
            }
            Statement::Empty(_)
            | Statement::Break(_)
            | Statement::Continue(_)
            | Statement::Debugger(_)
            | Statement::ImportDecl(_)
            | Statement::TypeAliasDecl(_) => {}
        }
    }

    fn visit_expr(&mut self, expr: &Expression) {
        match expr {
            Expression::Call(call) => {
                self.record_call_op(call);
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
                let kind = if arrow.is_async {
                    CallableKind::AsyncFunction
                } else {
                    CallableKind::SyncFunction
                };
                self.callables.push(SemanticCallableInfo {
                    name: None,
                    kind,
                    span_start: arrow.span.start,
                });
                self.function_semantics.push(FunctionSemantics {
                    span_start: arrow.span.start,
                    kind,
                    uses_js_this: false,
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
                self.record_reference_expr(expr);
                self.visit_expr(&member.object);
            }
            Expression::Index(index) => {
                self.record_reference_expr(expr);
                self.visit_expr(&index.object);
                self.visit_expr(&index.index);
            }
            Expression::New(new_expr) => {
                self.call_ops.push(SemanticCallOp {
                    span_start: new_expr.span.start,
                    kind: CallOpKind::Constructor,
                    callee_span_start: new_expr.callee.span().start,
                });
                self.visit_expr(&new_expr.callee);
                for arg in &new_expr.arguments {
                    self.visit_expr(arg.expression());
                }
            }
            Expression::Assignment(assign) => {
                self.record_reference_expr(&assign.left);
                self.binding_ops.push(SemanticBindingOp {
                    span_start: assign.span.start,
                    kind: BindingOpKind::Assign,
                    name: None,
                    reference_span_start: Some(assign.left.span().start),
                });
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
            Expression::Unary(unary) => {
                match unary.operator {
                    UnaryOperator::PrefixIncrement
                    | UnaryOperator::PrefixDecrement
                    | UnaryOperator::PostfixIncrement
                    | UnaryOperator::PostfixDecrement => {
                        self.record_reference_expr(&unary.operand);
                        self.update_ops.push(SemanticUpdateOp {
                            span_start: unary.span.start,
                            kind: match unary.operator {
                                UnaryOperator::PrefixIncrement => UpdateOpKind::PrefixIncrement,
                                UnaryOperator::PrefixDecrement => UpdateOpKind::PrefixDecrement,
                                UnaryOperator::PostfixIncrement => UpdateOpKind::PostfixIncrement,
                                UnaryOperator::PostfixDecrement => UpdateOpKind::PostfixDecrement,
                                _ => unreachable!(),
                            },
                            reference_span_start: unary.operand.span().start,
                        });
                    }
                    UnaryOperator::Delete => {
                        self.record_reference_expr(&unary.operand);
                        self.binding_ops.push(SemanticBindingOp {
                            span_start: unary.span.start,
                            kind: BindingOpKind::Delete,
                            name: None,
                            reference_span_start: Some(unary.operand.span().start),
                        });
                    }
                    _ => {}
                }
                self.visit_expr(&unary.operand)
            }
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
            Expression::Typeof(typeof_expr) => self.visit_expr(&typeof_expr.argument),
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
            Expression::Identifier(ident) => self.record_resolved_identifier(ident),
            | Expression::IntLiteral(_)
            | Expression::FloatLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::RegexLiteral(_)
            | Expression::This(_)
            | Expression::NewTarget(_)
            | Expression::Super(_)
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

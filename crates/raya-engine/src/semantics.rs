//! Shared semantic profile and inspection types.
//!
//! This module centralizes source-kind-driven semantic decisions so parser,
//! checker, lowering, and runtime entrypoints can derive behavior from one
//! profile instead of scattering booleans across layers.

use crate::compiler::module::{
    builtin_surface_manifest_for_mode, builtin_surface_mode_for_profile, BuiltinGlobalKind,
    BuiltinSurfaceManifest,
};
use crate::compiler::type_registry::TypeRegistry;
use crate::parser::ast::{
    self, AssignmentOperator, Expression, FunctionDecl, MethodDecl, Pattern, Statement,
    UnaryOperator, VariableKind,
};
use crate::parser::checker::{CheckerPolicy, EarlyErrorOptions, TsTypeFlags, TypeSystemMode};
use crate::parser::types::ty::{ClassType, InterfaceType, ObjectType, PrimitiveType, Type};
use crate::parser::{Interner, Symbol, TypeContext, TypeId};
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

/// Policy for dynamic JS runtime semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JsDynamicSemantics {
    Disabled,
    EcmaScript,
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
    /// Whether JS runtime semantics may lower to explicit dynamic kernel ops.
    pub js_dynamic_semantics: JsDynamicSemantics,
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
    /// Whether JS runtime semantics may lower to explicit dynamic kernel ops.
    pub js_dynamic_semantics: JsDynamicSemantics,
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
            js_dynamic_semantics: JsDynamicSemantics::EcmaScript,
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
            js_dynamic_semantics: JsDynamicSemantics::EcmaScript,
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
            js_dynamic_semantics: JsDynamicSemantics::EcmaScript,
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
            js_dynamic_semantics: JsDynamicSemantics::Disabled,
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
            js_dynamic_semantics: self.js_dynamic_semantics,
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
    pub span_end: usize,
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
    AmbientGlobal,
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
    pub span_end: usize,
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

/// Semantic object/value shape classification used by lowering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectShapeKind {
    ClassValue,
    NominalInstance,
    StructuralObject,
    BuiltinValue,
    Dynamic,
}

/// Semantic object-shape summary for an expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticObjectShape {
    pub span_start: usize,
    pub span_end: usize,
    pub kind: ObjectShapeKind,
    pub type_id: Option<TypeId>,
    pub type_name: Option<String>,
}

/// Semantic member target classification used by lowering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemberTargetKind {
    NominalField,
    NominalMethod,
    StaticMethod,
    StructuralSlot,
    BuiltinProperty,
    DynamicProperty,
}

/// Semantic member target summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticMemberTarget {
    pub span_start: usize,
    pub span_end: usize,
    pub kind: MemberTargetKind,
    pub name: Option<String>,
    pub receiver_type_id: Option<TypeId>,
    pub receiver_shape_kind: ObjectShapeKind,
}

/// Semantic call target classification used by lowering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallTargetKind {
    PlainFunction,
    NominalMethod,
    StaticMethod,
    StructuralCall,
    ConstructorLikeValue,
    DynamicCall,
}

/// Semantic call target summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticCallTarget {
    pub span_start: usize,
    pub span_end: usize,
    pub kind: CallTargetKind,
    pub receiver_type_id: Option<TypeId>,
    pub member_name: Option<String>,
    pub return_type_id: Option<TypeId>,
    pub return_shape: Option<SemanticObjectShape>,
}

/// Semantic constructor target classification used by lowering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConstructorTargetKind {
    NominalClass,
    ConstructorLikeValue,
    DynamicConstructor,
}

/// Semantic constructor target summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticConstructorTarget {
    pub span_start: usize,
    pub span_end: usize,
    pub kind: ConstructorTargetKind,
    pub instance_shape: Option<SemanticObjectShape>,
    pub callee_type_id: Option<TypeId>,
}

/// Semantic value origin classification used by lowering dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueOriginKind {
    BoundValue,
    ImportedBinding,
    ImportedNamespace,
    BuiltinGlobalValue,
    BuiltinNamespace,
    RuntimeEnvValue,
    RuntimeLateBoundValue,
    DynamicValue,
}

/// Semantic value origin summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticValueOrigin {
    pub span_start: usize,
    pub span_end: usize,
    pub kind: ValueOriginKind,
    pub type_id: Option<TypeId>,
    pub export_name: Option<String>,
}

/// Semantic property dispatch classification used by lowering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PropertyDispatchKind {
    NominalField,
    StructuralSlot,
    ImportedNamespaceExport,
    BuiltinNamespaceProperty,
    RuntimeLateBoundProperty,
    DynamicProperty,
}

/// Semantic property dispatch summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticPropertyDispatch {
    pub span_start: usize,
    pub span_end: usize,
    pub kind: PropertyDispatchKind,
    pub receiver_origin: ValueOriginKind,
    pub receiver_type_id: Option<TypeId>,
    pub property_name: Option<String>,
    pub result_type_id: Option<TypeId>,
}

/// Semantic call dispatch classification used by lowering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallDispatchKind {
    PlainFunction,
    NominalMethod,
    StaticMethod,
    StructuralCall,
    ImportedNamespaceCall,
    BuiltinNamespaceMethod,
    BuiltinInstanceMethod,
    RuntimeLateBoundMethod,
    DynamicCall,
}

/// Call completion behavior selected by the semantic planner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallCompletionKind {
    Sync,
    Task,
    RuntimeClosure,
}

/// Semantic call dispatch summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticCallDispatch {
    pub span_start: usize,
    pub span_end: usize,
    pub kind: CallDispatchKind,
    pub callee_origin: ValueOriginKind,
    pub receiver_origin: Option<ValueOriginKind>,
    pub receiver_type_id: Option<TypeId>,
    pub member_name: Option<String>,
    pub result_type_id: Option<TypeId>,
    pub completion_kind: CallCompletionKind,
}

/// Semantic constructor dispatch classification used by lowering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConstructorDispatchKind {
    NominalClass,
    BuiltinNativeConstructor,
    ImportedConstructorValue,
    RuntimeConstructorValue,
    DynamicConstructor,
}

/// Semantic constructor dispatch summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticConstructorDispatch {
    pub span_start: usize,
    pub span_end: usize,
    pub kind: ConstructorDispatchKind,
    pub callee_origin: ValueOriginKind,
    pub callee_type_id: Option<TypeId>,
    pub result_type_id: Option<TypeId>,
}

/// Semantic builtin value origin classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinOriginKind {
    BuiltinNamespace,
    BuiltinClassValue,
    BuiltinInstance,
    HostHandleValue,
    ImportedBuiltinBinding,
    ImportedBuiltinNamespace,
    DynamicValue,
}

/// Semantic builtin/metaobject dispatch classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinDispatchKind {
    NamespaceProperty,
    NamespaceCall,
    Constructor,
    InstanceProperty,
    InstanceMethod,
    MetaobjectOp,
    IteratorOp,
    HostHandleOp,
    DynamicFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetaobjectOpKind {
    DefineProperty,
    GetOwnPropertyDescriptor,
    DefineProperties,
    DeleteProperty,
    GetPrototypeOf,
    SetPrototypeOf,
    PreventExtensions,
    IsExtensible,
    ReflectGet,
    ReflectSet,
    ReflectHas,
    ReflectOwnKeys,
    ReflectConstruct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IteratorOpKind {
    GetIterator,
    GetAsyncIterator,
    Step,
    Done,
    Value,
    ResumeNext,
    ResumeReturn,
    ResumeThrow,
    Close,
    CloseOnThrow,
    CloseCompletion,
    AppendToArray,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostHandleOpKind {
    ChannelConstructor,
    MutexConstructor,
    MutexLock,
    MutexUnlock,
    TaskCancel,
    TaskIsDone,
    TaskIsCancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JsOpKind {
    GetNamed,
    GetKeyed,
    SetNamed { strict: bool },
    SetKeyed { strict: bool },
    BindMethod,
    ResolveIdentifier { non_throwing: bool },
    AssignIdentifier { strict: bool },
    CallValue,
    CallMemberNamed,
    CallMemberKeyed,
    ConstructValue,
    PushWithEnv,
    PopWithEnv,
    PushDeclarativeEnv,
    PopDeclarativeEnv,
    ReplaceDeclarativeEnv,
    DirectEval,
    EvalGetCompletion,
    EvalSetCompletion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinNamespaceCallKind {
    ObjectSameValue,
    JsonParse,
    JsonStringify,
    StringFromCharCode,
    NumberIsNaN,
    NumberIsFinite,
    DateNow,
    DateParse,
}

/// Semantic builtin/metaobject dispatch summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticBuiltinDispatch {
    pub span_start: usize,
    pub span_end: usize,
    pub origin: BuiltinOriginKind,
    pub kind: BuiltinDispatchKind,
    pub metaobject_op: Option<MetaobjectOpKind>,
    pub iterator_op: Option<IteratorOpKind>,
    pub host_handle_op: Option<HostHandleOpKind>,
    pub namespace_call: Option<BuiltinNamespaceCallKind>,
    pub registry_dispatch: Option<crate::compiler::type_registry::DispatchAction>,
    pub native_id: Option<u16>,
    pub receiver_type_id: Option<TypeId>,
    pub callee_type_id: Option<TypeId>,
    pub result_type_id: Option<TypeId>,
    pub property_name: Option<String>,
    pub member_name: Option<String>,
    pub export_name: Option<String>,
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
    pub value_origins: Vec<SemanticValueOrigin>,
    pub object_shapes: Vec<SemanticObjectShape>,
    pub member_targets: Vec<SemanticMemberTarget>,
    pub property_dispatches: Vec<SemanticPropertyDispatch>,
    pub builtin_dispatches: Vec<SemanticBuiltinDispatch>,
    pub binding_ops: Vec<SemanticBindingOp>,
    pub update_ops: Vec<SemanticUpdateOp>,
    pub call_ops: Vec<SemanticCallOp>,
    pub call_targets: Vec<SemanticCallTarget>,
    pub call_dispatches: Vec<SemanticCallDispatch>,
    pub constructor_targets: Vec<SemanticConstructorTarget>,
    pub constructor_dispatches: Vec<SemanticConstructorDispatch>,
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

type SpanKey = (usize, usize);

fn span_key(span_start: usize, span_end: usize) -> SpanKey {
    (span_start, span_end)
}

/// Semantic lowering plan derived from a module before IR lowering starts.
#[derive(Debug, Clone)]
pub struct SemanticLoweringPlan {
    pub hir: SemanticHirModule,
    callable_kinds_by_span: FxHashMap<usize, CallableKind>,
    references_by_span: FxHashMap<SpanKey, SemanticReferenceExpr>,
    resolved_identifiers_by_span: FxHashMap<usize, SemanticResolvedIdentifier>,
    value_origins_by_span: FxHashMap<SpanKey, SemanticValueOrigin>,
    object_shapes_by_span: FxHashMap<SpanKey, SemanticObjectShape>,
    member_targets_by_span: FxHashMap<SpanKey, SemanticMemberTarget>,
    property_dispatches_by_span: FxHashMap<SpanKey, SemanticPropertyDispatch>,
    builtin_dispatches_by_span: FxHashMap<SpanKey, SemanticBuiltinDispatch>,
    binding_ops_by_span: FxHashMap<usize, SemanticBindingOp>,
    update_ops_by_span: FxHashMap<usize, SemanticUpdateOp>,
    call_ops_by_span: FxHashMap<SpanKey, SemanticCallOp>,
    call_targets_by_span: FxHashMap<SpanKey, SemanticCallTarget>,
    call_dispatches_by_span: FxHashMap<SpanKey, SemanticCallDispatch>,
    constructor_targets_by_span: FxHashMap<SpanKey, SemanticConstructorTarget>,
    constructor_dispatches_by_span: FxHashMap<SpanKey, SemanticConstructorDispatch>,
    destructuring_by_span: FxHashMap<usize, DestructuringPlan>,
    loop_scopes_by_span: FxHashMap<usize, LoopScopePlan>,
    scope_snapshots_by_span: FxHashMap<SpanKey, Vec<ScopeSnapshotBinding>>,
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
                value_origins: Vec::new(),
                object_shapes: Vec::new(),
                member_targets: Vec::new(),
                property_dispatches: Vec::new(),
                builtin_dispatches: Vec::new(),
                binding_ops: Vec::new(),
                update_ops: Vec::new(),
                call_ops: Vec::new(),
                call_targets: Vec::new(),
                call_dispatches: Vec::new(),
                constructor_targets: Vec::new(),
                constructor_dispatches: Vec::new(),
                destructuring_plans: Vec::new(),
                loop_scopes: Vec::new(),
                suspension_points: Vec::new(),
                uses_direct_eval: false,
            },
            callable_kinds_by_span: FxHashMap::default(),
            references_by_span: FxHashMap::default(),
            resolved_identifiers_by_span: FxHashMap::default(),
            value_origins_by_span: FxHashMap::default(),
            object_shapes_by_span: FxHashMap::default(),
            member_targets_by_span: FxHashMap::default(),
            property_dispatches_by_span: FxHashMap::default(),
            builtin_dispatches_by_span: FxHashMap::default(),
            binding_ops_by_span: FxHashMap::default(),
            update_ops_by_span: FxHashMap::default(),
            call_ops_by_span: FxHashMap::default(),
            call_targets_by_span: FxHashMap::default(),
            call_dispatches_by_span: FxHashMap::default(),
            constructor_targets_by_span: FxHashMap::default(),
            constructor_dispatches_by_span: FxHashMap::default(),
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

    pub fn reference_at_span(
        &self,
        span_start: usize,
        span_end: usize,
    ) -> Option<&SemanticReferenceExpr> {
        self.references_by_span.get(&span_key(span_start, span_end))
    }

    pub fn resolved_identifier_at_span(
        &self,
        span_start: usize,
    ) -> Option<&SemanticResolvedIdentifier> {
        self.resolved_identifiers_by_span.get(&span_start)
    }

    pub fn value_origin_at_span(
        &self,
        span_start: usize,
        span_end: usize,
    ) -> Option<&SemanticValueOrigin> {
        self.value_origins_by_span
            .get(&span_key(span_start, span_end))
    }

    pub fn object_shape_at_span(
        &self,
        span_start: usize,
        span_end: usize,
    ) -> Option<&SemanticObjectShape> {
        self.object_shapes_by_span
            .get(&span_key(span_start, span_end))
    }

    pub fn member_target_at_span(
        &self,
        span_start: usize,
        span_end: usize,
    ) -> Option<&SemanticMemberTarget> {
        self.member_targets_by_span
            .get(&span_key(span_start, span_end))
    }

    pub fn property_dispatch_at_span(
        &self,
        span_start: usize,
        span_end: usize,
    ) -> Option<&SemanticPropertyDispatch> {
        self.property_dispatches_by_span
            .get(&span_key(span_start, span_end))
    }

    pub fn builtin_dispatch_at_span(
        &self,
        span_start: usize,
        span_end: usize,
    ) -> Option<&SemanticBuiltinDispatch> {
        self.builtin_dispatches_by_span
            .get(&span_key(span_start, span_end))
    }

    pub fn binding_op_at_span(&self, span_start: usize) -> Option<&SemanticBindingOp> {
        self.binding_ops_by_span.get(&span_start)
    }

    pub fn update_op_at_span(&self, span_start: usize) -> Option<&SemanticUpdateOp> {
        self.update_ops_by_span.get(&span_start)
    }

    pub fn call_op_at_span(&self, span_start: usize, span_end: usize) -> Option<&SemanticCallOp> {
        self.call_ops_by_span.get(&span_key(span_start, span_end))
    }

    pub fn call_target_at_span(
        &self,
        span_start: usize,
        span_end: usize,
    ) -> Option<&SemanticCallTarget> {
        self.call_targets_by_span
            .get(&span_key(span_start, span_end))
    }

    pub fn call_dispatch_at_span(
        &self,
        span_start: usize,
        span_end: usize,
    ) -> Option<&SemanticCallDispatch> {
        self.call_dispatches_by_span
            .get(&span_key(span_start, span_end))
    }

    pub fn constructor_target_at_span(
        &self,
        span_start: usize,
        span_end: usize,
    ) -> Option<&SemanticConstructorTarget> {
        self.constructor_targets_by_span
            .get(&span_key(span_start, span_end))
    }

    pub fn constructor_dispatch_at_span(
        &self,
        span_start: usize,
        span_end: usize,
    ) -> Option<&SemanticConstructorDispatch> {
        self.constructor_dispatches_by_span
            .get(&span_key(span_start, span_end))
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
        span_end: usize,
    ) -> Option<&[ScopeSnapshotBinding]> {
        self.scope_snapshots_by_span
            .get(&span_key(span_start, span_end))
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
    build_semantic_lowering_plan_with_types(module, interner, profile, None, None)
}

/// Build a lowering-oriented semantic plan from an AST module plus checker type data.
pub fn build_semantic_lowering_plan_with_types(
    module: &ast::Module,
    interner: &Interner,
    profile: SemanticProfile,
    type_ctx: Option<&TypeContext>,
    expr_types: Option<&FxHashMap<usize, TypeId>>,
) -> SemanticLoweringPlan {
    let builtin_surface =
        builtin_surface_manifest_for_mode(builtin_surface_mode_for_profile(profile));
    let mut builder = SemanticHirBuilder {
        interner,
        typed: type_ctx
            .zip(expr_types)
            .map(|(type_ctx, expr_types)| TypedSemanticInfo {
                type_ctx,
                expr_types,
            }),
        builtin_surface,
        type_registry: type_ctx.map(|type_ctx| TypeRegistry::new(type_ctx, builtin_surface)),
        callables: Vec::new(),
        function_semantics: Vec::new(),
        bindings: Vec::new(),
        references: Vec::new(),
        resolved_identifiers: Vec::new(),
        value_origins: Vec::new(),
        object_shapes: Vec::new(),
        member_targets: Vec::new(),
        property_dispatches: Vec::new(),
        builtin_dispatches: Vec::new(),
        binding_ops: Vec::new(),
        update_ops: Vec::new(),
        call_ops: Vec::new(),
        call_targets: Vec::new(),
        call_dispatches: Vec::new(),
        constructor_targets: Vec::new(),
        constructor_dispatches: Vec::new(),
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
        class_accessor_names: FxHashMap::default(),
        class_static_accessor_names: FxHashMap::default(),
        scope_snapshots_by_span: FxHashMap::default(),
        imported_symbols: FxHashSet::default(),
        imported_namespace_symbols: FxHashSet::default(),
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
        value_origins: builder.value_origins,
        object_shapes: builder.object_shapes,
        member_targets: builder.member_targets,
        property_dispatches: builder.property_dispatches,
        builtin_dispatches: builder.builtin_dispatches,
        binding_ops: builder.binding_ops,
        update_ops: builder.update_ops,
        call_ops: builder.call_ops,
        call_targets: builder.call_targets,
        call_dispatches: builder.call_dispatches,
        constructor_targets: builder.constructor_targets,
        constructor_dispatches: builder.constructor_dispatches,
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
        .map(|reference| {
            (
                span_key(reference.span_start, reference.span_end),
                reference,
            )
        })
        .collect();
    let resolved_identifiers_by_span = hir
        .resolved_identifiers
        .iter()
        .cloned()
        .map(|resolved| (resolved.span_start, resolved))
        .collect();
    let value_origins_by_span = hir
        .value_origins
        .iter()
        .cloned()
        .map(|origin| (span_key(origin.span_start, origin.span_end), origin))
        .collect();
    let object_shapes_by_span = hir
        .object_shapes
        .iter()
        .cloned()
        .map(|shape| (span_key(shape.span_start, shape.span_end), shape))
        .collect();
    let member_targets_by_span = hir
        .member_targets
        .iter()
        .cloned()
        .map(|target| (span_key(target.span_start, target.span_end), target))
        .collect();
    let property_dispatches_by_span = hir
        .property_dispatches
        .iter()
        .cloned()
        .map(|dispatch| (span_key(dispatch.span_start, dispatch.span_end), dispatch))
        .collect();
    let builtin_dispatches_by_span = hir
        .builtin_dispatches
        .iter()
        .cloned()
        .map(|dispatch| (span_key(dispatch.span_start, dispatch.span_end), dispatch))
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
        .map(|op| (span_key(op.span_start, op.span_end), op))
        .collect();
    let call_targets_by_span = hir
        .call_targets
        .iter()
        .cloned()
        .map(|target| (span_key(target.span_start, target.span_end), target))
        .collect();
    let call_dispatches_by_span = hir
        .call_dispatches
        .iter()
        .cloned()
        .map(|dispatch| (span_key(dispatch.span_start, dispatch.span_end), dispatch))
        .collect();
    let constructor_targets_by_span = hir
        .constructor_targets
        .iter()
        .cloned()
        .map(|target| (span_key(target.span_start, target.span_end), target))
        .collect();
    let constructor_dispatches_by_span = hir
        .constructor_dispatches
        .iter()
        .cloned()
        .map(|dispatch| (span_key(dispatch.span_start, dispatch.span_end), dispatch))
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
        value_origins_by_span,
        object_shapes_by_span,
        member_targets_by_span,
        property_dispatches_by_span,
        builtin_dispatches_by_span,
        binding_ops_by_span,
        update_ops_by_span,
        call_ops_by_span,
        call_targets_by_span,
        call_dispatches_by_span,
        constructor_targets_by_span,
        constructor_dispatches_by_span,
        destructuring_by_span,
        loop_scopes_by_span,
        scope_snapshots_by_span: builder
            .scope_snapshots_by_span
            .into_iter()
            .map(|((span_start, span_end), bindings)| (span_key(span_start, span_end), bindings))
            .collect(),
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
    value_facts: FxHashMap<Symbol, ScopeValueFact>,
}

impl ScopeFrame {
    fn new(kind: ScopeFrameKind) -> Self {
        Self {
            kind,
            bindings: FxHashMap::default(),
            value_facts: FxHashMap::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SemanticBoundMethodInfo {
    kind: CallTargetKind,
    receiver_type_id: Option<TypeId>,
    member_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScopeValueFact {
    shape: Option<SemanticObjectShape>,
    bound_method: Option<SemanticBoundMethodInfo>,
}

#[derive(Clone, Copy)]
struct TypedSemanticInfo<'a> {
    type_ctx: &'a TypeContext,
    expr_types: &'a FxHashMap<usize, TypeId>,
}

struct SemanticHirBuilder<'a> {
    interner: &'a Interner,
    typed: Option<TypedSemanticInfo<'a>>,
    builtin_surface: &'static BuiltinSurfaceManifest,
    type_registry: Option<TypeRegistry>,
    callables: Vec<SemanticCallableInfo>,
    function_semantics: Vec<FunctionSemantics>,
    bindings: Vec<SemanticBindingInfo>,
    references: Vec<SemanticReferenceExpr>,
    resolved_identifiers: Vec<SemanticResolvedIdentifier>,
    value_origins: Vec<SemanticValueOrigin>,
    object_shapes: Vec<SemanticObjectShape>,
    member_targets: Vec<SemanticMemberTarget>,
    property_dispatches: Vec<SemanticPropertyDispatch>,
    builtin_dispatches: Vec<SemanticBuiltinDispatch>,
    binding_ops: Vec<SemanticBindingOp>,
    update_ops: Vec<SemanticUpdateOp>,
    call_ops: Vec<SemanticCallOp>,
    call_targets: Vec<SemanticCallTarget>,
    call_dispatches: Vec<SemanticCallDispatch>,
    constructor_targets: Vec<SemanticConstructorTarget>,
    constructor_dispatches: Vec<SemanticConstructorDispatch>,
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
    class_accessor_names: FxHashMap<String, FxHashSet<String>>,
    class_static_accessor_names: FxHashMap<String, FxHashSet<String>>,
    scope_snapshots_by_span: FxHashMap<SpanKey, Vec<ScopeSnapshotBinding>>,
    imported_symbols: FxHashSet<Symbol>,
    imported_namespace_symbols: FxHashSet<Symbol>,
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

    fn resolve_scope_value_fact(&self, symbol: Symbol) -> Option<&ScopeValueFact> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.value_facts.get(&symbol))
    }

    fn binding_scope_index(&self, symbol: Symbol) -> Option<usize> {
        self.scopes
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, scope)| scope.bindings.contains_key(&symbol).then_some(index))
    }

    fn set_scope_value_fact(&mut self, symbol: Symbol, fact: Option<ScopeValueFact>) {
        let Some(scope_index) = self.binding_scope_index(symbol) else {
            return;
        };
        if let Some(scope) = self.scopes.get_mut(scope_index) {
            if let Some(fact) = fact {
                scope.value_facts.insert(symbol, fact);
            } else {
                scope.value_facts.remove(&symbol);
            }
        }
    }

    fn clear_pattern_value_facts(&mut self, pattern: &Pattern) {
        let mut names = Vec::new();
        Self::collect_pattern_symbols(pattern, &mut names);
        for name in names {
            self.set_scope_value_fact(name, None);
        }
    }

    fn resolved_identifier_kind_for_ident(
        &self,
        ident: &ast::Identifier,
    ) -> (ResolvedIdentifierKind, Option<ScopeBindingInfo>) {
        let name = self.identifier(ident);
        let implicit_arguments_binding_depth = (name == "arguments")
            .then(|| self.arguments_binding_depths.last().copied())
            .flatten();
        let binding = self.resolve_scope_binding(ident.name);
        let kind = if self.with_depth > 0 || binding.is_some_and(|binding| binding.runtime_env) {
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
        } else if self.is_builtin_global_name(&name) {
            ResolvedIdentifierKind::BuiltinGlobal
        } else {
            ResolvedIdentifierKind::AmbientGlobal
        };
        (kind, binding)
    }

    fn record_resolved_identifier(&mut self, ident: &ast::Identifier) {
        let name = self.identifier(ident);
        let (kind, binding) = self.resolved_identifier_kind_for_ident(ident);
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

    fn expr_type(&self, expr: &Expression) -> Option<TypeId> {
        let typed = self.typed?;
        typed
            .expr_types
            .get(&(expr as *const Expression as usize))
            .copied()
    }

    fn binding_kind_for_identifier_expr(&self, expr: &Expression) -> Option<BindingKind> {
        let Expression::Identifier(ident) = expr else {
            return None;
        };
        self.resolve_scope_binding(ident.name)
            .map(|binding| binding.kind)
    }

    fn value_origin_kind_for_expr(&self, expr: &Expression) -> ValueOriginKind {
        match expr {
            Expression::Identifier(ident) => {
                let name = self.interner.resolve(ident.name);
                if self.imported_namespace_symbols.contains(&ident.name) {
                    ValueOriginKind::ImportedNamespace
                } else if self.is_builtin_namespace_name(name) {
                    ValueOriginKind::BuiltinNamespace
                } else if self.imported_symbols.contains(&ident.name) {
                    ValueOriginKind::ImportedBinding
                } else {
                    match self.resolved_identifier_kind_for_ident(ident).0 {
                        ResolvedIdentifierKind::LocalBinding
                        | ResolvedIdentifierKind::CaptureBinding
                        | ResolvedIdentifierKind::ScriptGlobalBinding => {
                            ValueOriginKind::BoundValue
                        }
                        ResolvedIdentifierKind::RuntimeEnvLookup => {
                            ValueOriginKind::RuntimeEnvValue
                        }
                        ResolvedIdentifierKind::BuiltinGlobal => {
                            ValueOriginKind::BuiltinGlobalValue
                        }
                        ResolvedIdentifierKind::AmbientGlobal => {
                            let type_id = self.expr_type(expr);
                            if self.runtime_late_bound_receiver(expr, type_id) {
                                ValueOriginKind::RuntimeLateBoundValue
                            } else {
                                ValueOriginKind::DynamicValue
                            }
                        }
                    }
                }
            }
            Expression::Parenthesized(paren) => self.value_origin_kind_for_expr(&paren.expression),
            Expression::TypeCast(cast) => self.value_origin_kind_for_expr(&cast.object),
            Expression::Member(member) => {
                let receiver_origin = self.value_origin_kind_for_expr(&member.object);
                match receiver_origin {
                    ValueOriginKind::ImportedNamespace => ValueOriginKind::ImportedBinding,
                    ValueOriginKind::BuiltinNamespace => ValueOriginKind::BuiltinGlobalValue,
                    ValueOriginKind::RuntimeLateBoundValue => {
                        ValueOriginKind::RuntimeLateBoundValue
                    }
                    _ => {
                        let member_name = self.identifier(&member.property);
                        let receiver_shape = self.object_shape_for_expr(&member.object);
                        let member_target_kind = receiver_shape
                            .as_ref()
                            .map(|shape| {
                                self.member_target_kind_for_shape_and_name(shape, &member_name)
                            })
                            .unwrap_or(MemberTargetKind::DynamicProperty);
                        match member_target_kind {
                            MemberTargetKind::NominalField
                            | MemberTargetKind::NominalMethod
                            | MemberTargetKind::StaticMethod
                            | MemberTargetKind::StructuralSlot
                            | MemberTargetKind::BuiltinProperty => ValueOriginKind::BoundValue,
                            MemberTargetKind::DynamicProperty => {
                                let receiver_type_id =
                                    receiver_shape.as_ref().and_then(|shape| shape.type_id);
                                if self
                                    .runtime_late_bound_receiver(&member.object, receiver_type_id)
                                {
                                    ValueOriginKind::RuntimeLateBoundValue
                                } else {
                                    ValueOriginKind::DynamicValue
                                }
                            }
                        }
                    }
                }
            }
            Expression::Call(call) => match self
                .call_dispatches
                .iter()
                .rev()
                .find(|dispatch| {
                    dispatch.span_start == call.span.start && dispatch.span_end == call.span.end
                })
                .map(|dispatch| dispatch.kind)
            {
                Some(CallDispatchKind::NominalMethod)
                | Some(CallDispatchKind::StaticMethod)
                | Some(CallDispatchKind::StructuralCall)
                | Some(CallDispatchKind::PlainFunction)
                | Some(CallDispatchKind::BuiltinInstanceMethod)
                | Some(CallDispatchKind::ImportedNamespaceCall)
                | Some(CallDispatchKind::BuiltinNamespaceMethod) => ValueOriginKind::BoundValue,
                Some(CallDispatchKind::RuntimeLateBoundMethod) => {
                    ValueOriginKind::RuntimeLateBoundValue
                }
                Some(CallDispatchKind::DynamicCall) | None => ValueOriginKind::DynamicValue,
            },
            Expression::New(new_expr) => match self
                .constructor_dispatches
                .iter()
                .rev()
                .find(|dispatch| {
                    dispatch.span_start == new_expr.span.start
                        && dispatch.span_end == new_expr.span.end
                })
                .map(|dispatch| dispatch.kind)
            {
                Some(ConstructorDispatchKind::NominalClass)
                | Some(ConstructorDispatchKind::BuiltinNativeConstructor)
                | Some(ConstructorDispatchKind::ImportedConstructorValue)
                | Some(ConstructorDispatchKind::RuntimeConstructorValue) => {
                    ValueOriginKind::BoundValue
                }
                Some(ConstructorDispatchKind::DynamicConstructor) | None => {
                    ValueOriginKind::DynamicValue
                }
            },
            _ => {
                let type_id = self.expr_type(expr);
                if self.runtime_late_bound_receiver(expr, type_id) {
                    ValueOriginKind::RuntimeLateBoundValue
                } else {
                    ValueOriginKind::BoundValue
                }
            }
        }
    }

    fn record_value_origin_for_expr(&mut self, expr: &Expression) {
        self.value_origins.push(SemanticValueOrigin {
            span_start: expr.span().start,
            span_end: expr.span().end,
            kind: self.value_origin_kind_for_expr(expr),
            type_id: self.expr_type(expr),
            export_name: match expr {
                Expression::Member(member) => Some(self.identifier(&member.property)),
                _ => None,
            },
        });
    }

    fn object_shape_kind_for_type_id(
        &self,
        ty_id: TypeId,
        binding_kind: Option<BindingKind>,
    ) -> ObjectShapeKind {
        let Some(typed) = self.typed else {
            return ObjectShapeKind::Dynamic;
        };

        if binding_kind == Some(BindingKind::Class) {
            return ObjectShapeKind::ClassValue;
        }

        match typed.type_ctx.get(ty_id) {
            Some(Type::Reference(reference)) => typed
                .type_ctx
                .lookup_named_type(&reference.name)
                .map(|resolved| self.object_shape_kind_for_type_id(resolved, binding_kind))
                .unwrap_or(ObjectShapeKind::Dynamic),
            Some(Type::Generic(generic)) => {
                self.object_shape_kind_for_type_id(generic.base, binding_kind)
            }
            Some(Type::TypeVar(tv)) => tv
                .constraint
                .map(|constraint| self.object_shape_kind_for_type_id(constraint, binding_kind))
                .unwrap_or(ObjectShapeKind::Dynamic),
            Some(Type::Union(union)) => {
                let mut kind = None;
                for member in &union.members {
                    let member_kind = self.object_shape_kind_for_type_id(*member, binding_kind);
                    match kind {
                        None => kind = Some(member_kind),
                        Some(existing) if existing == member_kind => {}
                        _ => return ObjectShapeKind::Dynamic,
                    }
                }
                kind.unwrap_or(ObjectShapeKind::Dynamic)
            }
            Some(Type::Class(_)) => ObjectShapeKind::NominalInstance,
            Some(Type::Object(_) | Type::Interface(_)) => ObjectShapeKind::StructuralObject,
            Some(
                Type::Function(_)
                | Type::Array(_)
                | Type::Task(_)
                | Type::Mutex
                | Type::RegExp
                | Type::Channel(_)
                | Type::Map(_)
                | Type::Set(_)
                | Type::Date
                | Type::Buffer
                | Type::Tuple(_),
            ) => ObjectShapeKind::BuiltinValue,
            Some(
                Type::Primitive(PrimitiveType::String)
                | Type::Primitive(PrimitiveType::Number)
                | Type::Primitive(PrimitiveType::Boolean),
            ) => ObjectShapeKind::BuiltinValue,
            _ => ObjectShapeKind::Dynamic,
        }
    }

    fn type_name_for_type_id(&self, ty_id: TypeId) -> Option<String> {
        let typed = self.typed?;
        match typed.type_ctx.get(ty_id) {
            Some(Type::Class(class_ty)) => Some(class_ty.name.clone()),
            Some(Type::Interface(interface_ty)) => Some(interface_ty.name.clone()),
            Some(Type::Reference(reference)) => Some(reference.name.clone()),
            Some(Type::Array(_)) => Some(TypeContext::ARRAY_TYPE_NAME.to_string()),
            Some(Type::Task(_)) => Some(TypeContext::PROMISE_TYPE_NAME.to_string()),
            Some(Type::Mutex) => Some(TypeContext::MUTEX_TYPE_NAME.to_string()),
            Some(Type::Channel(_)) => Some(TypeContext::CHANNEL_TYPE_NAME.to_string()),
            Some(Type::Map(_)) => Some(TypeContext::MAP_TYPE_NAME.to_string()),
            Some(Type::Set(_)) => Some(TypeContext::SET_TYPE_NAME.to_string()),
            Some(Type::RegExp) => Some("RegExp".to_string()),
            Some(Type::Date) => Some("Date".to_string()),
            Some(Type::Buffer) => Some("Buffer".to_string()),
            _ => None,
        }
    }

    fn canonical_surface_type_name_for_expr(
        &self,
        expr: &Expression,
        type_id: Option<TypeId>,
    ) -> Option<String> {
        match expr {
            Expression::Identifier(ident) => {
                let name = self.identifier(ident);
                self.builtin_surface
                    .backing_type_name(&name)
                    .map(str::to_string)
                    .or_else(|| type_id.and_then(|ty| self.type_name_for_type_id(ty)))
            }
            Expression::New(new_expr) => self
                .builtin_global_name_for_expr(&new_expr.callee)
                .and_then(|name| self.builtin_surface.backing_type_name(&name).map(str::to_string))
                .or_else(|| type_id.and_then(|ty| self.type_name_for_type_id(ty))),
            Expression::Parenthesized(paren) => {
                self.canonical_surface_type_name_for_expr(&paren.expression, type_id)
            }
            Expression::TypeCast(cast) => {
                self.canonical_surface_type_name_for_expr(&cast.object, type_id)
            }
            _ => type_id.and_then(|ty| self.type_name_for_type_id(ty)),
        }
    }

    fn has_local_class_named(&self, name: &str) -> bool {
        self.scopes.iter().rev().any(|scope| {
            scope.bindings.iter().any(|(&symbol, info)| {
                info.kind == BindingKind::Class && self.interner.resolve(symbol) == name
            })
        })
    }

    fn is_builtin_namespace_name(&self, name: &str) -> bool {
        self.builtin_surface.is_namespace_global(name)
    }

    fn is_builtin_global_name(&self, name: &str) -> bool {
        self.builtin_surface.is_builtin_global(name)
    }

    fn dispatch_type_id_for_type_id(&self, ty_id: TypeId) -> Option<u32> {
        let typed = self.typed?;
        let registry = self.type_registry.as_ref()?;
        let normalized = registry.normalize_type(ty_id.as_u32(), typed.type_ctx);
        match normalized {
            Ok(id) if id != crate::compiler::type_registry::UNRESOLVED_TYPE_ID => Some(id),
            _ => None,
        }
    }

    fn receiver_has_builtin_registry_property(
        &self,
        receiver_expr: &Expression,
        receiver_ty: Option<TypeId>,
        name: &str,
    ) -> bool {
        let Some(type_ctx) = self.typed.map(|typed| typed.type_ctx) else {
            return false;
        };
        self.receiver_builtin_surface_type_name_for_expr(receiver_expr, receiver_ty)
            .and_then(|type_name| {
                self.builtin_surface
                    .instance_property_binding(&type_name, name)
            })
            .and_then(|binding| binding.to_dispatch_action(type_ctx))
            .is_some()
    }

    fn receiver_has_builtin_registry_method(
        &self,
        receiver_expr: &Expression,
        receiver_ty: Option<TypeId>,
        name: &str,
    ) -> bool {
        let Some(type_ctx) = self.typed.map(|typed| typed.type_ctx) else {
            return false;
        };
        self.receiver_builtin_surface_type_name_for_expr(receiver_expr, receiver_ty)
            .and_then(|type_name| self.builtin_surface.instance_method_binding(&type_name, name))
            .and_then(|binding| binding.to_dispatch_action(type_ctx))
            .is_some()
    }

    fn type_uses_wrapper_method_surface(&self, receiver_ty: TypeId) -> bool {
        self.receiver_builtin_surface_type_name(Some(receiver_ty))
            .as_deref()
            .and_then(|name| self.builtin_surface.type_surface(name))
            .is_some_and(|surface| surface.wrapper_method_surface)
    }

    fn receiver_builtin_registry_property_dispatch(
        &self,
        receiver_expr: &Expression,
        receiver_ty: Option<TypeId>,
        name: &str,
    ) -> Option<crate::compiler::type_registry::DispatchAction> {
        let type_ctx = self.typed?.type_ctx;
        self.receiver_builtin_surface_type_name_for_expr(receiver_expr, receiver_ty)
            .and_then(|type_name| {
                self.builtin_surface
                    .instance_property_binding(&type_name, name)
            })
            .and_then(|binding| binding.to_dispatch_action(type_ctx))
    }

    fn receiver_builtin_registry_method_dispatch(
        &self,
        receiver_expr: &Expression,
        receiver_ty: Option<TypeId>,
        name: &str,
    ) -> Option<crate::compiler::type_registry::DispatchAction> {
        let type_ctx = self.typed?.type_ctx;
        self.receiver_builtin_surface_type_name_for_expr(receiver_expr, receiver_ty)
            .and_then(|type_name| self.builtin_surface.instance_method_binding(&type_name, name))
            .and_then(|binding| binding.to_dispatch_action(type_ctx))
    }

    fn receiver_builtin_surface_type_name_for_expr(
        &self,
        receiver_expr: &Expression,
        receiver_ty: Option<TypeId>,
    ) -> Option<String> {
        self.object_shape_for_expr(receiver_expr)
            .and_then(|shape| shape.type_name)
            .or_else(|| self.canonical_surface_type_name_for_expr(receiver_expr, receiver_ty))
            .or_else(|| self.receiver_builtin_surface_type_name(receiver_ty))
    }

    fn receiver_builtin_surface_type_name(&self, receiver_ty: Option<TypeId>) -> Option<String> {
        let receiver_ty = receiver_ty?;
        self.dispatch_type_id_for_type_id(receiver_ty)
            .and_then(|dispatch_ty| {
                self.type_registry
                    .as_ref()
                    .and_then(|registry| registry.type_name(dispatch_ty))
                    .map(str::to_string)
            })
            .or_else(|| self.type_name_for_type_id(receiver_ty))
    }

    fn type_name_is_host_handle(name: &str) -> bool {
        matches!(
            name,
            TypeContext::PROMISE_TYPE_NAME
                | TypeContext::CHANNEL_TYPE_NAME
                | TypeContext::MUTEX_TYPE_NAME
        )
    }

    fn builtin_global_name_for_expr(&self, expr: &Expression) -> Option<String> {
        match expr {
            Expression::Identifier(ident) => {
                let name = self.identifier(ident);
                self.is_builtin_global_name(&name).then_some(name)
            }
            Expression::Parenthesized(paren) => self.builtin_global_name_for_expr(&paren.expression),
            Expression::TypeCast(cast) => self.builtin_global_name_for_expr(&cast.object),
            _ => None,
        }
    }

    fn builtin_static_method_binding_for_expr(
        &self,
        expr: &Expression,
        member_name: &str,
    ) -> Option<(String, &crate::compiler::module::BuiltinDispatchBinding)> {
        let global_name = self.builtin_global_name_for_expr(expr)?;
        self.builtin_surface
            .static_method_binding(&global_name, member_name)
            .map(|binding| (global_name, binding))
    }

    fn builtin_static_property_binding_for_expr(
        &self,
        expr: &Expression,
        property_name: &str,
    ) -> Option<(String, &crate::compiler::module::BuiltinDispatchBinding)> {
        let global_name = self.builtin_global_name_for_expr(expr)?;
        self.builtin_surface
            .static_property_binding(&global_name, property_name)
            .map(|binding| (global_name, binding))
    }

    fn metaobject_op_from_native_id(native_id: u16) -> Option<MetaobjectOpKind> {
        match native_id {
            crate::compiler::native_id::OBJECT_DEFINE_PROPERTY => {
                Some(MetaobjectOpKind::DefineProperty)
            }
            crate::compiler::native_id::OBJECT_GET_OWN_PROPERTY_DESCRIPTOR => {
                Some(MetaobjectOpKind::GetOwnPropertyDescriptor)
            }
            crate::compiler::native_id::OBJECT_DEFINE_PROPERTIES => {
                Some(MetaobjectOpKind::DefineProperties)
            }
            crate::compiler::native_id::OBJECT_DELETE_PROPERTY => {
                Some(MetaobjectOpKind::DeleteProperty)
            }
            crate::compiler::native_id::OBJECT_GET_PROTOTYPE_OF => {
                Some(MetaobjectOpKind::GetPrototypeOf)
            }
            crate::compiler::native_id::OBJECT_SET_PROTOTYPE_OF => {
                Some(MetaobjectOpKind::SetPrototypeOf)
            }
            crate::compiler::native_id::OBJECT_PREVENT_EXTENSIONS => {
                Some(MetaobjectOpKind::PreventExtensions)
            }
            crate::compiler::native_id::OBJECT_IS_EXTENSIBLE => {
                Some(MetaobjectOpKind::IsExtensible)
            }
            crate::compiler::native_id::REFLECT_GET => Some(MetaobjectOpKind::ReflectGet),
            crate::compiler::native_id::REFLECT_SET => Some(MetaobjectOpKind::ReflectSet),
            crate::compiler::native_id::REFLECT_HAS => Some(MetaobjectOpKind::ReflectHas),
            crate::compiler::native_id::REFLECT_OWN_KEYS => Some(MetaobjectOpKind::ReflectOwnKeys),
            crate::compiler::native_id::REFLECT_CONSTRUCT => {
                Some(MetaobjectOpKind::ReflectConstruct)
            }
            _ => None,
        }
    }

    fn namespace_call_kind_from_native_id(native_id: u16) -> Option<BuiltinNamespaceCallKind> {
        match native_id {
            crate::compiler::native_id::OBJECT_SAME_VALUE => {
                Some(BuiltinNamespaceCallKind::ObjectSameValue)
            }
            crate::compiler::native_id::JSON_PARSE => Some(BuiltinNamespaceCallKind::JsonParse),
            crate::compiler::native_id::JSON_STRINGIFY => {
                Some(BuiltinNamespaceCallKind::JsonStringify)
            }
            crate::compiler::native_id::OBJECT_STRING_FROM_CHAR_CODE => {
                Some(BuiltinNamespaceCallKind::StringFromCharCode)
            }
            crate::vm::builtin::number::IS_NAN => Some(BuiltinNamespaceCallKind::NumberIsNaN),
            crate::vm::builtin::number::IS_FINITE => {
                Some(BuiltinNamespaceCallKind::NumberIsFinite)
            }
            crate::compiler::native_id::DATE_NOW => Some(BuiltinNamespaceCallKind::DateNow),
            crate::compiler::native_id::DATE_PARSE => Some(BuiltinNamespaceCallKind::DateParse),
            _ => None,
        }
    }

    fn builtin_origin_kind_for_expr(&self, expr: &Expression) -> BuiltinOriginKind {
        let value_origin = self.value_origin_kind_for_expr(expr);
        let ty = self.expr_type(expr);
        let shape = self.object_shape_for_expr(expr);
        let shape_kind = shape.as_ref().map(|shape| shape.kind);
        let type_name = shape
            .as_ref()
            .and_then(|shape| shape.type_name.clone())
            .or_else(|| self.canonical_surface_type_name_for_expr(expr, ty));
        let builtin_global_kind = self
            .builtin_global_name_for_expr(expr)
            .and_then(|name| self.builtin_surface.global_kind(&name));
        match value_origin {
            ValueOriginKind::ImportedBinding => BuiltinOriginKind::ImportedBuiltinBinding,
            ValueOriginKind::ImportedNamespace => BuiltinOriginKind::ImportedBuiltinNamespace,
            ValueOriginKind::BuiltinNamespace => BuiltinOriginKind::BuiltinNamespace,
            ValueOriginKind::BuiltinGlobalValue => {
                if type_name
                    .as_deref()
                    .is_some_and(Self::type_name_is_host_handle)
                {
                    BuiltinOriginKind::HostHandleValue
                } else if matches!(
                    builtin_global_kind,
                    Some(BuiltinGlobalKind::ClassValue | BuiltinGlobalKind::StaticValue)
                ) || matches!(shape_kind, Some(ObjectShapeKind::ClassValue))
                {
                    BuiltinOriginKind::BuiltinClassValue
                } else {
                    BuiltinOriginKind::BuiltinInstance
                }
            }
            _ => {
                if type_name
                    .as_deref()
                    .is_some_and(Self::type_name_is_host_handle)
                {
                    BuiltinOriginKind::HostHandleValue
                } else if type_name
                    .as_deref()
                    .is_some_and(|name| self.builtin_surface.has_dispatch_type(name))
                {
                    if matches!(shape_kind, Some(ObjectShapeKind::ClassValue)) {
                        BuiltinOriginKind::BuiltinClassValue
                    } else {
                        BuiltinOriginKind::BuiltinInstance
                    }
                } else {
                    BuiltinOriginKind::DynamicValue
                }
            }
        }
    }

    fn host_handle_op_for_member(
        &self,
        receiver_expr: &Expression,
        receiver_type_id: Option<TypeId>,
        member_name: &str,
    ) -> Option<HostHandleOpKind> {
        let shape_type_name = self
            .object_shape_for_expr(receiver_expr)
            .and_then(|shape| shape.type_name);
        let raw_type_name = receiver_type_id.and_then(|ty| self.type_name_for_type_id(ty));
        let surface_type_name = shape_type_name
            .clone()
            .or_else(|| self.receiver_builtin_surface_type_name(receiver_type_id))
            .or_else(|| raw_type_name.clone());
        let type_name = surface_type_name?;
        match (type_name.as_str(), member_name) {
            (TypeContext::MUTEX_TYPE_NAME, "lock") => Some(HostHandleOpKind::MutexLock),
            (TypeContext::MUTEX_TYPE_NAME, "unlock") => Some(HostHandleOpKind::MutexUnlock),
            (TypeContext::PROMISE_TYPE_NAME, "cancel") => Some(HostHandleOpKind::TaskCancel),
            (TypeContext::PROMISE_TYPE_NAME, "isDone") => Some(HostHandleOpKind::TaskIsDone),
            (TypeContext::PROMISE_TYPE_NAME, "isCancelled") => {
                Some(HostHandleOpKind::TaskIsCancelled)
            }
            _ => None,
        }
    }

    fn is_builtin_constructor_type_name(&self, name: &str) -> bool {
        self.builtin_surface
            .type_surface(name)
            .is_some_and(|surface| surface.constructor_native_id.is_some())
    }

    fn runtime_late_bound_receiver(&self, expr: &Expression, receiver_ty: Option<TypeId>) -> bool {
        let Some(receiver_ty) = receiver_ty else {
            return false;
        };
        if !self.type_is_nominal_class_type(receiver_ty) {
            return false;
        }
        let Some(type_name) = self.type_name_for_type_id(receiver_ty) else {
            return false;
        };
        if self.has_local_class_named(&type_name) {
            return false;
        }
        if self.builtin_surface.has_dispatch_type(&type_name) {
            return false;
        }
        if matches!(expr, Expression::Identifier(ident) if self.imported_symbols.contains(&ident.name))
        {
            return true;
        }
        !self.is_builtin_namespace_name(&type_name)
    }

    fn call_completion_kind_for_expr(
        &self,
        call: &ast::CallExpression,
        dispatch_kind: CallDispatchKind,
    ) -> CallCompletionKind {
        let callee_ty = self.expr_type(&call.callee);
        let return_ty = self.expr_type(&Expression::Call(call.clone()));
        if callee_ty.is_some_and(|ty| self.type_id_is_async_callable(ty))
            || return_ty.is_some_and(|ty| {
                matches!(
                    self.typed.and_then(|typed| typed.type_ctx.get(ty)),
                    Some(Type::Task(_))
                )
            })
        {
            return CallCompletionKind::Task;
        }
        match dispatch_kind {
            CallDispatchKind::RuntimeLateBoundMethod | CallDispatchKind::DynamicCall => {
                CallCompletionKind::RuntimeClosure
            }
            _ => CallCompletionKind::Sync,
        }
    }

    fn object_shape_for_expr(&self, expr: &Expression) -> Option<SemanticObjectShape> {
        if let Expression::Object(object) = expr {
            let requires_runtime_publication = object.properties.iter().any(|prop| match prop {
                ast::ObjectProperty::Property(property) => {
                    property.kind != ast::PropertyKind::Init
                        || matches!(property.key, ast::PropertyKey::Computed(_))
                }
                ast::ObjectProperty::Spread(_) => true,
            });
            if requires_runtime_publication {
                return Some(SemanticObjectShape {
                    span_start: expr.span().start,
                    span_end: expr.span().end,
                    kind: ObjectShapeKind::Dynamic,
                    type_id: self.expr_type(expr),
                    type_name: self
                        .expr_type(expr)
                        .and_then(|ty| self.type_name_for_type_id(ty)),
                });
            }
        }

        if let Expression::Identifier(ident) = expr {
            if let Some(shape) = self
                .resolve_scope_value_fact(ident.name)
                .and_then(|fact| fact.shape.clone())
            {
                return Some(shape);
            }
        }

        let type_id = self.expr_type(expr)?;
        let binding_kind = self.binding_kind_for_identifier_expr(expr);
        let mut kind = self.object_shape_kind_for_type_id(type_id, binding_kind);
        if let Expression::Identifier(ident) = expr {
            let name = self.interner.resolve(ident.name);
            let builtin_global_kind = self.builtin_surface.global_kind(name);
            if binding_kind == Some(BindingKind::Class)
                || (self.imported_symbols.contains(&ident.name)
                    && !self.imported_namespace_symbols.contains(&ident.name)
                    && (self.type_is_nominal_class_type(type_id)
                        || self.type_has_construct_signatures(type_id)))
                || matches!(
                    builtin_global_kind,
                    Some(BuiltinGlobalKind::ClassValue | BuiltinGlobalKind::StaticValue)
                )
            {
                kind = ObjectShapeKind::ClassValue;
            }
        }
        Some(SemanticObjectShape {
            span_start: expr.span().start,
            span_end: expr.span().end,
            kind,
            type_id: Some(type_id),
            type_name: self.canonical_surface_type_name_for_expr(expr, Some(type_id)),
        })
    }

    fn class_includes_method(&self, class_ty: &ClassType, name: &str) -> bool {
        if class_ty.methods.iter().any(|method| method.name == name) {
            return true;
        }
        let Some(typed) = self.typed else {
            return false;
        };
        class_ty
            .extends
            .is_some_and(|parent| match typed.type_ctx.get(parent) {
                Some(Type::Class(parent_class)) => self.class_includes_method(parent_class, name),
                _ => false,
            })
    }

    fn class_includes_property(&self, class_ty: &ClassType, name: &str) -> bool {
        if class_ty
            .properties
            .iter()
            .any(|property| property.name == name)
        {
            return true;
        }
        let Some(typed) = self.typed else {
            return false;
        };
        class_ty
            .extends
            .is_some_and(|parent| match typed.type_ctx.get(parent) {
                Some(Type::Class(parent_class)) => self.class_includes_property(parent_class, name),
                _ => false,
            })
    }

    fn class_includes_static_method(&self, class_ty: &ClassType, name: &str) -> bool {
        class_ty
            .static_methods
            .iter()
            .any(|method| method.name == name)
    }

    fn class_includes_static_property(&self, class_ty: &ClassType, name: &str) -> bool {
        class_ty
            .static_properties
            .iter()
            .any(|property| property.name == name)
    }

    fn class_includes_accessor(&self, class_ty: &ClassType, name: &str, is_static: bool) -> bool {
        let accessors = if is_static {
            &self.class_static_accessor_names
        } else {
            &self.class_accessor_names
        };
        if accessors
            .get(&class_ty.name)
            .is_some_and(|names| names.contains(name))
        {
            return true;
        }
        let Some(typed) = self.typed else {
            return false;
        };
        class_ty
            .extends
            .is_some_and(|parent| match typed.type_ctx.get(parent) {
                Some(Type::Class(parent_class)) => {
                    self.class_includes_accessor(parent_class, name, is_static)
                }
                _ => false,
            })
    }

    fn structural_includes_slot_type(&self, ty_id: TypeId, name: &str) -> bool {
        let Some(typed) = self.typed else {
            return false;
        };
        match typed.type_ctx.get(ty_id) {
            Some(Type::Object(ObjectType {
                properties,
                call_signatures: _,
                construct_signatures: _,
                index_signature: _,
            })) => properties.iter().any(|property| property.name == name),
            Some(Type::Interface(InterfaceType {
                properties,
                methods,
                extends,
                ..
            })) => {
                properties.iter().any(|property| property.name == name)
                    || methods.iter().any(|method| method.name == name)
                    || extends
                        .iter()
                        .copied()
                        .any(|parent| self.structural_includes_slot_type(parent, name))
            }
            Some(Type::TypeVar(tv)) => tv
                .constraint
                .is_some_and(|constraint| self.structural_includes_slot_type(constraint, name)),
            Some(Type::Reference(reference)) => typed
                .type_ctx
                .lookup_named_type(&reference.name)
                .is_some_and(|resolved| self.structural_includes_slot_type(resolved, name)),
            Some(Type::Generic(generic)) => self.structural_includes_slot_type(generic.base, name),
            Some(Type::Union(union)) => union
                .members
                .iter()
                .copied()
                .any(|member| self.structural_includes_slot_type(member, name)),
            _ => false,
        }
    }

    fn record_object_shape_for_expr(&mut self, expr: &Expression) {
        if let Some(shape) = self.object_shape_for_expr(expr) {
            self.object_shapes.push(shape);
        }
    }

    fn record_member_target_for_expr(&mut self, member: &ast::MemberExpression) {
        let Some(shape) = self.object_shape_for_expr(&member.object) else {
            return;
        };
        let name = self.identifier(&member.property);
        let kind = self.member_target_kind_for_shape_and_name(&shape, &name);

        self.member_targets.push(SemanticMemberTarget {
            span_start: member.span.start,
            span_end: member.span.end,
            kind,
            name: Some(name),
            receiver_type_id: shape.type_id,
            receiver_shape_kind: shape.kind,
        });
    }

    fn record_property_dispatch_for_expr(&mut self, member: &ast::MemberExpression) {
        let name = self.identifier(&member.property);
        let receiver_origin = self.value_origin_kind_for_expr(&member.object);
        let receiver_shape = self.object_shape_for_expr(&member.object);
        let receiver_type_id = receiver_shape.as_ref().and_then(|shape| shape.type_id);
        let has_builtin_static_binding = self
            .builtin_static_property_binding_for_expr(&member.object, &name)
            .or_else(|| self.builtin_static_method_binding_for_expr(&member.object, &name))
            .is_some();
        let member_target_kind = receiver_shape
            .as_ref()
            .map(|shape| self.member_target_kind_for_shape_and_name(shape, &name))
            .unwrap_or(MemberTargetKind::DynamicProperty);
        let kind = match receiver_origin {
            ValueOriginKind::ImportedNamespace => PropertyDispatchKind::ImportedNamespaceExport,
            ValueOriginKind::BuiltinNamespace => PropertyDispatchKind::BuiltinNamespaceProperty,
            ValueOriginKind::BuiltinGlobalValue if has_builtin_static_binding => {
                PropertyDispatchKind::BuiltinNamespaceProperty
            }
            _ => match member_target_kind {
                MemberTargetKind::NominalField => PropertyDispatchKind::NominalField,
                MemberTargetKind::StructuralSlot => PropertyDispatchKind::StructuralSlot,
                MemberTargetKind::DynamicProperty
                    if self.runtime_late_bound_receiver(&member.object, receiver_type_id) =>
                {
                    PropertyDispatchKind::RuntimeLateBoundProperty
                }
                _ => PropertyDispatchKind::DynamicProperty,
            },
        };

        self.property_dispatches.push(SemanticPropertyDispatch {
            span_start: member.span.start,
            span_end: member.span.end,
            kind,
            receiver_origin,
            receiver_type_id,
            property_name: Some(name),
            result_type_id: self.expr_type(&Expression::Member(member.clone())),
        });
    }

    fn record_builtin_dispatch_for_expr(&mut self, expr: &Expression) {
        match expr {
            Expression::Member(member) => {
                let property_name = self.identifier(&member.property);
                let receiver_type_id = self.expr_type(&member.object);
                let origin = self.builtin_origin_kind_for_expr(&member.object);
                let registry_dispatch = self.receiver_builtin_registry_property_dispatch(
                    &member.object,
                    receiver_type_id,
                    &property_name,
                );
                let static_binding = self
                    .builtin_static_property_binding_for_expr(&member.object, &property_name)
                    .or_else(|| {
                        self.builtin_static_method_binding_for_expr(&member.object, &property_name)
                    });
                let dispatch = match origin {
                    BuiltinOriginKind::BuiltinNamespace | BuiltinOriginKind::BuiltinClassValue => static_binding.map(|(export_name, binding)| SemanticBuiltinDispatch {
                            span_start: member.span.start,
                            span_end: member.span.end,
                            origin,
                            kind: BuiltinDispatchKind::NamespaceProperty,
                            metaobject_op: None,
                            iterator_op: None,
                            host_handle_op: None,
                            namespace_call: None,
                            registry_dispatch: None,
                            native_id: match binding {
                                crate::compiler::module::BuiltinDispatchBinding::VmNative {
                                    native_id,
                                    ..
                                } => Some(*native_id),
                                _ => None,
                            },
                            receiver_type_id,
                            callee_type_id: None,
                            result_type_id: self.expr_type(expr),
                            property_name: Some(property_name.clone()),
                            member_name: None,
                            export_name: Some(export_name),
                        }),
                    _ if registry_dispatch.is_some() => Some(SemanticBuiltinDispatch {
                        span_start: member.span.start,
                        span_end: member.span.end,
                        origin,
                        kind: BuiltinDispatchKind::InstanceProperty,
                        metaobject_op: None,
                        iterator_op: None,
                        host_handle_op: None,
                        namespace_call: None,
                        registry_dispatch,
                        native_id: None,
                        receiver_type_id,
                        callee_type_id: None,
                        result_type_id: self.expr_type(expr),
                        property_name: Some(property_name),
                        member_name: None,
                        export_name: None,
                    }),
                    _ => None,
                };
                if let Some(dispatch) = dispatch {
                    self.builtin_dispatches.push(dispatch);
                }
            }
            Expression::Call(call) => {
                let dispatch = match &*call.callee {
                    Expression::Member(member) => {
                        let member_name = self.identifier(&member.property);
                        let receiver_type_id = self.expr_type(&member.object);
                        let origin = self.builtin_origin_kind_for_expr(&member.object);
                        if let Some((export_name, binding)) = self
                            .builtin_static_method_binding_for_expr(&member.object, &member_name)
                        {
                            let native_id = match binding {
                                crate::compiler::module::BuiltinDispatchBinding::VmNative {
                                    native_id,
                                    ..
                                } => Some(*native_id),
                                _ => None,
                            };
                            if let Some(metaobject_op) =
                                native_id.and_then(Self::metaobject_op_from_native_id)
                            {
                                Some(SemanticBuiltinDispatch {
                                    span_start: call.span.start,
                                    span_end: call.span.end,
                                    origin,
                                    kind: BuiltinDispatchKind::MetaobjectOp,
                                    metaobject_op: Some(metaobject_op),
                                    iterator_op: None,
                                    host_handle_op: None,
                                    namespace_call: None,
                                    registry_dispatch: None,
                                    native_id,
                                    receiver_type_id,
                                    callee_type_id: self.expr_type(&call.callee),
                                    result_type_id: self.expr_type(expr),
                                    property_name: None,
                                    member_name: Some(member_name),
                                    export_name: Some(export_name),
                                })
                            } else {
                                Some(SemanticBuiltinDispatch {
                                    span_start: call.span.start,
                                    span_end: call.span.end,
                                    origin,
                                    kind: BuiltinDispatchKind::NamespaceCall,
                                    metaobject_op: None,
                                    iterator_op: None,
                                    host_handle_op: None,
                                    namespace_call: native_id
                                        .and_then(Self::namespace_call_kind_from_native_id),
                                    registry_dispatch: None,
                                    native_id,
                                    receiver_type_id,
                                    callee_type_id: self.expr_type(&call.callee),
                                    result_type_id: self.expr_type(expr),
                                    property_name: None,
                                    member_name: Some(member_name),
                                    export_name: Some(export_name),
                                })
                            }
                        } else if let Some(host_handle_op) = self.host_handle_op_for_member(
                            &member.object,
                            receiver_type_id,
                            &member_name,
                        )
                        {
                            Some(SemanticBuiltinDispatch {
                                span_start: call.span.start,
                                span_end: call.span.end,
                                origin,
                                kind: BuiltinDispatchKind::HostHandleOp,
                                metaobject_op: None,
                                iterator_op: None,
                                host_handle_op: Some(host_handle_op),
                                namespace_call: None,
                                registry_dispatch: None,
                                native_id: None,
                                receiver_type_id,
                                callee_type_id: self.expr_type(&call.callee),
                                result_type_id: self.expr_type(expr),
                                property_name: None,
                                member_name: Some(member_name),
                                export_name: None,
                            })
                        } else {
                            self.receiver_builtin_registry_method_dispatch(
                                &member.object,
                                receiver_type_id,
                                &member_name,
                            )
                            .map(|registry_dispatch| {
                                SemanticBuiltinDispatch {
                                    span_start: call.span.start,
                                    span_end: call.span.end,
                                    origin,
                                    kind: BuiltinDispatchKind::InstanceMethod,
                                    metaobject_op: None,
                                    iterator_op: None,
                                    host_handle_op: None,
                                    namespace_call: None,
                                    native_id: None,
                                    registry_dispatch: Some(registry_dispatch),
                                    receiver_type_id,
                                    callee_type_id: self.expr_type(&call.callee),
                                    result_type_id: self.expr_type(expr),
                                    property_name: None,
                                    member_name: Some(member_name),
                                    export_name: None,
                                }
                            })
                        }
                    }
                    _ => None,
                };
                if let Some(dispatch) = dispatch {
                    self.builtin_dispatches.push(dispatch);
                }
            }
            Expression::New(new_expr) => {
                let constructor_dispatch = self
                    .constructor_dispatches
                    .iter()
                    .find(|dispatch| {
                        dispatch.span_start == new_expr.span.start
                            && dispatch.span_end == new_expr.span.end
                    })
                    .cloned();
                let callee_origin = self.builtin_origin_kind_for_expr(&new_expr.callee);
                let callee_name = match &*new_expr.callee {
                    Expression::Identifier(ident) => {
                        Some(self.interner.resolve(ident.name).to_string())
                    }
                    _ => None,
                };
                let dispatch = match constructor_dispatch.map(|dispatch| dispatch.kind) {
                    Some(ConstructorDispatchKind::BuiltinNativeConstructor) => {
                        let host_handle_op = match callee_name.as_deref() {
                            Some(TypeContext::CHANNEL_TYPE_NAME) => {
                                Some(HostHandleOpKind::ChannelConstructor)
                            }
                            Some(TypeContext::MUTEX_TYPE_NAME) => {
                                Some(HostHandleOpKind::MutexConstructor)
                            }
                            _ => None,
                        };
                        Some(SemanticBuiltinDispatch {
                            span_start: new_expr.span.start,
                            span_end: new_expr.span.end,
                            origin: callee_origin,
                            kind: BuiltinDispatchKind::HostHandleOp,
                            metaobject_op: None,
                            iterator_op: None,
                            host_handle_op,
                            namespace_call: None,
                            registry_dispatch: None,
                            native_id: None,
                            receiver_type_id: None,
                            callee_type_id: self.expr_type(&new_expr.callee),
                            result_type_id: self.expr_type(expr),
                            property_name: None,
                            member_name: None,
                            export_name: callee_name,
                        })
                    }
                    Some(ConstructorDispatchKind::NominalClass)
                        if matches!(
                            callee_origin,
                            BuiltinOriginKind::BuiltinClassValue
                                | BuiltinOriginKind::ImportedBuiltinBinding
                        ) =>
                    {
                        Some(SemanticBuiltinDispatch {
                            span_start: new_expr.span.start,
                            span_end: new_expr.span.end,
                            origin: callee_origin,
                            kind: BuiltinDispatchKind::Constructor,
                            metaobject_op: None,
                            iterator_op: None,
                            host_handle_op: None,
                            namespace_call: None,
                            registry_dispatch: None,
                            native_id: None,
                            receiver_type_id: None,
                            callee_type_id: self.expr_type(&new_expr.callee),
                            result_type_id: self.expr_type(expr),
                            property_name: None,
                            member_name: None,
                            export_name: callee_name,
                        })
                    }
                    Some(ConstructorDispatchKind::ImportedConstructorValue) => {
                        Some(SemanticBuiltinDispatch {
                            span_start: new_expr.span.start,
                            span_end: new_expr.span.end,
                            origin: BuiltinOriginKind::ImportedBuiltinBinding,
                            kind: BuiltinDispatchKind::Constructor,
                            metaobject_op: None,
                            iterator_op: None,
                            host_handle_op: None,
                            namespace_call: None,
                            registry_dispatch: None,
                            native_id: None,
                            receiver_type_id: None,
                            callee_type_id: self.expr_type(&new_expr.callee),
                            result_type_id: self.expr_type(expr),
                            property_name: None,
                            member_name: None,
                            export_name: callee_name,
                        })
                    }
                    _ => None,
                };
                if let Some(dispatch) = dispatch {
                    self.builtin_dispatches.push(dispatch);
                }
            }
            _ => {}
        }
    }

    fn record_call_target_for_expr(&mut self, call: &ast::CallExpression) {
        let (kind, receiver_type_id, member_name) = match &*call.callee {
            Expression::Member(member) => {
                let name = self.identifier(&member.property);
                match self.object_shape_for_expr(&member.object) {
                    Some(shape) => {
                        match self.member_target_kind_for_shape_and_name(&shape, &name) {
                            MemberTargetKind::NominalMethod => {
                                (CallTargetKind::NominalMethod, shape.type_id, Some(name))
                            }
                            MemberTargetKind::StaticMethod => {
                                (CallTargetKind::StaticMethod, shape.type_id, Some(name))
                            }
                            MemberTargetKind::StructuralSlot => {
                                (CallTargetKind::StructuralCall, shape.type_id, Some(name))
                            }
                            MemberTargetKind::BuiltinProperty
                                if matches!(
                                    shape.kind,
                                    ObjectShapeKind::BuiltinValue | ObjectShapeKind::ClassValue
                                ) =>
                            {
                                (CallTargetKind::NominalMethod, shape.type_id, Some(name))
                            }
                            _ => (CallTargetKind::DynamicCall, shape.type_id, Some(name)),
                        }
                    }
                    None => (CallTargetKind::DynamicCall, None, Some(name)),
                }
            }
            Expression::Identifier(ident) => {
                if let Some(bound_method) = self
                    .resolve_scope_value_fact(ident.name)
                    .and_then(|fact| fact.bound_method.clone())
                {
                    (
                        bound_method.kind,
                        bound_method.receiver_type_id,
                        bound_method.member_name,
                    )
                } else {
                    let callee_ty = self.expr_type(&call.callee);
                    let callee_shape = self.object_shape_for_expr(&call.callee);
                    if callee_shape
                        .as_ref()
                        .is_some_and(|shape| shape.kind == ObjectShapeKind::ClassValue)
                    {
                        (CallTargetKind::ConstructorLikeValue, None, None)
                    } else if callee_ty.is_some_and(|ty| self.type_is_callable_type(ty)) {
                        (CallTargetKind::PlainFunction, None, None)
                    } else {
                        (CallTargetKind::DynamicCall, None, None)
                    }
                }
            }
            _ => {
                let callee_ty = self.expr_type(&call.callee);
                (
                    callee_ty
                        .filter(|&ty| self.type_is_callable_type(ty))
                        .map(|_| CallTargetKind::PlainFunction)
                        .unwrap_or(CallTargetKind::DynamicCall),
                    None,
                    None,
                )
            }
        };

        self.call_targets.push(SemanticCallTarget {
            span_start: call.span.start,
            span_end: call.span.end,
            kind,
            receiver_type_id,
            member_name,
            return_type_id: self.expr_type(&Expression::Call(call.clone())),
            return_shape: None,
        });
    }

    fn record_call_dispatch_for_expr(&mut self, call: &ast::CallExpression) {
        let callee_origin = self.value_origin_kind_for_expr(&call.callee);
        let (kind, receiver_origin, receiver_type_id, member_name) = match &*call.callee {
            Expression::Member(member) => {
                let member_name = self.identifier(&member.property);
                let receiver_origin = self.value_origin_kind_for_expr(&member.object);
                let receiver_shape = self.object_shape_for_expr(&member.object);
                let receiver_type_id = receiver_shape.as_ref().and_then(|shape| shape.type_id);
                let has_builtin_static_method = self
                    .builtin_static_method_binding_for_expr(&member.object, &member_name)
                    .is_some();
                let member_target_kind = receiver_shape
                    .as_ref()
                    .map(|shape| self.member_target_kind_for_shape_and_name(shape, &member_name))
                    .unwrap_or(MemberTargetKind::DynamicProperty);

                let dispatch_kind = match receiver_origin {
                    ValueOriginKind::ImportedNamespace => CallDispatchKind::ImportedNamespaceCall,
                    ValueOriginKind::BuiltinNamespace => CallDispatchKind::BuiltinNamespaceMethod,
                    ValueOriginKind::BuiltinGlobalValue if has_builtin_static_method => {
                        CallDispatchKind::BuiltinNamespaceMethod
                    }
                    _ => match member_target_kind {
                        MemberTargetKind::NominalMethod => CallDispatchKind::NominalMethod,
                        MemberTargetKind::StaticMethod => CallDispatchKind::StaticMethod,
                        MemberTargetKind::StructuralSlot => CallDispatchKind::StructuralCall,
                        MemberTargetKind::BuiltinProperty
                            if self.receiver_has_builtin_registry_method(
                                &member.object,
                                receiver_type_id,
                                &member_name,
                            ) =>
                        {
                            CallDispatchKind::BuiltinInstanceMethod
                        }
                        MemberTargetKind::DynamicProperty
                            if self
                                .runtime_late_bound_receiver(&member.object, receiver_type_id) =>
                        {
                            CallDispatchKind::RuntimeLateBoundMethod
                        }
                        _ => CallDispatchKind::DynamicCall,
                    },
                };

                (
                    dispatch_kind,
                    Some(receiver_origin),
                    receiver_type_id,
                    Some(member_name),
                )
            }
            Expression::Identifier(ident) => {
                if let Some(bound_method) = self
                    .resolve_scope_value_fact(ident.name)
                    .and_then(|fact| fact.bound_method.clone())
                {
                    let kind = match bound_method.kind {
                        CallTargetKind::NominalMethod => CallDispatchKind::NominalMethod,
                        CallTargetKind::StaticMethod => CallDispatchKind::StaticMethod,
                        CallTargetKind::StructuralCall => CallDispatchKind::StructuralCall,
                        _ => CallDispatchKind::DynamicCall,
                    };
                    (
                        kind,
                        None,
                        bound_method.receiver_type_id,
                        bound_method.member_name,
                    )
                } else {
                    let callee_ty = self.expr_type(&call.callee);
                    let dispatch_kind =
                        if callee_ty.is_some_and(|ty| self.type_is_callable_type(ty)) {
                            CallDispatchKind::PlainFunction
                        } else {
                            CallDispatchKind::DynamicCall
                        };
                    (dispatch_kind, None, None, None)
                }
            }
            _ => {
                let callee_ty = self.expr_type(&call.callee);
                let dispatch_kind = if callee_ty.is_some_and(|ty| self.type_is_callable_type(ty)) {
                    CallDispatchKind::PlainFunction
                } else {
                    CallDispatchKind::DynamicCall
                };
                (dispatch_kind, None, None, None)
            }
        };

        self.call_dispatches.push(SemanticCallDispatch {
            span_start: call.span.start,
            span_end: call.span.end,
            kind,
            callee_origin,
            receiver_origin,
            receiver_type_id,
            member_name,
            result_type_id: self.expr_type(&Expression::Call(call.clone())),
            completion_kind: self.call_completion_kind_for_expr(call, kind),
        });
    }

    fn record_constructor_target_for_expr(&mut self, new_expr: &ast::NewExpression) {
        let callee_type_id = self.expr_type(&new_expr.callee);
        let kind = match &*new_expr.callee {
            Expression::Identifier(ident)
                if self
                    .resolve_scope_binding(ident.name)
                    .is_some_and(|binding| binding.kind == BindingKind::Class) =>
            {
                ConstructorTargetKind::NominalClass
            }
            _ => callee_type_id
                .map(|ty| {
                    if self.type_is_nominal_class_type(ty) {
                        ConstructorTargetKind::NominalClass
                    } else if self.type_has_construct_signatures(ty) {
                        ConstructorTargetKind::ConstructorLikeValue
                    } else {
                        ConstructorTargetKind::DynamicConstructor
                    }
                })
                .unwrap_or(ConstructorTargetKind::DynamicConstructor),
        };

        self.constructor_targets.push(SemanticConstructorTarget {
            span_start: new_expr.span.start,
            span_end: new_expr.span.end,
            kind,
            instance_shape: None,
            callee_type_id,
        });
    }

    fn record_constructor_dispatch_for_expr(&mut self, new_expr: &ast::NewExpression) {
        let callee_type_id = self.expr_type(&new_expr.callee);
        let callee_shape = self.object_shape_for_expr(&new_expr.callee);
        let callee_origin = self.value_origin_kind_for_expr(&new_expr.callee);
        let callee_name = self.builtin_global_name_for_expr(&new_expr.callee);
        let kind = match &*new_expr.callee {
            Expression::Identifier(ident)
                if self
                    .resolve_scope_binding(ident.name)
                    .is_some_and(|binding| binding.kind == BindingKind::Class) =>
            {
                ConstructorDispatchKind::NominalClass
            }
            Expression::Identifier(_) => match callee_origin {
                ValueOriginKind::ImportedBinding => {
                    ConstructorDispatchKind::ImportedConstructorValue
                }
                ValueOriginKind::BuiltinGlobalValue
                    if callee_name.as_deref().is_some_and(|name| {
                        matches!(
                            name,
                            TypeContext::CHANNEL_TYPE_NAME | TypeContext::MUTEX_TYPE_NAME
                        )
                    }) =>
                {
                    ConstructorDispatchKind::BuiltinNativeConstructor
                }
                ValueOriginKind::BuiltinGlobalValue
                    if callee_type_id.is_some_and(|ty| {
                        self.type_is_nominal_class_type(ty)
                            || self.type_has_construct_signatures(ty)
                    }) =>
                {
                    ConstructorDispatchKind::NominalClass
                }
                ValueOriginKind::RuntimeLateBoundValue
                    if callee_type_id.is_some_and(|ty| self.type_has_construct_signatures(ty)) =>
                {
                    ConstructorDispatchKind::RuntimeConstructorValue
                }
                _ if callee_shape
                    .as_ref()
                    .is_some_and(|shape| shape.kind == ObjectShapeKind::ClassValue) =>
                {
                    ConstructorDispatchKind::RuntimeConstructorValue
                }
                _ if callee_type_id.is_some_and(|ty| {
                    self.type_has_construct_signatures(ty) || self.type_is_nominal_class_type(ty)
                }) =>
                {
                    ConstructorDispatchKind::RuntimeConstructorValue
                }
                _ => ConstructorDispatchKind::DynamicConstructor,
            },
            _ => callee_type_id
                .map(|ty| {
                    if self.type_is_nominal_class_type(ty)
                        && self
                            .type_name_for_type_id(ty)
                            .is_some_and(|name| self.has_local_class_named(&name))
                    {
                        ConstructorDispatchKind::NominalClass
                    } else if self.type_has_construct_signatures(ty) {
                        ConstructorDispatchKind::RuntimeConstructorValue
                    } else {
                        ConstructorDispatchKind::DynamicConstructor
                    }
                })
                .unwrap_or(ConstructorDispatchKind::DynamicConstructor),
        };

        self.constructor_dispatches
            .push(SemanticConstructorDispatch {
                span_start: new_expr.span.start,
                span_end: new_expr.span.end,
                kind,
                callee_origin,
                callee_type_id,
                result_type_id: self.expr_type(&Expression::New(new_expr.clone())),
            });
    }

    fn bound_method_info_for_expr(&self, expr: &Expression) -> Option<SemanticBoundMethodInfo> {
        match expr {
            Expression::Identifier(ident) => self
                .resolve_scope_value_fact(ident.name)
                .and_then(|fact| fact.bound_method.clone()),
            Expression::Parenthesized(paren) => self.bound_method_info_for_expr(&paren.expression),
            Expression::Member(member) => {
                let receiver_shape = self.object_shape_for_expr(&member.object)?;
                let member_name = self.identifier(&member.property);
                let kind = match self
                    .member_target_kind_for_shape_and_name(&receiver_shape, &member_name)
                {
                    MemberTargetKind::NominalMethod | MemberTargetKind::BuiltinProperty => {
                        CallTargetKind::NominalMethod
                    }
                    MemberTargetKind::StaticMethod => CallTargetKind::StaticMethod,
                    _ => return None,
                };
                Some(SemanticBoundMethodInfo {
                    kind,
                    receiver_type_id: receiver_shape.type_id,
                    member_name: Some(member_name),
                })
            }
            Expression::TypeCast(cast) => self.bound_method_info_for_expr(&cast.object),
            _ => None,
        }
    }

    fn value_fact_for_expr(&self, expr: &Expression) -> Option<ScopeValueFact> {
        let shape = match expr {
            Expression::Identifier(ident) => self
                .resolve_scope_value_fact(ident.name)
                .and_then(|fact| fact.shape.clone())
                .or_else(|| self.object_shape_for_expr(expr)),
            Expression::Parenthesized(paren) => self
                .value_fact_for_expr(&paren.expression)
                .and_then(|fact| fact.shape),
            _ => self.object_shape_for_expr(expr),
        };
        let bound_method = self.bound_method_info_for_expr(expr);
        if shape.is_none() && bound_method.is_none() {
            None
        } else {
            Some(ScopeValueFact {
                shape,
                bound_method,
            })
        }
    }

    fn apply_value_fact_to_pattern(&mut self, pattern: &Pattern, expr: Option<&Expression>) {
        match pattern {
            Pattern::Identifier(ident) => {
                let fact = expr.and_then(|expr| self.value_fact_for_expr(expr));
                self.set_scope_value_fact(ident.name, fact);
            }
            Pattern::Array(_) | Pattern::Object(_) | Pattern::Rest(_) => {
                self.clear_pattern_value_facts(pattern)
            }
        }
    }

    fn clear_assignment_target_value_facts(&mut self, expr: &Expression) {
        match expr {
            Expression::Identifier(ident) => self.set_scope_value_fact(ident.name, None),
            Expression::Parenthesized(paren) => {
                self.clear_assignment_target_value_facts(&paren.expression)
            }
            _ => {}
        }
    }

    fn apply_value_fact_to_assignment_target(
        &mut self,
        expr: &Expression,
        value_expr: Option<&Expression>,
    ) {
        match expr {
            Expression::Identifier(ident) => {
                let fact = value_expr.and_then(|expr| self.value_fact_for_expr(expr));
                self.set_scope_value_fact(ident.name, fact);
            }
            Expression::Parenthesized(paren) => {
                self.apply_value_fact_to_assignment_target(&paren.expression, value_expr)
            }
            _ => {}
        }
    }

    fn member_target_kind_for_shape_and_name(
        &self,
        shape: &SemanticObjectShape,
        name: &str,
    ) -> MemberTargetKind {
        if shape.kind == ObjectShapeKind::Dynamic {
            return MemberTargetKind::DynamicProperty;
        }

        let Some(receiver_ty) = shape.type_id else {
            return MemberTargetKind::DynamicProperty;
        };
        let Some(typed) = self.typed else {
            return MemberTargetKind::DynamicProperty;
        };

        let effective_receiver_ty = match typed.type_ctx.get(receiver_ty) {
            Some(Type::Reference(reference)) => typed
                .type_ctx
                .lookup_named_type(&reference.name)
                .unwrap_or(receiver_ty),
            Some(Type::Generic(generic)) => generic.base,
            Some(Type::TypeVar(tv)) => tv.constraint.unwrap_or(receiver_ty),
            _ => receiver_ty,
        };

        let wrapper_surface_member = self
            .receiver_builtin_surface_type_name(Some(effective_receiver_ty))
            .and_then(|type_name| self.builtin_surface.type_surface(&type_name))
            .is_some_and(|surface| {
                surface.wrapper_method_surface
                    && (surface.instance_methods.contains_key(name)
                        || surface.instance_properties.contains_key(name))
            });

        match typed.type_ctx.get(effective_receiver_ty) {
            Some(Type::Class(class_ty)) => match shape.kind {
                ObjectShapeKind::ClassValue => {
                    if self.class_includes_accessor(class_ty, name, true) {
                        MemberTargetKind::DynamicProperty
                    } else if self.class_includes_static_method(class_ty, name) {
                        MemberTargetKind::StaticMethod
                    } else if self.class_includes_static_property(class_ty, name) {
                        MemberTargetKind::BuiltinProperty
                    } else {
                        MemberTargetKind::DynamicProperty
                    }
                }
                ObjectShapeKind::NominalInstance => {
                    if wrapper_surface_member {
                        MemberTargetKind::BuiltinProperty
                    } else if self.class_includes_accessor(class_ty, name, false) {
                        MemberTargetKind::DynamicProperty
                    } else if self.class_includes_method(class_ty, name) {
                        MemberTargetKind::NominalMethod
                    } else if self.class_includes_property(class_ty, name) {
                        MemberTargetKind::NominalField
                    } else {
                        MemberTargetKind::DynamicProperty
                    }
                }
                _ => MemberTargetKind::DynamicProperty,
            },
            Some(
                Type::Object(_)
                | Type::Interface(_)
                | Type::TypeVar(_)
                | Type::Reference(_)
                | Type::Generic(_)
                | Type::Union(_),
            ) => {
                if shape.kind == ObjectShapeKind::StructuralObject
                    && self.structural_includes_slot_type(effective_receiver_ty, name)
                {
                    MemberTargetKind::StructuralSlot
                } else {
                    MemberTargetKind::DynamicProperty
                }
            }
            Some(
                Type::Array(_)
                | Type::Task(_)
                | Type::RegExp
                | Type::Map(_)
                | Type::Set(_)
                | Type::Date
                | Type::Buffer
                | Type::Mutex
                | Type::Channel(_)
                | Type::Tuple(_)
                | Type::Function(_)
                | Type::Primitive(PrimitiveType::String)
                | Type::Primitive(PrimitiveType::Number)
                | Type::Primitive(PrimitiveType::Boolean),
            ) => MemberTargetKind::BuiltinProperty,
            _ => MemberTargetKind::DynamicProperty,
        }
    }

    fn type_is_nominal_class_type(&self, ty_id: TypeId) -> bool {
        let Some(typed) = self.typed else {
            return false;
        };
        match typed.type_ctx.get(ty_id) {
            Some(Type::Class(_)) => true,
            Some(Type::Reference(reference)) => typed
                .type_ctx
                .lookup_named_type(&reference.name)
                .is_some_and(|resolved| self.type_is_nominal_class_type(resolved)),
            Some(Type::Generic(generic)) => self.type_is_nominal_class_type(generic.base),
            Some(Type::TypeVar(tv)) => tv
                .constraint
                .is_some_and(|constraint| self.type_is_nominal_class_type(constraint)),
            _ => false,
        }
    }

    fn type_has_construct_signatures(&self, ty_id: TypeId) -> bool {
        let Some(typed) = self.typed else {
            return false;
        };
        match typed.type_ctx.get(ty_id) {
            Some(Type::Class(class_ty)) => !class_ty.is_abstract,
            Some(Type::Function(_)) => true,
            Some(Type::Object(obj)) => !obj.construct_signatures.is_empty(),
            Some(Type::Interface(interface_ty)) => !interface_ty.construct_signatures.is_empty(),
            Some(Type::Reference(reference)) => typed
                .type_ctx
                .lookup_named_type(&reference.name)
                .is_some_and(|resolved| self.type_has_construct_signatures(resolved)),
            Some(Type::Generic(generic)) => self.type_has_construct_signatures(generic.base),
            Some(Type::TypeVar(tv)) => tv
                .constraint
                .is_some_and(|constraint| self.type_has_construct_signatures(constraint)),
            Some(Type::Union(union)) => union
                .members
                .iter()
                .copied()
                .any(|member| self.type_has_construct_signatures(member)),
            _ => false,
        }
    }

    fn type_is_callable_type(&self, ty_id: TypeId) -> bool {
        let Some(typed) = self.typed else {
            return false;
        };
        match typed.type_ctx.get(ty_id) {
            Some(Type::Function(_)) => true,
            Some(Type::Object(obj)) => !obj.call_signatures.is_empty(),
            Some(Type::Interface(interface_ty)) => !interface_ty.call_signatures.is_empty(),
            Some(Type::Reference(reference)) => typed
                .type_ctx
                .lookup_named_type(&reference.name)
                .is_some_and(|resolved| self.type_is_callable_type(resolved)),
            Some(Type::Generic(generic)) => self.type_is_callable_type(generic.base),
            Some(Type::TypeVar(tv)) => tv
                .constraint
                .is_some_and(|constraint| self.type_is_callable_type(constraint)),
            Some(Type::Union(union)) => union
                .members
                .iter()
                .copied()
                .any(|member| self.type_is_callable_type(member)),
            _ => false,
        }
    }

    fn type_id_is_async_callable(&self, ty_id: TypeId) -> bool {
        let Some(typed) = self.typed else {
            return false;
        };
        match typed.type_ctx.get(ty_id) {
            Some(Type::Function(func)) => {
                func.is_async && self.type_id_returns_task_like(func.return_type)
            }
            Some(Type::Object(obj)) => obj
                .call_signatures
                .iter()
                .copied()
                .any(|sig| self.type_id_is_async_callable(sig)),
            Some(Type::Interface(iface)) => iface
                .call_signatures
                .iter()
                .copied()
                .any(|sig| self.type_id_is_async_callable(sig)),
            Some(Type::Reference(reference)) => typed
                .type_ctx
                .lookup_named_type(&reference.name)
                .is_some_and(|resolved| self.type_id_is_async_callable(resolved)),
            Some(Type::TypeVar(tv)) => tv
                .constraint
                .is_some_and(|constraint| self.type_id_is_async_callable(constraint)),
            Some(Type::Union(union)) => union
                .members
                .iter()
                .copied()
                .any(|member| self.type_id_is_async_callable(member)),
            Some(Type::Generic(generic)) => self.type_id_is_async_callable(generic.base),
            _ => false,
        }
    }

    fn type_id_returns_task_like(&self, ty_id: TypeId) -> bool {
        let Some(typed) = self.typed else {
            return false;
        };
        match typed.type_ctx.get(ty_id) {
            Some(Type::Task(_)) => true,
            Some(Type::Reference(reference)) => typed
                .type_ctx
                .lookup_named_type(&reference.name)
                .is_some_and(|resolved| self.type_id_returns_task_like(resolved)),
            Some(Type::TypeVar(tv)) => tv
                .constraint
                .is_some_and(|constraint| self.type_id_returns_task_like(constraint)),
            Some(Type::Union(union)) => union
                .members
                .iter()
                .copied()
                .any(|member| self.type_id_returns_task_like(member)),
            Some(Type::Generic(generic)) => self.type_id_returns_task_like(generic.base),
            _ => false,
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
            Pattern::Identifier(id) => {
                self.record_binding_with_runtime_env(id.name, kind, runtime_env)
            }
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
            Pattern::Rest(rest) => {
                self.record_pattern_with_runtime_env(&rest.argument, kind, runtime_env)
            }
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

    fn record_scope_snapshot(&mut self, span_start: usize, span_end: usize) {
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
        self.scope_snapshots_by_span
            .insert(span_key(span_start, span_end), bindings);
    }

    fn pattern_has_computed_keys(pattern: &Pattern) -> bool {
        match pattern {
            Pattern::Identifier(_) => false,
            Pattern::Array(arr) => {
                arr.elements
                    .iter()
                    .flatten()
                    .any(|elem| Self::pattern_has_computed_keys(&elem.pattern))
                    || arr
                        .rest
                        .as_deref()
                        .is_some_and(Self::pattern_has_computed_keys)
            }
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
                span_end: ident.span.end,
                kind: ReferenceExprKind::Identifier,
                name: Some(self.identifier(ident)),
            },
            Expression::Member(member) => SemanticReferenceExpr {
                span_start: member.span.start,
                span_end: member.span.end,
                kind: if matches!(&*member.object, Expression::Super(_)) {
                    ReferenceExprKind::SuperNamed
                } else {
                    ReferenceExprKind::PropertyNamed
                },
                name: Some(self.identifier(&member.property)),
            },
            Expression::Index(index) => SemanticReferenceExpr {
                span_start: index.span.start,
                span_end: index.span.end,
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
            span_end: call.span.end,
            kind,
            callee_span_start: call.callee.span().start,
        });
        if kind == CallOpKind::DirectEval {
            self.record_scope_snapshot(call.span.start, call.span.end);
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
            self.record_scope_snapshot(span_start, span_start);
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
                let class_name = self.identifier(&class_decl.name);
                for member in &class_decl.members {
                    match member {
                        ast::ClassMember::Method(method) => {
                            if matches!(
                                method.kind,
                                ast::MethodKind::Getter | ast::MethodKind::Setter
                            ) {
                                if let Some(member_name) = self.property_key_name(&method.name) {
                                    let accessors = if method.is_static {
                                        &mut self.class_static_accessor_names
                                    } else {
                                        &mut self.class_accessor_names
                                    };
                                    accessors
                                        .entry(class_name.clone())
                                        .or_default()
                                        .insert(member_name);
                                }
                            }
                            self.visit_method(method);
                        }
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
                self.record_pattern_with_runtime_env(&var_decl.pattern, kind, self.with_depth > 0);
                self.binding_ops.push(SemanticBindingOp {
                    span_start: var_decl.span.start,
                    kind: BindingOpKind::Initialize,
                    name: None,
                    reference_span_start: Some(var_decl.pattern.span().start),
                });
                if let Some(init) = &var_decl.initializer {
                    self.visit_expr(init);
                }
                self.apply_value_fact_to_pattern(&var_decl.pattern, var_decl.initializer.as_ref());
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
            Statement::ImportDecl(import) => {
                for specifier in &import.specifiers {
                    let local_name = match specifier {
                        ast::ImportSpecifier::Named { name, alias } => {
                            alias.as_ref().map_or(name.name, |alias| alias.name)
                        }
                        ast::ImportSpecifier::Namespace(alias) => alias.name,
                        ast::ImportSpecifier::Default(local) => local.name,
                    };
                    self.imported_symbols.insert(local_name);
                    if matches!(specifier, ast::ImportSpecifier::Namespace(_)) {
                        self.imported_namespace_symbols.insert(local_name);
                    }
                }
            }
            Statement::Empty(_)
            | Statement::Break(_)
            | Statement::Continue(_)
            | Statement::Debugger(_)
            | Statement::TypeAliasDecl(_) => {}
        }
    }

    fn visit_expr(&mut self, expr: &Expression) {
        self.record_object_shape_for_expr(expr);
        match expr {
            Expression::Call(call) => {
                self.record_call_op(call);
                self.record_call_target_for_expr(call);
                self.record_call_dispatch_for_expr(call);
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
                let callable_kind = match (func.is_method, func.is_async, func.is_generator) {
                    (false, false, false) => CallableKind::SyncFunction,
                    (false, true, false) => CallableKind::AsyncFunction,
                    (false, false, true) => CallableKind::GeneratorFunction,
                    (false, true, true) => CallableKind::AsyncGeneratorFunction,
                    (true, false, false) => CallableKind::SyncMethod,
                    (true, true, false) => CallableKind::AsyncMethod,
                    (true, false, true) => CallableKind::GeneratorMethod,
                    (true, true, true) => CallableKind::AsyncGeneratorMethod,
                };
                self.visit_function_like(
                    func.name.as_ref().map(|ident| self.identifier(ident)),
                    func.is_async,
                    func.is_generator,
                    &func.params,
                    Some(&func.body),
                    func.span.start,
                    Some(callable_kind),
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
                self.record_member_target_for_expr(member);
                self.record_property_dispatch_for_expr(member);
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
                    span_end: new_expr.span.end,
                    kind: CallOpKind::Constructor,
                    callee_span_start: new_expr.callee.span().start,
                });
                self.record_constructor_target_for_expr(new_expr);
                self.record_constructor_dispatch_for_expr(new_expr);
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
                if matches!(assign.operator, AssignmentOperator::Assign) {
                    self.apply_value_fact_to_assignment_target(&assign.left, Some(&assign.right));
                } else {
                    self.clear_assignment_target_value_facts(&assign.left);
                }
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
            Expression::IntLiteral(_)
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
        self.record_value_origin_for_expr(expr);
        self.record_builtin_dispatch_for_expr(expr);
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

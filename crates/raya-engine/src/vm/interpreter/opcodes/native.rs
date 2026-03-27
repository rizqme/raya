//! Native call opcode handlers: NativeCall, ModuleNativeCall
//!
//! NativeCall dispatches to built-in operations (channel, buffer, map, set, date, regexp, etc.)
//! and reflect/runtime methods. ModuleNativeCall dispatches through the resolved natives table.

use crate::compiler::native_id::{
    CHANNEL_CAPACITY, CHANNEL_CLOSE, CHANNEL_IS_CLOSED, CHANNEL_LENGTH, CHANNEL_NEW,
    CHANNEL_RECEIVE, CHANNEL_SEND, CHANNEL_TRY_RECEIVE, CHANNEL_TRY_SEND,
};
use crate::compiler::{Compiler, Module, Opcode};
use crate::parser::ast::visitor::{walk_module, Visitor};
use crate::parser::ast::{
    ArrowFunction, ClassDecl, Expression, FunctionDecl, FunctionExpression, Pattern, Statement,
    VariableDecl,
};
use crate::parser::checker::{
    check_early_errors, check_early_errors_with_options, Binder, CheckerPolicy, EarlyErrorOptions,
    ScopeId, TypeChecker, TypeSystemMode,
};
use crate::parser::{Parser, TypeContext};
use crate::vm::builtin::{buffer, date, map, mutex, regexp, set, url};
use crate::vm::gc::header_ptr_from_value_ptr;
use crate::vm::interpreter::execution::{ExecutionResult, OpcodeResult, ReturnAction};
use crate::vm::interpreter::Interpreter;
use crate::vm::interpreter::{PromiseHandle, PromiseMicrotask};
use crate::vm::object::{
    layout_id_from_ordered_names, ArgumentsDataProperty, ArgumentsIndexedProperty,
    ArgumentsObjectData, Array, Buffer, CallableKind, ChannelObject, Class, DateObject, DynProp,
    ExoticKind, GeneratorSnapshotData, GeneratorStateData, LayoutId, MapObject, Object, RayaBigInt,
    RayaString, RefCell, RegExpObject, SetObject, SlotMeta, TypeHandle,
};
use crate::vm::scheduler::{PromiseReaction, PromiseReactionKind, Task, TaskId, TaskState};
use crate::vm::stack::Stack;
use crate::vm::sync::MutexId;
use crate::vm::value::Value;
use crate::vm::VmError;
use num_bigint::BigInt as ArbitraryBigInt;
use rustc_hash::FxHashSet;
use std::any::TypeId;
use std::collections::BTreeSet;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub(crate) const NON_OBJECT_DYNAMIC_VALUE_METADATA_KEY: &str = "__dynamic_value_property";
pub(crate) const NON_OBJECT_DESCRIPTOR_METADATA_KEY: &str = "__dynamic_descriptor_property";
const CALLABLE_VIRTUAL_VALUE_METADATA_KEY: &str = "__callable_virtual_value";
const CALLABLE_VIRTUAL_DELETED_METADATA_KEY: &str = "__callable_virtual_deleted";
const FIXED_PROPERTY_DELETED_METADATA_KEY: &str = "__fixed_property_deleted";
const OBJECT_PROTOTYPE_OVERRIDE_METADATA_KEY: &str = "__object_prototype_override__";
const OBJECT_EXTENSIBLE_METADATA_KEY: &str = "__object_extensible__";
const FIELD_PRESENT_MASK_KEY: &str = "__field_present_mask__";
const DIRECT_EVAL_OUTER_ENV_KEY: &str = "__direct_eval_outer_env__";
const DIRECT_EVAL_COMPLETION_KEY: &str = "__direct_eval_completion__";
const DIRECT_EVAL_LEXICAL_MARKER_PREFIX: &str = "__direct_eval_lexical__:";
const DIRECT_EVAL_UNINITIALIZED_MARKER_PREFIX: &str = "__direct_eval_uninitialized__:";
const DIRECT_EVAL_OUTER_SNAPSHOT_MARKER_PREFIX: &str = "__direct_eval_outer_snapshot__:";
const DIRECT_EVAL_UNINITIALIZED_PREFIX: &str = "__direct_eval_uninitialized__:";
const WITH_ENV_TARGET_KEY: &str = "__with_env_target__";
const SUPER_THIS_INITIALIZED_KEY: &str = "__super_this_initialized__";
static DYNAMIC_JS_FUNCTION_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Map descriptor field names to bit positions for the presence bitmask.
fn descriptor_field_bit(field_name: &str) -> u32 {
    match field_name {
        "value" => 1 << 0,
        "writable" => 1 << 1,
        "enumerable" => 1 << 2,
        "configurable" => 1 << 3,
        "get" => 1 << 4,
        "set" => 1 << 5,
        _ => 0,
    }
}

#[derive(Clone, Copy)]
struct JsPropertyDescriptorRecord {
    has_value: bool,
    value: Value,
    has_writable: bool,
    writable: bool,
    has_configurable: bool,
    configurable: bool,
    has_enumerable: bool,
    enumerable: bool,
    has_get: bool,
    get: Value,
    has_set: bool,
    set: Value,
}

impl Default for JsPropertyDescriptorRecord {
    fn default() -> Self {
        Self {
            has_value: false,
            value: Value::undefined(),
            has_writable: false,
            writable: false,
            has_configurable: false,
            configurable: false,
            has_enumerable: false,
            enumerable: false,
            has_get: false,
            get: Value::undefined(),
            has_set: false,
            set: Value::undefined(),
        }
    }
}

impl JsPropertyDescriptorRecord {
    fn is_accessor(self) -> bool {
        self.has_get || self.has_set
    }

    fn is_data(self) -> bool {
        self.has_value || self.has_writable
    }
}

const SYMBOL_KEY_PREFIX: &str = "@@sym:";

#[derive(Default)]
struct OrderedOwnKeyCollector {
    indices: BTreeSet<u32>,
    strings_seen: FxHashSet<String>,
    strings: Vec<String>,
}

impl OrderedOwnKeyCollector {
    fn push(&mut self, name: impl Into<String>) {
        let name = name.into();
        if name.is_empty() || name == FIELD_PRESENT_MASK_KEY {
            return;
        }
        if let Some(index) = parse_js_array_index_name(&name) {
            let Ok(index) = u32::try_from(index) else {
                return;
            };
            self.indices.insert(index);
            return;
        }
        if self.strings_seen.insert(name.clone()) {
            self.strings.push(name);
        }
    }

    fn extend<I>(&mut self, names: I)
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        for name in names {
            self.push(name);
        }
    }

    fn finish(self) -> Vec<String> {
        let mut ordered = self
            .indices
            .into_iter()
            .map(|index| index.to_string())
            .collect::<Vec<_>>();
        ordered.extend(self.strings);
        ordered
    }
}

#[derive(Default, Clone)]
struct DynamicJsCompileOptions {
    direct_eval_entry_function: Option<String>,
    direct_eval_binding_names: FxHashSet<String>,
    has_parameter_named_arguments: bool,
    in_parameter_initializer: bool,
    uses_script_global_bindings: bool,
    allow_new_target: bool,
    allow_super_property: bool,
    track_top_level_completion: bool,
    emit_script_global_bindings: bool,
    script_global_bindings_configurable: bool,
}

struct DirectEvalIdentifierCollector<'a> {
    interner: &'a crate::parser::Interner,
    names: FxHashSet<String>,
}

impl Visitor for DirectEvalIdentifierCollector<'_> {
    fn visit_identifier(&mut self, ident: &crate::parser::ast::Identifier) {
        self.names
            .insert(self.interner.resolve(ident.name).to_string());
    }

    fn visit_member_expression(&mut self, expr: &crate::parser::ast::MemberExpression) {
        self.visit_expression(&expr.object);
    }
}

struct DirectEvalDeclarationCollector<'a> {
    interner: &'a crate::parser::Interner,
    declares_arguments: bool,
    seen_var_names: FxHashSet<String>,
    var_names: Vec<String>,
    seen_function_names: FxHashSet<String>,
    function_names: Vec<String>,
    seen_lexical_names: FxHashSet<String>,
    lexical_names: Vec<String>,
}

#[derive(Default, Clone)]
struct DirectEvalDeclarations {
    declares_arguments: bool,
    source_is_strict: bool,
    var_names: Vec<String>,
    function_names: Vec<String>,
    lexical_names: Vec<String>,
}

#[derive(Clone, Copy)]
struct DirectEvalBehavior {
    is_strict: bool,
    publish_script_global_bindings: bool,
    persist_caller_declarations: bool,
}

#[derive(Clone, Copy)]
struct AmbientGlobalDescriptor {
    value: Value,
    writable: bool,
    configurable: bool,
    enumerable: bool,
}

impl<'a> DirectEvalDeclarationCollector<'a> {
    fn new(interner: &'a crate::parser::Interner) -> Self {
        Self {
            interner,
            declares_arguments: false,
            seen_var_names: FxHashSet::default(),
            var_names: Vec::new(),
            seen_function_names: FxHashSet::default(),
            function_names: Vec::new(),
            seen_lexical_names: FxHashSet::default(),
            lexical_names: Vec::new(),
        }
    }

    fn push_var_name(&mut self, name: &str) {
        if self.seen_var_names.insert(name.to_string()) {
            self.var_names.push(name.to_string());
        }
    }

    fn push_function_name(&mut self, name: &str) {
        if self.seen_function_names.insert(name.to_string()) {
            self.function_names.push(name.to_string());
        }
    }

    fn push_lexical_name(&mut self, name: &str) {
        if self.seen_lexical_names.insert(name.to_string()) {
            self.lexical_names.push(name.to_string());
        }
    }

    fn collect_var_pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Identifier(ident) => {
                let name = self.interner.resolve(ident.name);
                if name == "arguments" {
                    self.declares_arguments = true;
                }
                self.push_var_name(name);
            }
            Pattern::Array(array) => {
                for element in array.elements.iter().flatten() {
                    self.collect_var_pattern(&element.pattern);
                }
                if let Some(rest) = &array.rest {
                    self.collect_var_pattern(rest);
                }
            }
            Pattern::Object(object) => {
                for property in &object.properties {
                    self.collect_var_pattern(&property.value);
                }
                if let Some(rest) = &object.rest {
                    let name = self.interner.resolve(rest.name);
                    if name == "arguments" {
                        self.declares_arguments = true;
                    }
                    self.push_var_name(name);
                }
            }
            Pattern::Rest(rest) => self.collect_var_pattern(&rest.argument),
        }
    }

    fn collect_lexical_pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Identifier(ident) => {
                let name = self.interner.resolve(ident.name);
                if name == "arguments" {
                    self.declares_arguments = true;
                }
                self.push_lexical_name(name);
            }
            Pattern::Array(array) => {
                for element in array.elements.iter().flatten() {
                    self.collect_lexical_pattern(&element.pattern);
                }
                if let Some(rest) = &array.rest {
                    self.collect_lexical_pattern(rest);
                }
            }
            Pattern::Object(object) => {
                for property in &object.properties {
                    self.collect_lexical_pattern(&property.value);
                }
                if let Some(rest) = &object.rest {
                    let name = self.interner.resolve(rest.name);
                    if name == "arguments" {
                        self.declares_arguments = true;
                    }
                    self.push_lexical_name(name);
                }
            }
            Pattern::Rest(rest) => self.collect_lexical_pattern(&rest.argument),
        }
    }
}

fn direct_eval_lexical_marker_key(name: &str) -> String {
    format!("{DIRECT_EVAL_LEXICAL_MARKER_PREFIX}{name}")
}

fn direct_eval_uninitialized_marker_key(name: &str) -> String {
    format!("{DIRECT_EVAL_UNINITIALIZED_MARKER_PREFIX}{name}")
}

fn direct_eval_outer_snapshot_marker_key(name: &str) -> String {
    format!("{DIRECT_EVAL_OUTER_SNAPSHOT_MARKER_PREFIX}{name}")
}

impl From<DirectEvalDeclarationCollector<'_>> for DirectEvalDeclarations {
    fn from(collector: DirectEvalDeclarationCollector<'_>) -> Self {
        Self {
            declares_arguments: collector.declares_arguments,
            source_is_strict: false,
            var_names: collector.var_names,
            function_names: collector.function_names,
            lexical_names: collector.lexical_names,
        }
    }
}

impl Visitor for DirectEvalDeclarationCollector<'_> {
    fn visit_variable_decl(&mut self, decl: &VariableDecl) {
        if matches!(decl.kind, crate::parser::ast::VariableKind::Var) {
            self.collect_var_pattern(&decl.pattern);
        } else {
            self.collect_lexical_pattern(&decl.pattern);
        }
        if let Some(initializer) = &decl.initializer {
            self.visit_expression(initializer);
        }
    }

    fn visit_class_decl(&mut self, decl: &ClassDecl) {
        let name = self.interner.resolve(decl.name.name);
        if name == "arguments" {
            self.declares_arguments = true;
        }
        self.push_lexical_name(name);
    }

    fn visit_function_decl(&mut self, decl: &FunctionDecl) {
        let name = self.interner.resolve(decl.name.name);
        if name == "arguments" {
            self.declares_arguments = true;
        }
        self.push_function_name(name);
    }

    fn visit_function_expression(&mut self, _func: &FunctionExpression) {}

    fn visit_arrow_function(&mut self, _func: &ArrowFunction) {}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JsOwnPropertySource {
    ArgumentsExotic,
    ArrayExotic,
    StringExotic,
    TypedArrayExotic,
    BuiltinObjectConstant,
    Ordinary,
    Metadata,
    BuiltinGlobal,
    CallableVirtual,
    NominalMethod,
    BuiltinNativeMethod,
    ConstructorStaticField,
    ConstructorStaticAccessor,
    ConstructorStaticMethod,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JsOwnPropertyKind {
    Data,
    Accessor,
    PoisonedAccessor,
}

#[derive(Clone, Copy, Debug)]
struct JsOwnPropertyShape {
    source: JsOwnPropertySource,
    kind: JsOwnPropertyKind,
    writable: bool,
    configurable: bool,
    enumerable: bool,
}

impl JsOwnPropertyShape {
    fn data(
        source: JsOwnPropertySource,
        writable: bool,
        configurable: bool,
        enumerable: bool,
    ) -> Self {
        Self {
            source,
            kind: JsOwnPropertyKind::Data,
            writable,
            configurable,
            enumerable,
        }
    }

    fn accessor(source: JsOwnPropertySource, configurable: bool, enumerable: bool) -> Self {
        Self {
            source,
            kind: JsOwnPropertyKind::Accessor,
            writable: false,
            configurable,
            enumerable,
        }
    }

    fn poisoned_accessor(
        source: JsOwnPropertySource,
        configurable: bool,
        enumerable: bool,
    ) -> Self {
        Self {
            source,
            kind: JsOwnPropertyKind::PoisonedAccessor,
            writable: false,
            configurable,
            enumerable,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct JsOwnPropertyRecord {
    shape: JsOwnPropertyShape,
    value: Option<Value>,
    getter: Option<Value>,
    setter: Option<Value>,
}

impl JsOwnPropertyRecord {
    fn data(shape: JsOwnPropertyShape, value: Value) -> Self {
        Self {
            shape,
            value: Some(value),
            getter: None,
            setter: None,
        }
    }

    fn accessor(shape: JsOwnPropertyShape, getter: Option<Value>, setter: Option<Value>) -> Self {
        Self {
            shape,
            value: None,
            getter,
            setter,
        }
    }

    fn as_ordinary_property(self) -> OrdinaryOwnProperty {
        match self.shape.kind {
            JsOwnPropertyKind::Data => OrdinaryOwnProperty::Data {
                value: self.value.unwrap_or(Value::undefined()),
                writable: self.shape.writable,
                enumerable: self.shape.enumerable,
                configurable: self.shape.configurable,
            },
            JsOwnPropertyKind::Accessor | JsOwnPropertyKind::PoisonedAccessor => {
                OrdinaryOwnProperty::Accessor {
                    get: self.getter.unwrap_or(Value::undefined()),
                    set: self.setter.unwrap_or(Value::undefined()),
                    enumerable: self.shape.enumerable,
                    configurable: self.shape.configurable,
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct JsResolvedPropertyRecord {
    owner: Value,
    record: JsOwnPropertyRecord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JsExoticAdapterKind {
    Arguments,
    Array,
    String,
    TypedArray,
}

#[derive(Clone, Copy)]
struct ArgumentsDataDescriptorState {
    value: Value,
    writable: bool,
    enumerable: bool,
    configurable: bool,
}

#[derive(Clone, Copy)]
pub(in crate::vm::interpreter::opcodes) enum OrdinaryOwnProperty {
    Data {
        value: Value,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    },
    Accessor {
        get: Value,
        set: Value,
        enumerable: bool,
        configurable: bool,
    },
}

fn value_as_string(arg: Value) -> Result<String, VmError> {
    if !arg.is_ptr() {
        return Err(VmError::TypeError("Expected string".to_string()));
    }
    let Some(s) = checked_string_ptr(arg) else {
        return Err(VmError::TypeError("Expected string".to_string()));
    };
    Ok(unsafe { &*s.as_ptr() }.data.clone())
}

impl<'a> Interpreter<'a> {
    const GENERATOR_RETURN_SIGNAL_KEY: &'static str = "__raya_generator_return_signal__";
    const GENERATOR_RETURN_VALUE_KEY: &'static str = "__raya_generator_return_value__";

    fn alloc_bound_native_value(&self, receiver: Value, native_id: u16) -> Value {
        let method = Object::new_bound_native(receiver, native_id);
        let method_ptr = self.gc.lock().allocate(method);
        unsafe { Value::from_ptr(NonNull::new(method_ptr.as_ptr()).expect("bound native ptr")) }
    }

    fn record_test262_async_callback_success(&self) {
        let _ = self.test262_async_state.compare_exchange(
            0,
            1,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
        );
    }

    fn record_test262_async_callback_failure(&self, message: String) {
        if self
            .test262_async_state
            .compare_exchange(
                0,
                2,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .is_ok()
        {
            *self.test262_async_failure.lock() = Some(message);
        }
    }

    fn format_test262_async_failure_message(&self, error: Value) -> String {
        if let Some(obj_ptr) = checked_object_ptr(error) {
            let obj = unsafe { &*obj_ptr.as_ptr() };
            let name = self
                .get_object_named_field_value(obj, "name")
                .and_then(primitive_to_js_string)
                .unwrap_or_else(|| "Error".to_string());
            let message = self
                .get_object_named_field_value(obj, "message")
                .and_then(primitive_to_js_string)
                .unwrap_or_else(|| "[object Object]".to_string());
            return format!("{name}: {message}");
        }

        let rendered = primitive_to_js_string(error).unwrap_or_else(|| "undefined".to_string());
        format!("Test262Error: {rendered}")
    }

    fn generator_result_object(&self, value: Value, done: bool) -> Value {
        let mut result = Object::new_dynamic(layout_id_from_ordered_names(&[]), 0);
        {
            let dyn_props = result.ensure_dyn_props();
            dyn_props.insert(
                self.intern_prop_key("value"),
                DynProp::data_with_attrs(value, true, true, true),
            );
            dyn_props.insert(
                self.intern_prop_key("done"),
                DynProp::data_with_attrs(Value::bool(done), true, true, true),
            );
        }
        if let Some(object_ctor) = self.builtin_global_value("Object") {
            if let Some(prototype) = self.constructor_prototype_value(object_ctor) {
                result.prototype = prototype;
            }
        }
        let result_ptr = self.gc.lock().allocate(result);
        unsafe { Value::from_ptr(NonNull::new(result_ptr.as_ptr()).expect("iterator result ptr")) }
    }

    fn generator_snapshot_iterator_object(&self, yielded: Vec<Value>, completion: Value) -> Value {
        let mut iterator = Object::new_dynamic(layout_id_from_ordered_names(&[]), 0);
        iterator.generator_snapshot = Some(Box::new(GeneratorSnapshotData {
            yielded,
            next_index: 0,
            completion,
            completion_emitted: false,
        }));
        {
            let dyn_props = iterator.ensure_dyn_props();
            dyn_props.insert(
                self.intern_prop_key("next"),
                DynProp::data_with_attrs(
                    self.alloc_bound_native_value(
                        Value::null(),
                        crate::compiler::native_id::OBJECT_GENERATOR_SNAPSHOT_NEXT,
                    ),
                    true,
                    false,
                    true,
                ),
            );
            dyn_props.insert(
                self.intern_prop_key("return"),
                DynProp::data_with_attrs(
                    self.alloc_bound_native_value(
                        Value::null(),
                        crate::compiler::native_id::OBJECT_GENERATOR_SNAPSHOT_RETURN,
                    ),
                    true,
                    false,
                    true,
                ),
            );
            dyn_props.insert(
                self.intern_prop_key("Symbol.iterator"),
                DynProp::data_with_attrs(
                    self.alloc_bound_native_value(
                        Value::null(),
                        crate::compiler::native_id::OBJECT_GENERATOR_SNAPSHOT_ITERATOR,
                    ),
                    true,
                    false,
                    true,
                ),
            );
        }
        if let Some(generator_ctor) = self.builtin_global_value("Generator") {
            if let Some(prototype) = self.constructor_prototype_value(generator_ctor) {
                iterator.prototype = prototype;
            }
        }
        let iterator_ptr = self.gc.lock().allocate(iterator);
        let iterator_value =
            unsafe { Value::from_ptr(NonNull::new(iterator_ptr.as_ptr()).expect("iterator ptr")) };

        if let Some(obj_ptr) = checked_object_ptr(iterator_value) {
            let iterator = unsafe { &mut *obj_ptr.as_ptr() };
            if let Some(dyn_props) = iterator.dyn_props_mut() {
                for key in ["next", "return", "Symbol.iterator"] {
                    if let Some(prop) = dyn_props.get_mut(self.intern_prop_key(key)) {
                        let native_id = match key {
                            "next" => crate::compiler::native_id::OBJECT_GENERATOR_SNAPSHOT_NEXT,
                            "return" => {
                                crate::compiler::native_id::OBJECT_GENERATOR_SNAPSHOT_RETURN
                            }
                            _ => crate::compiler::native_id::OBJECT_GENERATOR_SNAPSHOT_ITERATOR,
                        };
                        prop.value = self.alloc_bound_native_value(iterator_value, native_id);
                    }
                }
            }
        }

        iterator_value
    }

    fn generator_iterator_object(
        &self,
        task_id: TaskId,
        prototype: Option<Value>,
        is_async: bool,
    ) -> Value {
        let mut iterator = Object::new_dynamic(layout_id_from_ordered_names(&[]), 0);
        iterator.generator_state = Some(Box::new(GeneratorStateData {
            task_id,
            is_async,
            started: false,
            closed: false,
            pending_return_completion: None,
            completion: Value::undefined(),
            completion_emitted: false,
        }));
        if !is_async {
            let dyn_props = iterator.ensure_dyn_props();
            dyn_props.insert(
                self.intern_prop_key("next"),
                DynProp::data_with_attrs(
                    self.alloc_bound_native_value(
                        Value::null(),
                        crate::compiler::native_id::OBJECT_GENERATOR_NEXT,
                    ),
                    true,
                    false,
                    true,
                ),
            );
            dyn_props.insert(
                self.intern_prop_key("return"),
                DynProp::data_with_attrs(
                    self.alloc_bound_native_value(
                        Value::null(),
                        crate::compiler::native_id::OBJECT_GENERATOR_RETURN,
                    ),
                    true,
                    false,
                    true,
                ),
            );
            dyn_props.insert(
                self.intern_prop_key("Symbol.iterator"),
                DynProp::data_with_attrs(
                    self.alloc_bound_native_value(
                        Value::null(),
                        crate::compiler::native_id::OBJECT_GENERATOR_ITERATOR,
                    ),
                    true,
                    false,
                    true,
                ),
            );
        }
        if let Some(prototype) = prototype {
            iterator.prototype = prototype;
        }
        let iterator_ptr = self.gc.lock().allocate(iterator);
        let iterator_value =
            unsafe { Value::from_ptr(NonNull::new(iterator_ptr.as_ptr()).expect("iterator ptr")) };

        if !is_async {
            if let Some(obj_ptr) = checked_object_ptr(iterator_value) {
                let iterator = unsafe { &mut *obj_ptr.as_ptr() };
                if let Some(dyn_props) = iterator.dyn_props_mut() {
                    for key in ["next", "return", "Symbol.iterator"] {
                        if let Some(prop) = dyn_props.get_mut(self.intern_prop_key(key)) {
                            let native_id = match key {
                                "next" => crate::compiler::native_id::OBJECT_GENERATOR_NEXT,
                                "return" => crate::compiler::native_id::OBJECT_GENERATOR_RETURN,
                                _ => crate::compiler::native_id::OBJECT_GENERATOR_ITERATOR,
                            };
                            prop.value = self.alloc_bound_native_value(iterator_value, native_id);
                        }
                    }
                }
            }
        }

        iterator_value
    }

    fn generator_return_signal(&self, completion: Value) -> Value {
        let mut signal = Object::new_dynamic(layout_id_from_ordered_names(&[]), 0);
        {
            let dyn_props = signal.ensure_dyn_props();
            dyn_props.insert(
                self.intern_prop_key(Self::GENERATOR_RETURN_SIGNAL_KEY),
                DynProp::data_with_attrs(Value::bool(true), false, false, false),
            );
            dyn_props.insert(
                self.intern_prop_key(Self::GENERATOR_RETURN_VALUE_KEY),
                DynProp::data_with_attrs(completion, false, false, false),
            );
        }
        let signal_ptr = self.gc.lock().allocate(signal);
        unsafe { Value::from_ptr(NonNull::new(signal_ptr.as_ptr()).expect("signal ptr")) }
    }

    fn settled_task_handle(
        &mut self,
        caller_task: &Arc<Task>,
        value: Result<Value, Value>,
    ) -> PromiseHandle {
        let settled_task = Arc::new(Task::with_args(
            0,
            caller_task.current_module(),
            Some(caller_task.id()),
            vec![],
        ));
        settled_task.replace_stack(self.stack_pool.acquire());
        if std::env::var("RAYA_DEBUG_ASYNC_TASKS").is_ok() {
            let current_module = caller_task.current_module();
            let current_func_id = caller_task.current_func_id();
            let current_func_name = current_module
                .functions
                .get(current_func_id)
                .map(|function| function.name.as_str())
                .unwrap_or("<unknown>");
            let detail = match &value {
                Ok(result) => format!("resolve raw={:#x}", result.raw()),
                Err(exception) => format!("reject raw={:#x}", exception.raw()),
            };
            eprintln!(
                "[async-task] settled task={:?} parent={:?} from={}::{}#{} {}",
                settled_task.id(),
                caller_task.id(),
                current_module.metadata.name,
                current_func_name,
                current_func_id,
                detail
            );
        }
        match value {
            Ok(result) => settled_task.complete(result),
            Err(exception) => {
                settled_task.set_exception(exception);
                settled_task.fail();
            }
        }
        let task_id = settled_task.id();
        self.tasks.write().insert(task_id, settled_task);
        PromiseHandle::new(task_id)
    }

    fn pending_task_handle(&mut self, caller_task: &Arc<Task>) -> PromiseHandle {
        let pending_task = Arc::new(Task::with_args(
            0,
            caller_task.current_module(),
            Some(caller_task.id()),
            vec![],
        ));
        pending_task.replace_stack(self.stack_pool.acquire());
        let task_id = pending_task.id();
        self.tasks.write().insert(task_id, pending_task);
        PromiseHandle::new(task_id)
    }

    pub(in crate::vm::interpreter) fn promise_handle_from_value(
        &self,
        value: Value,
    ) -> Option<PromiseHandle> {
        let handle = PromiseHandle::from_value(value)?;
        self.tasks
            .read()
            .contains_key(&handle.task_id())
            .then_some(handle)
    }

    fn task_from_promise_handle(&self, handle: PromiseHandle) -> Option<Arc<Task>> {
        self.tasks.read().get(&handle.task_id()).cloned()
    }

    fn task_from_handle_value(&self, value: Value) -> Option<Arc<Task>> {
        let handle = self.promise_handle_from_value(value)?;
        self.task_from_promise_handle(handle)
    }

    fn enqueue_promise_microtask(&self, job: PromiseMicrotask) {
        self.promise_microtasks.lock().push_back(job);
    }

    pub(in crate::vm::interpreter) fn task_uses_async_js_promise_semantics(
        &self,
        task: &Arc<Task>,
    ) -> bool {
        let module = task.current_module();
        let function = module.functions.get(task.current_func_id());
        function.is_some_and(|function| {
            function.is_async && function.uses_js_runtime_semantics && !function.is_generator
        })
    }

    fn normalize_promise_source_task(
        &mut self,
        value: Value,
        caller_task: &Arc<Task>,
    ) -> Arc<Task> {
        if let Some(task) = self.task_from_handle_value(value) {
            return task;
        }
        let settled = self.settled_task_handle(caller_task, Ok(value));
        self.task_from_promise_handle(settled)
            .expect("settled_task_handle must create a valid PromiseHandle")
    }

    fn queue_or_attach_promise_reaction(&self, source_task: &Arc<Task>, reaction: PromiseReaction) {
        source_task.mark_rejection_observed();
        if !source_task.add_reaction_if_incomplete(reaction) {
            self.enqueue_promise_microtask(PromiseMicrotask::RunReaction {
                source_task_id: source_task.id(),
                reaction,
            });
        }
    }

    fn promise_chain_handle(
        &mut self,
        source: Value,
        on_fulfilled: Value,
        on_rejected: Value,
        caller_task: &Arc<Task>,
    ) -> Value {
        let target_handle = self.pending_task_handle(caller_task);
        let target_task_id = target_handle.task_id();
        let source_task = self.normalize_promise_source_task(source, caller_task);
        self.queue_or_attach_promise_reaction(
            &source_task,
            PromiseReaction {
                target_task_id,
                kind: PromiseReactionKind::Chain,
                on_fulfilled,
                on_rejected,
            },
        );
        target_handle.into_value()
    }

    fn promise_finally_handle(
        &mut self,
        source: Value,
        on_finally: Value,
        caller_task: &Arc<Task>,
    ) -> Value {
        let target_handle = self.pending_task_handle(caller_task);
        let target_task_id = target_handle.task_id();
        let source_task = self.normalize_promise_source_task(source, caller_task);
        self.queue_or_attach_promise_reaction(
            &source_task,
            PromiseReaction {
                target_task_id,
                kind: PromiseReactionKind::Finally,
                on_fulfilled: on_finally,
                on_rejected: Value::undefined(),
            },
        );
        target_handle.into_value()
    }

    fn promise_all_state_value(&mut self, count: usize) -> Result<Value, VmError> {
        let remaining = i32::try_from(count)
            .map_err(|_| VmError::RuntimeError("Promise.all input is too large".to_string()))?;
        let results_ptr = self.gc.lock().allocate(Array::new(0, count));
        let results = unsafe {
            Value::from_ptr(
                std::ptr::NonNull::new(results_ptr.as_ptr()).expect("promise all results array"),
            )
        };
        let mut state = Object::new_dynamic(layout_id_from_ordered_names(&[]), 0);
        if let Some(dyn_props) = state.dyn_props.as_mut() {
            dyn_props.insert(
                self.intern_prop_key("__promiseAllResults"),
                DynProp::data_with_attrs(results, false, false, false),
            );
            dyn_props.insert(
                self.intern_prop_key("__promiseAllRemaining"),
                DynProp::data_with_attrs(Value::i32(remaining), false, false, false),
            );
        }
        let state_ptr = self.gc.lock().allocate(state);
        Ok(unsafe {
            Value::from_ptr(
                std::ptr::NonNull::new(state_ptr.as_ptr()).expect("promise all state object"),
            )
        })
    }

    fn promise_all_state_store_result(
        &mut self,
        state: Value,
        index: usize,
        value: Value,
    ) -> Result<Option<Value>, VmError> {
        let results_key = self.intern_prop_key("__promiseAllResults");
        let remaining_key = self.intern_prop_key("__promiseAllRemaining");
        let Some(state_ptr) = checked_object_ptr(state) else {
            return Err(VmError::RuntimeError(
                "Promise.all aggregate state is not an object".to_string(),
            ));
        };
        let state = unsafe { &mut *state_ptr.as_ptr() };
        let Some(dyn_props) = state.dyn_props.as_mut() else {
            return Err(VmError::RuntimeError(
                "Promise.all aggregate state has no dynamic properties".to_string(),
            ));
        };
        let results = dyn_props
            .get(results_key)
            .map(|prop| prop.value)
            .ok_or_else(|| {
                VmError::RuntimeError("Promise.all aggregate results are missing".to_string())
            })?;
        let remaining = dyn_props
            .get(remaining_key)
            .and_then(|prop| prop.value.as_i32())
            .ok_or_else(|| {
                VmError::RuntimeError(
                    "Promise.all aggregate remaining count is missing".to_string(),
                )
            })?;
        let Some(results_ptr) = checked_array_ptr(results) else {
            return Err(VmError::RuntimeError(
                "Promise.all aggregate results are not an array".to_string(),
            ));
        };
        let results_array = unsafe { &mut *results_ptr.as_ptr() };
        results_array
            .set(index, value)
            .map_err(VmError::RuntimeError)?;
        let next_remaining = remaining - 1;
        dyn_props.insert(
            remaining_key,
            DynProp::data_with_attrs(Value::i32(next_remaining), false, false, false),
        );
        Ok((next_remaining == 0).then_some(results))
    }

    fn promise_all_handle(
        &mut self,
        values: Value,
        caller_task: &Arc<Task>,
    ) -> Result<Value, VmError> {
        let Some(array_ptr) = checked_array_ptr(values) else {
            return Err(VmError::TypeError(
                "Promise.all expects an array of values".to_string(),
            ));
        };
        let array = unsafe { &*array_ptr.as_ptr() };
        if array.is_empty() {
            let empty_ptr = self.gc.lock().allocate(Array::new(0, 0));
            let empty = unsafe {
                Value::from_ptr(std::ptr::NonNull::new(empty_ptr.as_ptr()).expect("empty array"))
            };
            return Ok(self
                .settled_task_handle(caller_task, Ok(empty))
                .into_value());
        }

        let state = self.promise_all_state_value(array.len())?;
        let target_handle = self.pending_task_handle(caller_task);
        let target_task_id = target_handle.task_id();

        for index in 0..array.len() {
            let source = array.get(index).unwrap_or(Value::undefined());
            let source_task = self.normalize_promise_source_task(source, caller_task);
            self.queue_or_attach_promise_reaction(
                &source_task,
                PromiseReaction {
                    target_task_id,
                    kind: PromiseReactionKind::All {
                        state,
                        index: index as u32,
                    },
                    on_fulfilled: Value::undefined(),
                    on_rejected: Value::undefined(),
                },
            );
        }

        Ok(target_handle.into_value())
    }

    fn promise_race_handle(
        &mut self,
        values: Value,
        caller_task: &Arc<Task>,
    ) -> Result<Value, VmError> {
        let Some(array_ptr) = checked_array_ptr(values) else {
            return Err(VmError::TypeError(
                "Promise.race expects an array of values".to_string(),
            ));
        };
        let array = unsafe { &*array_ptr.as_ptr() };
        if array.is_empty() {
            return Ok(self
                .settled_task_handle(
                    caller_task,
                    Err(self.alloc_string_value("Promise.race requires at least one promise")),
                )
                .into_value());
        }

        let target_handle = self.pending_task_handle(caller_task);
        let target_task_id = target_handle.task_id();

        for index in 0..array.len() {
            let source = array.get(index).unwrap_or(Value::undefined());
            let source_task = self.normalize_promise_source_task(source, caller_task);
            self.queue_or_attach_promise_reaction(
                &source_task,
                PromiseReaction {
                    target_task_id,
                    kind: PromiseReactionKind::Race,
                    on_fulfilled: Value::undefined(),
                    on_rejected: Value::undefined(),
                },
            );
        }

        Ok(target_handle.into_value())
    }

    pub(crate) fn run_promise_reaction(
        &mut self,
        source_task: &Arc<Task>,
        reaction: PromiseReaction,
    ) {
        let target_handle = PromiseHandle::new(reaction.target_task_id).into_value();
        let Some(target_task) = self.tasks.read().get(&reaction.target_task_id).cloned() else {
            return;
        };
        if matches!(
            target_task.state(),
            TaskState::Completed | TaskState::Failed
        ) {
            return;
        }

        let source_failed = source_task.state() == TaskState::Failed;
        let source_result = source_task.result().unwrap_or(Value::undefined());
        let source_reason = source_task.current_exception().unwrap_or(Value::null());
        if source_failed {
            source_task.mark_rejection_observed();
        }

        match reaction.kind {
            PromiseReactionKind::Chain => {
                let callback = if source_failed {
                    reaction.on_rejected
                } else {
                    reaction.on_fulfilled
                };
                let input = if source_failed {
                    source_reason
                } else {
                    source_result
                };
                if !self.js_call_target_supported(callback) {
                    let _ = self.settle_existing_task_handle(
                        target_handle,
                        if source_failed { Err(input) } else { Ok(input) },
                    );
                    return;
                }
                let caller_module = target_task.current_module();
                match self.invoke_callable_sync_with_this(
                    callback,
                    Some(Value::undefined()),
                    &[input],
                    &target_task,
                    &caller_module,
                ) {
                    Ok(returned) => {
                        let _ = self.settle_existing_task_handle(target_handle, Ok(returned));
                    }
                    Err(error) => {
                        self.ensure_task_exception_for_error(&target_task, &error);
                        let reason = target_task
                            .current_exception()
                            .unwrap_or(Value::undefined());
                        let _ = self.settle_existing_task_handle(target_handle, Err(reason));
                    }
                }
            }
            PromiseReactionKind::Finally => {
                let original_failed = source_failed;
                let original = if source_failed {
                    Err(source_reason)
                } else {
                    Ok(source_result)
                };
                let callback = reaction.on_fulfilled;
                if !self.js_call_target_supported(callback) {
                    let _ = self.settle_existing_task_handle(target_handle, original);
                    return;
                }
                let caller_module = target_task.current_module();
                match self.invoke_callable_sync_with_this(
                    callback,
                    Some(Value::undefined()),
                    &[],
                    &target_task,
                    &caller_module,
                ) {
                    Ok(returned) => {
                        if let Some(finalizer_task) = self.task_from_handle_value(returned) {
                            let original_value = match original {
                                Ok(value) | Err(value) => value,
                            };
                            self.queue_or_attach_promise_reaction(
                                &finalizer_task,
                                PromiseReaction {
                                    target_task_id: reaction.target_task_id,
                                    kind: PromiseReactionKind::FinallyResume {
                                        original: original_value,
                                        failed: original_failed,
                                    },
                                    on_fulfilled: Value::undefined(),
                                    on_rejected: Value::undefined(),
                                },
                            );
                            return;
                        }
                        let _ = self.settle_existing_task_handle(target_handle, original);
                    }
                    Err(error) => {
                        self.ensure_task_exception_for_error(&target_task, &error);
                        let reason = target_task
                            .current_exception()
                            .unwrap_or(Value::undefined());
                        let _ = self.settle_existing_task_handle(target_handle, Err(reason));
                    }
                }
            }
            PromiseReactionKind::FinallyResume { original, failed } => {
                if source_failed {
                    let _ = self.settle_existing_task_handle(target_handle, Err(source_reason));
                } else {
                    let _ = self.settle_existing_task_handle(
                        target_handle,
                        if failed { Err(original) } else { Ok(original) },
                    );
                }
            }
            PromiseReactionKind::All { state, index } => {
                if source_failed {
                    let _ = self.settle_existing_task_handle(target_handle, Err(source_reason));
                    return;
                }
                match self.promise_all_state_store_result(state, index as usize, source_result) {
                    Ok(Some(results)) => {
                        let _ = self.settle_existing_task_handle(target_handle, Ok(results));
                    }
                    Ok(None) => {}
                    Err(error) => {
                        self.ensure_task_exception_for_error(&target_task, &error);
                        let reason = target_task
                            .current_exception()
                            .unwrap_or(Value::undefined());
                        let _ = self.settle_existing_task_handle(target_handle, Err(reason));
                    }
                }
            }
            PromiseReactionKind::Race => {
                let _ = self.settle_existing_task_handle(
                    target_handle,
                    if source_failed {
                        Err(source_reason)
                    } else {
                        Ok(source_result)
                    },
                );
            }
        }
    }

    fn wake_task_waiters(&self, task: &Arc<Task>) {
        let waiters = task.take_waiters();
        if waiters.is_empty() {
            return;
        }

        let task_failed = task.state() == TaskState::Failed;
        let task_result = task.result();
        let task_exception = if task_failed {
            task.mark_rejection_observed();
            Some(task.current_exception().unwrap_or(Value::null()))
        } else {
            None
        };

        let tasks_map = self.tasks.read();
        let waiter_tasks: Vec<_> = waiters
            .into_iter()
            .filter_map(|task_id| tasks_map.get(&task_id).cloned())
            .collect();
        drop(tasks_map);

        for waiter in waiter_tasks {
            if let Some(exception) = task_exception {
                let _ = waiter.take_resume_value();
                waiter.set_exception(exception);
            } else if let Some(result) = task_result {
                waiter.set_resume_value(result);
            }
            if waiter.resume_if_pending() {
                waiter.clear_suspend_reason();
                self.injector.push(waiter);
            }
        }
    }

    fn propagate_task_settlement(&self, task: &Arc<Task>) {
        let mut settled = vec![task.clone()];
        while let Some(source) = settled.pop() {
            self.wake_task_waiters(&source);
            for reaction in source.take_reactions() {
                self.enqueue_promise_microtask(PromiseMicrotask::RunReaction {
                    source_task_id: source.id(),
                    reaction,
                });
            }

            let adopters = source.take_adopters();
            if adopters.is_empty() {
                continue;
            }

            let source_failed = source.state() == TaskState::Failed;
            let source_result = source.result();
            let source_exception = if source_failed {
                source.mark_rejection_observed();
                Some(source.current_exception().unwrap_or(Value::null()))
            } else {
                None
            };

            let tasks_map = self.tasks.read();
            let adopter_tasks: Vec<_> = adopters
                .into_iter()
                .filter_map(|task_id| tasks_map.get(&task_id).cloned())
                .collect();
            drop(tasks_map);

            for adopter in adopter_tasks {
                match adopter.state() {
                    TaskState::Completed | TaskState::Failed => continue,
                    _ => {}
                }
                if let Some(exception) = source_exception {
                    adopter.set_exception(exception);
                    adopter.fail();
                } else if let Some(result) = source_result {
                    adopter.complete(result);
                }
                settled.push(adopter);
            }
        }
    }

    fn reject_task_handle_with_exception(&self, pending_task: &Arc<Task>, exception: Value) {
        pending_task.set_exception(exception);
        pending_task.fail();
        self.propagate_task_settlement(pending_task);
    }

    fn mirror_task_settlement(&self, pending_task: &Arc<Task>, source_task: &Arc<Task>) {
        match source_task.state() {
            TaskState::Completed => {
                pending_task.complete(source_task.result().unwrap_or(Value::undefined()));
            }
            TaskState::Failed => {
                source_task.mark_rejection_observed();
                pending_task
                    .set_exception(source_task.current_exception().unwrap_or(Value::null()));
                pending_task.fail();
            }
            _ => return,
        }
        self.propagate_task_settlement(pending_task);
    }

    fn settle_existing_task_handle(
        &mut self,
        task_handle: Value,
        value: Result<Value, Value>,
    ) -> Result<(), VmError> {
        let Some(handle) = self.promise_handle_from_value(task_handle) else {
            return Err(VmError::TypeError(
                "Promise resolver receiver is not a task handle".to_string(),
            ));
        };
        let Some(pending_task) = self.task_from_promise_handle(handle) else {
            return Err(VmError::TypeError(
                "Promise resolver target task does not exist".to_string(),
            ));
        };
        match pending_task.state() {
            TaskState::Completed | TaskState::Failed => return Ok(()),
            _ => {}
        }
        match value {
            Ok(result) => {
                if let Some(source_task) = self.task_from_handle_value(result) {
                    if source_task.id() == pending_task.id() {
                        let error = VmError::TypeError(
                            "Promise cannot be resolved with itself".to_string(),
                        );
                        self.ensure_task_exception_for_error(&pending_task, &error);
                        pending_task.fail();
                        self.propagate_task_settlement(&pending_task);
                        return Ok(());
                    }
                    if source_task.add_adopter_if_incomplete(pending_task.id()) {
                        return Ok(());
                    }
                    self.mirror_task_settlement(&pending_task, &source_task);
                    return Ok(());
                }
                pending_task.complete(result);
                self.propagate_task_settlement(&pending_task);
            }
            Err(exception) => self.reject_task_handle_with_exception(&pending_task, exception),
        }
        Ok(())
    }

    pub(in crate::vm::interpreter) fn settle_completed_async_task(
        &mut self,
        task: &Arc<Task>,
        value: Value,
    ) -> Result<(), VmError> {
        if self.task_uses_async_js_promise_semantics(task) {
            self.settle_existing_task_handle(PromiseHandle::new(task.id()).into_value(), Ok(value))
        } else {
            task.complete(value);
            Ok(())
        }
    }

    fn construct_builtin_promise(
        &mut self,
        args: &[Value],
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Value, VmError> {
        let executor = args.first().copied().unwrap_or(Value::undefined());
        if !self.js_call_target_supported(executor) {
            return Err(VmError::TypeError(
                "Promise constructor executor is not callable".to_string(),
            ));
        }

        let promise_handle = self.pending_task_handle(caller_task);
        let promise_value = promise_handle.into_value();
        let resolve = self.alloc_bound_native_value(
            promise_value,
            crate::compiler::native_id::TASK_RESOLVE_PENDING,
        );
        let reject = self.alloc_bound_native_value(
            promise_value,
            crate::compiler::native_id::TASK_REJECT_PENDING,
        );

        if let Err(error) = self.invoke_callable_sync_with_this(
            executor,
            Some(Value::undefined()),
            &[resolve, reject],
            caller_task,
            caller_module,
        ) {
            if let Some(pending_task) = self.task_from_promise_handle(promise_handle) {
                if let Some(exception) = caller_task.current_exception() {
                    pending_task.set_exception(exception);
                    caller_task.clear_exception();
                } else {
                    self.ensure_task_exception_for_error(&pending_task, &error);
                }
                pending_task.fail();
                self.propagate_task_settlement(&pending_task);
            }
        }

        Ok(promise_value)
    }

    fn generator_return_signal_value(&self, value: Value) -> Option<Value> {
        let obj_ptr = checked_object_ptr(value)?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let dyn_props = obj.dyn_props()?;
        let marker = dyn_props.get(self.intern_prop_key(Self::GENERATOR_RETURN_SIGNAL_KEY))?;
        if !marker.value.is_bool() || !marker.value.as_bool().unwrap_or(false) {
            return None;
        }
        dyn_props
            .get(self.intern_prop_key(Self::GENERATOR_RETURN_VALUE_KEY))
            .map(|prop| prop.value)
    }

    fn take_generator_return_completion(&self, task: &Arc<Task>) -> Option<Value> {
        let completion = task
            .current_exception()
            .and_then(|exception| self.generator_return_signal_value(exception))?;
        task.clear_exception();
        Some(completion)
    }

    fn iterator_close_completion_value(
        &mut self,
        iterator: Value,
        completion: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Value {
        let is_generator_return = self.generator_return_signal_value(completion).is_some();
        match self.iterator_close(iterator, task, module) {
            Ok(()) => completion,
            Err(error) if is_generator_return => {
                self.ensure_task_exception_for_error(task, &error);
                let exception = task.current_exception().unwrap_or(Value::undefined());
                task.clear_exception();
                exception
            }
            Err(_) => {
                task.clear_exception();
                completion
            }
        }
    }

    fn inherit_generator_task_context(&self, callee_task: &Arc<Task>, caller_task: &Arc<Task>) {
        if let Some(env) = caller_task.current_active_direct_eval_env() {
            callee_task.push_active_direct_eval_env(
                env,
                caller_task.current_active_direct_eval_is_strict(),
                caller_task.current_active_direct_eval_uses_script_global_bindings(),
                caller_task.current_active_direct_eval_persist_caller_declarations(),
            );
            if let Some(completion) = caller_task.current_active_direct_eval_completion() {
                let _ = callee_task.set_current_active_direct_eval_completion(completion);
            }
        }
        if let Some(home_object) = caller_task.current_active_js_home_object() {
            callee_task.push_active_js_home_object(home_object);
        }
        if let Some(new_target) = caller_task.current_active_js_new_target() {
            callee_task.push_active_js_new_target(new_target);
        }
    }

    fn default_generator_instance_prototype(&self, is_async: bool) -> Option<Value> {
        let family_name = if is_async {
            "AsyncGenerator"
        } else {
            "Generator"
        };
        self.builtin_global_value(family_name)
            .and_then(|ctor| self.constructor_prototype_value(ctor))
    }

    fn generator_instance_prototype_for_function(
        &mut self,
        constructor: Option<Value>,
        func_id: usize,
        function_module: &Module,
        caller_task: &Arc<Task>,
    ) -> Result<Option<Value>, VmError> {
        if let Some(constructor) = constructor {
            if let Some(prototype) = self.get_property_value_via_js_semantics_with_context(
                constructor,
                "prototype",
                caller_task,
                function_module,
            )? {
                if self.is_js_object_value(prototype) {
                    return Ok(Some(prototype));
                }
            }
        }

        let is_async = function_module
            .functions
            .get(func_id)
            .is_some_and(|function| function.is_async);
        Ok(self.default_generator_instance_prototype(is_async))
    }

    pub(in crate::vm::interpreter) fn create_generator_task_object(
        &mut self,
        func_id: usize,
        function_module: Arc<Module>,
        args: Vec<Value>,
        closure: Option<Value>,
        caller_task: &Arc<Task>,
    ) -> Result<Value, VmError> {
        let is_async = function_module
            .functions
            .get(func_id)
            .is_some_and(|function| function.is_async);
        let generator_task = Arc::new(Task::with_args(
            func_id,
            function_module,
            Some(caller_task.id()),
            args,
        ));
        if let Some(closure) = closure {
            generator_task.push_closure(closure);
        }
        self.inherit_generator_task_context(&generator_task, caller_task);
        self.tasks
            .write()
            .insert(generator_task.id(), generator_task.clone());
        match self.run(&generator_task) {
            ExecutionResult::Completed(value) => {
                generator_task.complete(value);
                return Err(VmError::RuntimeError(
                    "Generator completed during call-time initialization unexpectedly".to_string(),
                ));
            }
            ExecutionResult::Suspended(crate::vm::scheduler::SuspendReason::JsGeneratorInit) => {
                generator_task.suspend(crate::vm::scheduler::SuspendReason::JsGeneratorInit);
            }
            ExecutionResult::Suspended(reason) => {
                generator_task.suspend(reason);
                return Err(VmError::RuntimeError(
                    "Generator suspended with an unexpected reason during call-time initialization"
                        .to_string(),
                ));
            }
            ExecutionResult::Failed(error) => {
                generator_task.fail();
                self.ensure_task_exception_for_error(&generator_task, &error);
                if !caller_task.has_exception() {
                    if let Some(exception) = generator_task.current_exception() {
                        caller_task.set_exception(exception);
                    }
                }
                return Err(error);
            }
        }
        let iterator_prototype = self.generator_instance_prototype_for_function(
            closure,
            func_id,
            generator_task.current_module().as_ref(),
            caller_task,
        )?;
        Ok(self.generator_iterator_object(generator_task.id(), iterator_prototype, is_async))
    }
}

pub(in crate::vm::interpreter) fn js_number_to_string(value: f64) -> String {
    fn trim_decimal_suffix(mut rendered: String) -> String {
        if rendered.contains('.') {
            while rendered.ends_with('0') {
                rendered.pop();
            }
            if rendered.ends_with('.') {
                rendered.pop();
            }
        }
        if rendered == "-0" {
            "0".to_string()
        } else {
            rendered
        }
    }

    fn expand_scientific_notation(rendered: &str) -> String {
        let (sign, body) = if let Some(rest) = rendered.strip_prefix('-') {
            ("-", rest)
        } else {
            ("", rendered)
        };
        let Some((mantissa, exponent)) = body.split_once('e') else {
            return rendered.to_string();
        };
        let exponent: i32 = exponent.parse().unwrap_or(0);
        let point_pos = mantissa.find('.').unwrap_or(mantissa.len()) as i32;
        let digits = mantissa.replace('.', "");
        let new_point = point_pos + exponent;

        let mut expanded = if new_point <= 0 {
            format!("0.{}{}", "0".repeat((-new_point) as usize), digits)
        } else if new_point >= digits.len() as i32 {
            format!(
                "{}{}",
                digits,
                "0".repeat((new_point as usize) - digits.len())
            )
        } else {
            let split = new_point as usize;
            format!("{}.{}", &digits[..split], &digits[split..])
        };

        if let Some(dot_idx) = expanded.find('.') {
            while expanded.ends_with('0') {
                expanded.pop();
            }
            if expanded.len() == dot_idx + 1 {
                expanded.pop();
            }
        }

        let expanded = trim_decimal_suffix(expanded);
        if expanded == "0" {
            expanded
        } else {
            format!("{sign}{expanded}")
        }
    }

    if value == 0.0 {
        return "0".to_string();
    }
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value == f64::INFINITY {
        return "Infinity".to_string();
    }
    if value == f64::NEG_INFINITY {
        return "-Infinity".to_string();
    }

    let abs = value.abs();
    let mut rendered = if abs >= 1e21 || abs < 1e-6 {
        let mut scientific = format!("{:e}", value);
        if let Some(exp_idx) = scientific.find('e') {
            let mantissa = &scientific[..exp_idx];
            let exponent = &scientific[exp_idx + 1..];
            let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
            let exponent = if exponent.starts_with('-') || exponent.starts_with('+') {
                exponent.to_string()
            } else {
                format!("+{exponent}")
            };
            scientific = format!("{mantissa}e{exponent}");
        }
        scientific
    } else {
        let rendered = ryu::Buffer::new().format_finite(value).to_string();
        if rendered.contains('e') {
            expand_scientific_notation(&rendered)
        } else {
            trim_decimal_suffix(rendered)
        }
    };
    if let Some(exp_idx) = rendered.find('e') {
        let exponent = &rendered[exp_idx + 1..];
        if !exponent.starts_with('+') && !exponent.starts_with('-') {
            rendered.insert(exp_idx + 1, '+');
        }
    }
    rendered
}

fn primitive_to_js_string(value: Value) -> Option<String> {
    if value.is_undefined() {
        return Some("undefined".to_string());
    }
    if value.is_null() {
        return Some("null".to_string());
    }
    if let Some(value) = value.as_bool() {
        return Some(if value { "true" } else { "false" }.to_string());
    }
    if let Some(value) = value.as_i32() {
        return Some(value.to_string());
    }
    if let Some(value) = value.as_f64() {
        return Some(js_number_to_string(value));
    }
    if let Some(bigint_ptr) = checked_bigint_ptr(value) {
        return Some(unsafe { &*bigint_ptr.as_ptr() }.data.to_string());
    }
    let string_ptr = checked_string_ptr(value)?;
    Some(unsafe { &*string_ptr.as_ptr() }.data.clone())
}

fn boxed_primitive_helper_class_name(class_name: &str) -> Option<&'static str> {
    match class_name {
        "Boolean" => Some("__BooleanPrototype"),
        "BigInt" => Some("__BigIntPrototype"),
        "Number" => Some("__NumberPrototype"),
        "String" => Some("__StringPrototype"),
        _ => None,
    }
}

fn intrinsic_number_constructor_constant(key: &str) -> Option<Value> {
    match key {
        "NaN" => Some(Value::f64(f64::NAN)),
        "POSITIVE_INFINITY" => Some(Value::f64(f64::INFINITY)),
        "NEGATIVE_INFINITY" => Some(Value::f64(f64::NEG_INFINITY)),
        "MAX_VALUE" => Some(Value::f64(1.7976931348623157e308)),
        "MIN_VALUE" => Some(Value::f64(5e-324)),
        "MAX_SAFE_INTEGER" => Some(Value::f64(9007199254740991.0)),
        "MIN_SAFE_INTEGER" => Some(Value::f64(-9007199254740991.0)),
        "EPSILON" => Some(Value::f64(2.220446049250313e-16)),
        _ => None,
    }
}

fn builtin_error_superclass_name(class_name: &str) -> Option<&'static str> {
    match class_name {
        "AggregateError" | "EvalError" | "RangeError" | "ReferenceError" | "SyntaxError"
        | "TypeError" | "URIError" | "InternalError" | "SuppressedError" | "ChannelClosedError"
        | "AssertionError" => Some("Error"),
        _ => None,
    }
}

pub(in crate::vm::interpreter) fn checked_object_ptr(value: Value) -> Option<NonNull<Object>> {
    if !value.is_ptr() || value.is_null() {
        return None;
    }
    let raw_ptr = unsafe { value.as_ptr::<u8>() }?;
    let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };
    if header.type_id() != TypeId::of::<Object>() {
        return None;
    }
    unsafe { value.as_ptr::<Object>() }
}

pub(in crate::vm::interpreter) fn checked_array_ptr(value: Value) -> Option<NonNull<Array>> {
    if !value.is_ptr() || value.is_null() {
        return None;
    }
    let raw_ptr = unsafe { value.as_ptr::<u8>() }?;
    let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };
    if header.type_id() != TypeId::of::<Array>() {
        return None;
    }
    unsafe { value.as_ptr::<Array>() }
}

/// Check if value is a callable Object (has callable data) and return pointer.
pub(in crate::vm::interpreter) fn checked_callable_ptr(value: Value) -> Option<NonNull<Object>> {
    let obj_ptr = checked_object_ptr(value)?;
    let obj = unsafe { &*obj_ptr.as_ptr() };
    if obj.is_callable() {
        Some(obj_ptr)
    } else {
        None
    }
}

pub(in crate::vm::interpreter) fn checked_string_ptr(value: Value) -> Option<NonNull<RayaString>> {
    if !value.is_ptr() || value.is_null() {
        return None;
    }
    let raw_ptr = unsafe { value.as_ptr::<u8>() }?;
    let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };
    if header.type_id() != TypeId::of::<RayaString>() {
        return None;
    }
    unsafe { value.as_ptr::<RayaString>() }
}

pub(in crate::vm::interpreter) fn checked_bigint_ptr(value: Value) -> Option<NonNull<RayaBigInt>> {
    if !value.is_ptr() || value.is_null() {
        return None;
    }
    let raw_ptr = unsafe { value.as_ptr::<u8>() }?;
    let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };
    if header.type_id() != TypeId::of::<RayaBigInt>() {
        return None;
    }
    unsafe { value.as_ptr::<RayaBigInt>() }
}

/// Check if value is a callable Object and return pointer (alias for checked_callable_ptr).
pub(in crate::vm::interpreter) fn checked_closure_ptr(value: Value) -> Option<NonNull<Object>> {
    checked_callable_ptr(value)
}

fn value_same_value(a: Value, b: Value) -> bool {
    if a.is_ptr() && b.is_ptr() {
        if let (Some(a_ptr), Some(b_ptr)) = (checked_bigint_ptr(a), checked_bigint_ptr(b)) {
            return unsafe { &*a_ptr.as_ptr() }.data == unsafe { &*b_ptr.as_ptr() }.data;
        }
        let a_str = checked_string_ptr(a);
        let b_str = checked_string_ptr(b);
        if let (Some(a_ptr), Some(b_ptr)) = (a_str, b_str) {
            let a_ref = unsafe { &*a_ptr.as_ptr() };
            let b_ref = unsafe { &*b_ptr.as_ptr() };
            return a_ref.data == b_ref.data;
        }
        return a.raw() == b.raw();
    }

    let a_num = a.as_f64().or_else(|| a.as_i32().map(|v| v as f64));
    let b_num = b.as_f64().or_else(|| b.as_i32().map(|v| v as f64));
    if let (Some(a_num), Some(b_num)) = (a_num, b_num) {
        if a_num.is_nan() && b_num.is_nan() {
            return true;
        }
        if a_num == 0.0 && b_num == 0.0 {
            let a_bits = a.as_f64().map(f64::to_bits).unwrap_or(0.0f64.to_bits());
            let b_bits = b.as_f64().map(f64::to_bits).unwrap_or(0.0f64.to_bits());
            return a_bits == b_bits;
        }
        return a_num == b_num;
    }

    a.raw() == b.raw()
}

#[inline]
fn native_arg(args: &[Value], index: usize) -> Value {
    args.get(index).copied().unwrap_or(Value::undefined())
}

fn is_uri_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~')
}

fn parse_js_array_index_name(key: &str) -> Option<usize> {
    if key.is_empty() {
        return None;
    }
    if key != "0" && key.starts_with('0') {
        return None;
    }
    let index = key.parse::<u32>().ok()?;
    if index == u32::MAX || index.to_string() != key {
        return None;
    }
    Some(index as usize)
}

fn percent_encode_uri_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        if is_uri_unreserved(byte) {
            out.push(byte as char);
        } else {
            use std::fmt::Write;
            let _ = write!(&mut out, "%{:02X}", byte);
        }
    }
    out
}

fn percent_decode_uri_component(input: &str) -> Result<String, VmError> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(VmError::RuntimeError(
                    "Malformed percent-encoding".to_string(),
                ));
            }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                .map_err(|_| VmError::RuntimeError("Malformed percent-encoding".to_string()))?;
            let byte = u8::from_str_radix(hex, 16)
                .map_err(|_| VmError::RuntimeError("Malformed percent-encoding".to_string()))?;
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| VmError::RuntimeError("Invalid UTF-8".to_string()))
}

fn object_to_string_tag_from_class_name(class_name: &str) -> &'static str {
    match class_name {
        "Array" => "Array",
        "Function" | "AsyncFunction" | "GeneratorFunction" | "AsyncGeneratorFunction" => "Function",
        "String" => "String",
        "Number" => "Number",
        "Boolean" => "Boolean",
        "Symbol" => "Symbol",
        "Date" => "Date",
        "RegExp" => "RegExp",
        "Error" | "TypeError" | "RangeError" | "ReferenceError" | "SyntaxError" | "URIError"
        | "EvalError" | "InternalError" | "AggregateError" | "SuppressedError"
        | "ChannelClosedError" | "AssertionError" => "Error",
        "Map" => "Map",
        "Set" => "Set",
        "WeakMap" => "WeakMap",
        "WeakSet" => "WeakSet",
        "WeakRef" => "WeakRef",
        "FinalizationRegistry" => "FinalizationRegistry",
        "ArrayBuffer" | "SharedArrayBuffer" => "ArrayBuffer",
        "DataView" => "DataView",
        _ => "Object",
    }
}

impl<'a> Interpreter<'a> {
    fn alloc_string_value(&self, value: impl Into<String>) -> Value {
        let gc_ptr = self.gc.lock().allocate(RayaString::new(value.into()));
        unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).expect("string ptr")) }
    }

    pub(in crate::vm::interpreter) fn alloc_builtin_error_value(
        &self,
        class_name: &str,
        message: &str,
    ) -> Value {
        let member_names = vec![
            "message".to_string(),
            "name".to_string(),
            "stack".to_string(),
            "cause".to_string(),
            "code".to_string(),
            "errno".to_string(),
            "syscall".to_string(),
            "path".to_string(),
            "errors".to_string(),
        ];
        let layout_id = layout_id_from_ordered_names(&member_names);
        self.register_structural_layout_shape(layout_id, &member_names);
        let object_ptr = self
            .gc
            .lock()
            .allocate(Object::new_dynamic(layout_id, member_names.len()));
        let object_value = unsafe {
            Value::from_ptr(std::ptr::NonNull::new(object_ptr.as_ptr()).expect("error object ptr"))
        };

        // Set prototype from constructor (e.g., TypeError.prototype)
        let constructor_value = self.builtin_global_value(class_name);
        if let Some(constructor) = constructor_value {
            self.set_constructed_object_prototype_from_constructor(object_value, constructor);
            let _ = self.define_data_property_on_target(
                object_value,
                "constructor",
                constructor,
                true,
                false,
                true,
            );
        }

        // Set nominal_type_id so `instanceof` works in all modes (not just JS prototype chain)
        if let Some(nominal_type_id) = self.builtin_class_nominal_type_id(class_name) {
            let obj = unsafe { &mut *object_ptr.as_ptr() };
            obj.set_nominal_type_id(Some(nominal_type_id as u32));
        }

        // Directly write fields — we just created the object with known layout
        let name_value = self.alloc_string_value(class_name);
        let message_value = self.alloc_string_value(message);
        let empty_string = self.alloc_string_value("");
        let errors_ptr = self.gc.lock().allocate(Array::new(0, 0));
        let errors_value =
            unsafe { Value::from_ptr(NonNull::new(errors_ptr.as_ptr()).expect("array ptr")) };
        let obj = unsafe { &mut *object_ptr.as_ptr() };
        let _ = obj.set_field(0, message_value); // slot 0 = "message"
        let _ = obj.set_field(1, name_value); // slot 1 = "name"
        let _ = obj.set_field(2, empty_string); // slot 2 = "stack"
        let _ = obj.set_field(3, Value::null()); // slot 3 = "cause"
        let _ = obj.set_field(4, empty_string); // slot 4 = "code"
        let _ = obj.set_field(5, Value::i32(0)); // slot 5 = "errno"
        let _ = obj.set_field(6, empty_string); // slot 6 = "syscall"
        let _ = obj.set_field(7, empty_string); // slot 7 = "path"
        let _ = obj.set_field(8, errors_value); // slot 8 = "errors"

        object_value
    }

    fn raise_task_builtin_error(
        &self,
        task: &Arc<Task>,
        class_name: &str,
        message: impl Into<String>,
    ) -> VmError {
        let message = message.into();
        if !task.has_exception() {
            let exception = self.alloc_builtin_error_value(class_name, &message);
            task.set_exception(exception);
        }
        match class_name {
            "TypeError" => VmError::TypeError(message),
            "SyntaxError" => VmError::SyntaxError(message),
            "RangeError" => VmError::RangeError(message),
            "ReferenceError" => VmError::ReferenceError(message),
            _ => VmError::RuntimeError(message),
        }
    }

    fn replace_task_builtin_error(
        &self,
        task: &Arc<Task>,
        class_name: &str,
        message: impl Into<String>,
    ) -> VmError {
        let message = message.into();
        let exception = self.alloc_builtin_error_value(class_name, &message);
        task.set_exception(exception);
        match class_name {
            "TypeError" => VmError::TypeError(message),
            "SyntaxError" => VmError::SyntaxError(message),
            "RangeError" => VmError::RangeError(message),
            "ReferenceError" => VmError::ReferenceError(message),
            _ => VmError::RuntimeError(message),
        }
    }

    pub(in crate::vm::interpreter) fn ensure_task_exception_for_error(
        &self,
        task: &Arc<Task>,
        error: &VmError,
    ) {
        if task.has_exception() {
            return;
        }
        let exception = match error {
            VmError::TypeError(message) => self.alloc_builtin_error_value("TypeError", message),
            VmError::SyntaxError(message) => self.alloc_builtin_error_value("SyntaxError", message),
            VmError::RangeError(message) => self.alloc_builtin_error_value("RangeError", message),
            VmError::ReferenceError(message) => {
                self.alloc_builtin_error_value("ReferenceError", message)
            }
            VmError::RuntimeError(message) => self.alloc_builtin_error_value("Error", message),
            _ => {
                let gc_ptr = self.gc.lock().allocate(RayaString::new(error.to_string()));
                unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
            }
        };
        self.ephemeral_gc_roots.write().push(exception);
        task.set_exception(exception);
    }

    fn builtin_error_layout_fields(class_name: &str) -> Option<&'static [&'static str]> {
        const ERROR_FIELDS: &[&str] = &[
            "message", "name", "stack", "cause", "code", "errno", "syscall", "path",
        ];
        const AGGREGATE_ERROR_FIELDS: &[&str] = &[
            "message", "name", "stack", "cause", "code", "errno", "syscall", "path", "errors",
        ];

        match class_name {
            "Error" | "TypeError" | "RangeError" | "ReferenceError" | "SyntaxError"
            | "URIError" | "EvalError" | "InternalError" | "SuppressedError"
            | "ChannelClosedError" | "AssertionError" => Some(ERROR_FIELDS),
            "AggregateError" => Some(AGGREGATE_ERROR_FIELDS),
            _ => None,
        }
    }

    fn ensure_builtin_error_class_layout(&self, class_name: &str) -> Option<usize> {
        let required_fields = Self::builtin_error_layout_fields(class_name)?;
        let id = self
            .classes
            .read()
            .get_class_by_name(class_name)
            .map(|class| class.id)?;

        if self
            .nominal_allocation(id)
            .is_some_and(|(_, field_count)| field_count < required_fields.len())
        {
            self.set_nominal_field_count(id, required_fields.len());
        }

        let mut metadata = self.class_metadata.write();
        let meta = metadata.get_or_create(id);
        for (index, field_name) in required_fields.iter().enumerate() {
            meta.add_field((*field_name).to_string(), index);
        }
        Some(id)
    }

    fn ensure_builtin_error_class_layout_for_nominal_type(
        &self,
        nominal_type_id: usize,
    ) -> Option<usize> {
        let class_name = self
            .classes
            .read()
            .get_class(nominal_type_id)
            .map(|class| class.name.clone())?;
        self.ensure_builtin_error_class_layout(&class_name)
            .or(Some(nominal_type_id))
    }

    fn construct_ordinary_callable(
        &mut self,
        constructor: Value,
        new_target: Value,
        args: &[Value],
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Value, VmError> {
        let member_names: Vec<String> = Vec::new();
        let layout_id = layout_id_from_ordered_names(&member_names);
        let object_ptr = self.gc.lock().allocate(Object::new_dynamic(layout_id, 0));
        let object_value =
            unsafe { Value::from_ptr(NonNull::new(object_ptr.as_ptr()).expect("object ptr")) };
        self.set_constructed_object_prototype_from_constructor(object_value, new_target);
        let returned = self.invoke_callable_sync_with_this_and_new_target(
            constructor,
            Some(object_value),
            Some(new_target),
            args,
            task,
            module,
        )?;
        Ok(self.constructor_result_or_receiver(returned, object_value))
    }

    pub(crate) fn constructor_result_or_receiver(&self, returned: Value, receiver: Value) -> Value {
        if self.is_js_object_value(returned) {
            returned
        } else {
            receiver
        }
    }

    fn super_this_is_initialized(&self, value: Value) -> bool {
        self.get_own_js_property_value_by_name(value, SUPER_THIS_INITIALIZED_KEY)
            .is_some_and(|value| value.is_truthy())
    }

    fn mark_super_this_initialized(&self, value: Value) -> Result<(), VmError> {
        if !self.is_js_object_value(value) {
            return Ok(());
        }
        self.define_data_property_on_target(
            value,
            SUPER_THIS_INITIALIZED_KEY,
            Value::bool(true),
            true,
            false,
            true,
        )
    }

    fn get_prototype_from_constructor_with_fallback(
        &mut self,
        constructor: Value,
        intrinsic_default_prototype: Option<Value>,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Option<Value>, VmError> {
        match self.get_property_value_via_js_semantics_with_context(
            constructor,
            "prototype",
            task,
            module,
        )? {
            Some(prototype) if self.is_js_object_value(prototype) => Ok(Some(prototype)),
            _ => Ok(intrinsic_default_prototype.filter(|value| self.is_js_object_value(*value))),
        }
    }

    fn construct_builtin_object(
        &mut self,
        new_target: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Value, VmError> {
        let member_names: Vec<String> = Vec::new();
        let layout_id = layout_id_from_ordered_names(&member_names);
        let object_ptr = self.gc.lock().allocate(Object::new_dynamic(layout_id, 0));
        let object_value =
            unsafe { Value::from_ptr(NonNull::new(object_ptr.as_ptr()).expect("object ptr")) };
        let intrinsic_default_prototype = self
            .builtin_global_value("Object")
            .and_then(|ctor| self.object_constructor_prototype_value(ctor));
        if let Some(prototype) = self.get_prototype_from_constructor_with_fallback(
            new_target,
            intrinsic_default_prototype,
            task,
            module,
        )? {
            self.set_constructed_object_prototype_from_value(object_value, prototype);
        }
        Ok(object_value)
    }

    fn set_constructed_value_prototype_from_constructor(&self, value: Value, constructor: Value) {
        if let Some(prototype) = self.constructed_object_prototype_from_constructor(constructor) {
            self.set_explicit_object_prototype(value, prototype);
        }
    }

    fn construct_builtin_array(
        &mut self,
        new_target: Value,
        args: &[Value],
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Value, VmError> {
        let array_ptr = self.gc.lock().allocate(Array::new(0, 0));
        let array_value =
            unsafe { Value::from_ptr(NonNull::new(array_ptr.as_ptr()).expect("array ptr")) };
        let intrinsic_default_prototype = self
            .builtin_global_value("Array")
            .and_then(|ctor| self.array_constructor_prototype_value(ctor));
        if let Some(prototype) = self.get_prototype_from_constructor_with_fallback(
            new_target,
            intrinsic_default_prototype,
            task,
            module,
        )? {
            self.set_constructed_object_prototype_from_value(array_value, prototype);
        }

        let array = unsafe { &mut *array_ptr.as_ptr() };
        if args.len() == 1 {
            if let Some(len) = self.js_array_constructor_length_from_value(args[0])? {
                array.resize_holey(len);
            } else {
                array.set(0, args[0]).map_err(VmError::RuntimeError)?;
            }
            return Ok(array_value);
        }

        for (index, value) in args.iter().copied().enumerate() {
            array.set(index, value).map_err(VmError::RuntimeError)?;
        }

        Ok(array_value)
    }

    pub(in crate::vm::interpreter) fn construct_value_with_new_target(
        &mut self,
        constructor: Value,
        new_target: Value,
        args: &[Value],
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Value, VmError> {
        if !self.callable_is_constructible(constructor) {
            return Err(VmError::TypeError("Value is not a constructor".to_string()));
        }
        if !self.callable_is_constructible(new_target) {
            return Err(VmError::TypeError("Value is not a constructor".to_string()));
        }

        if let Some(raw_ptr) = unsafe { constructor.as_ptr::<u8>() } {
            let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };
            if header.type_id() == std::any::TypeId::of::<Object>() {
                let co = unsafe { &*constructor.as_ptr::<Object>().unwrap().as_ptr() };
                if let Some(ref callable) = co.callable {
                    if let CallableKind::Bound {
                        target, bound_args, ..
                    } = &callable.kind
                    {
                        let mut combined_args = bound_args.clone();
                        combined_args.extend_from_slice(args);
                        let adjusted_new_target = if constructor.raw() == new_target.raw() {
                            *target
                        } else {
                            new_target
                        };
                        return self.construct_value_with_new_target(
                            *target,
                            adjusted_new_target,
                            &combined_args,
                            task,
                            module,
                        );
                    }
                }
            }
        }

        if let Some(value) = self.try_construct_boxed_primitive(constructor, args, task, module)? {
            return Ok(value);
        }

        if self
            .builtin_global_value("Array")
            .is_some_and(|builtin| builtin.raw() == constructor.raw())
        {
            return self.construct_builtin_array(new_target, args, task, module);
        }

        if self
            .builtin_global_value("Promise")
            .is_some_and(|builtin| builtin.raw() == constructor.raw())
        {
            return self.construct_builtin_promise(args, task, module);
        }

        if self
            .builtin_global_value("Object")
            .is_some_and(|builtin| builtin.raw() == constructor.raw())
        {
            return self.construct_builtin_object(new_target, task, module);
        }

        let constructor_nominal_type_id = self
            .js_callable_builtin_constructor_name(constructor)
            .and_then(|class_name| self.ensure_builtin_error_class_layout(class_name))
            .or_else(|| self.constructor_nominal_type_id(constructor))
            .or_else(|| self.nominal_type_id_from_imported_class_value(module, constructor))
            .and_then(|id| self.ensure_builtin_error_class_layout_for_nominal_type(id));
        if let Some(constructor_nominal_type_id) = constructor_nominal_type_id {
            let allocation_nominal_type_id = self
                .constructor_nominal_type_id(new_target)
                .or_else(|| self.nominal_type_id_from_imported_class_value(module, new_target))
                .unwrap_or(constructor_nominal_type_id);
            let obj_val = self.alloc_nominal_instance_value(allocation_nominal_type_id)?;
            self.ephemeral_gc_roots.write().push(obj_val);

            let prototype = match self.get_property_value_via_js_semantics_with_context(
                new_target,
                "prototype",
                task,
                module,
            )? {
                Some(prototype) if self.is_js_object_value(prototype) => Some(prototype),
                _ => self.constructor_prototype_value(constructor),
            };
            if let Some(prototype) = prototype {
                self.set_constructed_object_prototype_from_value(obj_val, prototype);
            }

            let (constructor_id, constructor_module) = {
                let classes = self.classes.read();
                let class = classes
                    .get_class(constructor_nominal_type_id)
                    .ok_or_else(|| {
                        VmError::RuntimeError(format!(
                            "Class {} not found",
                            constructor_nominal_type_id
                        ))
                    })?;
                (class.get_constructor(), class.module.clone())
            };

            if let Some(constructor_id) = constructor_id {
                let closure = if let Some(module) = constructor_module {
                    Object::new_closure_with_module(constructor_id, Vec::new(), module)
                } else {
                    Object::new_closure(constructor_id, Vec::new())
                };
                let closure_ptr = self.gc.lock().allocate(closure);
                let closure_val = unsafe {
                    Value::from_ptr(
                        std::ptr::NonNull::new(closure_ptr.as_ptr())
                            .expect("constructor closure ptr"),
                    )
                };
                self.ephemeral_gc_roots.write().push(closure_val);

                let mut invoke_args = Vec::with_capacity(args.len() + 1);
                invoke_args.push(obj_val);
                invoke_args.extend_from_slice(args);
                let invoke_result =
                    self.invoke_callable_sync(closure_val, &invoke_args, task, module);
                {
                    let mut ephemeral = self.ephemeral_gc_roots.write();
                    if let Some(index) = ephemeral
                        .iter()
                        .rposition(|candidate| *candidate == closure_val)
                    {
                        ephemeral.swap_remove(index);
                    }
                }
                let returned = invoke_result?;
                {
                    let mut ephemeral = self.ephemeral_gc_roots.write();
                    if let Some(index) = ephemeral
                        .iter()
                        .rposition(|candidate| *candidate == obj_val)
                    {
                        ephemeral.swap_remove(index);
                    }
                }
                return Ok(self.constructor_result_or_receiver(returned, obj_val));
            }

            {
                let mut ephemeral = self.ephemeral_gc_roots.write();
                if let Some(index) = ephemeral
                    .iter()
                    .rposition(|candidate| *candidate == obj_val)
                {
                    ephemeral.swap_remove(index);
                }
            }

            return Ok(obj_val);
        }

        if self.callable_function_info(constructor).is_some()
            && self
                .constructor_nominal_type_id(constructor)
                .or_else(|| self.nominal_type_id_from_imported_class_value(module, constructor))
                .is_none()
        {
            return self.construct_ordinary_callable(constructor, new_target, args, task, module);
        }

        let constructor_name = self
            .callable_function_info(constructor)
            .map(|(name, _)| name)
            .or_else(|| self.builtin_global_name_for_value(constructor))
            .unwrap_or_else(|| "<unknown>".to_string());
        let new_target_name = self
            .callable_function_info(new_target)
            .map(|(name, _)| name)
            .or_else(|| self.builtin_global_name_for_value(new_target))
            .unwrap_or_else(|| "<unknown>".to_string());
        Err(VmError::TypeError(format!(
            "Value is not a supported constructor (ctor={constructor_name}, newTarget={new_target_name})"
        )))
    }

    fn construct_value_with_existing_receiver_and_new_target(
        &mut self,
        receiver: Value,
        constructor: Value,
        new_target: Value,
        args: &[Value],
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Value, VmError> {
        if !self.is_js_object_value(receiver) {
            return self.construct_value_with_new_target(
                constructor,
                new_target,
                args,
                task,
                module,
            );
        }

        let was_initialized = self.super_this_is_initialized(receiver);

        if !self.callable_is_constructible(constructor) {
            return Err(VmError::TypeError("Value is not a constructor".to_string()));
        }
        if !self.callable_is_constructible(new_target) {
            return Err(VmError::TypeError("Value is not a constructor".to_string()));
        }

        if let Some(raw_ptr) = unsafe { constructor.as_ptr::<u8>() } {
            let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };
            if header.type_id() == std::any::TypeId::of::<Object>() {
                let co = unsafe { &*constructor.as_ptr::<Object>().unwrap().as_ptr() };
                if let Some(ref callable) = co.callable {
                    if let CallableKind::Bound {
                        target, bound_args, ..
                    } = &callable.kind
                    {
                        let mut combined_args = bound_args.clone();
                        combined_args.extend_from_slice(args);
                        let adjusted_new_target = if constructor.raw() == new_target.raw() {
                            *target
                        } else {
                            new_target
                        };
                        return self.construct_value_with_existing_receiver_and_new_target(
                            receiver,
                            *target,
                            adjusted_new_target,
                            &combined_args,
                            task,
                            module,
                        );
                    }
                }
            }
        }

        let constructor_nominal_type_id = self
            .constructor_nominal_type_id(constructor)
            .or_else(|| self.nominal_type_id_from_imported_class_value(module, constructor));
        if let Some(constructor_nominal_type_id) = constructor_nominal_type_id {
            let prototype = match self.get_property_value_via_js_semantics_with_context(
                new_target,
                "prototype",
                task,
                module,
            )? {
                Some(prototype) if self.is_js_object_value(prototype) => Some(prototype),
                _ => self.constructor_prototype_value(constructor),
            };
            if let Some(prototype) = prototype {
                self.set_constructed_object_prototype_from_value(receiver, prototype);
            }

            let (constructor_id, constructor_module) = {
                let classes = self.classes.read();
                let class = classes
                    .get_class(constructor_nominal_type_id)
                    .ok_or_else(|| {
                        VmError::RuntimeError(format!(
                            "Class {} not found",
                            constructor_nominal_type_id
                        ))
                    })?;
                (class.get_constructor(), class.module.clone())
            };

            if let Some(constructor_id) = constructor_id {
                let closure = if let Some(module) = constructor_module {
                    Object::new_closure_with_module(constructor_id, Vec::new(), module)
                } else {
                    Object::new_closure(constructor_id, Vec::new())
                };
                let closure_ptr = self.gc.lock().allocate(closure);
                let closure_val = unsafe {
                    Value::from_ptr(
                        std::ptr::NonNull::new(closure_ptr.as_ptr())
                            .expect("constructor closure ptr"),
                    )
                };
                self.ephemeral_gc_roots.write().push(closure_val);
                self.ephemeral_gc_roots.write().push(receiver);

                let mut invoke_args = Vec::with_capacity(args.len() + 1);
                invoke_args.push(receiver);
                invoke_args.extend_from_slice(args);
                let invoke_result =
                    self.invoke_callable_sync(closure_val, &invoke_args, task, module);
                {
                    let mut ephemeral = self.ephemeral_gc_roots.write();
                    if let Some(index) = ephemeral
                        .iter()
                        .rposition(|candidate| *candidate == closure_val)
                    {
                        ephemeral.swap_remove(index);
                    }
                }
                let returned = invoke_result?;
                {
                    let mut ephemeral = self.ephemeral_gc_roots.write();
                    if let Some(index) = ephemeral
                        .iter()
                        .rposition(|candidate| *candidate == receiver)
                    {
                        ephemeral.swap_remove(index);
                    }
                }
                let constructed = self.constructor_result_or_receiver(returned, receiver);
                self.mark_super_this_initialized(constructed)?;
                if was_initialized {
                    return Err(self.raise_task_builtin_error(
                        task,
                        "ReferenceError",
                        "Super constructor may only be called once".to_string(),
                    ));
                }
                return Ok(constructed);
            }

            self.mark_super_this_initialized(receiver)?;
            if was_initialized {
                return Err(self.raise_task_builtin_error(
                    task,
                    "ReferenceError",
                    "Super constructor may only be called once".to_string(),
                ));
            }
            return Ok(receiver);
        }

        self.construct_value_with_new_target(constructor, new_target, args, task, module)
    }

    pub(in crate::vm::interpreter) fn call_builtin_constructor_as_function(
        &mut self,
        callable: Value,
        args: &[Value],
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Option<Value>, VmError> {
        let Some(name) = self.js_callable_builtin_constructor_name(callable) else {
            return Ok(None);
        };

        match name {
            "Object" => {
                let first = args.first().copied().unwrap_or(Value::undefined());
                if first.is_null() || first.is_undefined() {
                    return self
                        .construct_builtin_object(callable, task, module)
                        .map(Some);
                }
                if self.is_symbol_value(first) {
                    if let Some(constructor) = self.builtin_global_value("Symbol") {
                        return self
                            .alloc_boxed_primitive_object(constructor, "Symbol", first)
                            .map(Some);
                    }
                }
                if self.is_js_object_value(first) || Self::is_callable_value(first) {
                    return Ok(Some(first));
                }
                if let Some(boxed) = self.box_js_this_primitive(first)? {
                    return Ok(Some(boxed));
                }
                self.construct_builtin_object(callable, task, module)
                    .map(Some)
            }
            "Array" => self
                .construct_builtin_array(callable, args, task, module)
                .map(Some),
            "Date" => {
                let date_value =
                    self.construct_value_with_new_target(callable, callable, args, task, module)?;
                let to_string = self
                    .get_property_value_via_js_semantics_with_context(
                        date_value, "toString", task, module,
                    )?
                    .ok_or_else(|| {
                        VmError::TypeError(
                            "Date ordinary call requires a callable toString method".to_string(),
                        )
                    })?;
                self.invoke_callable_sync_with_this(to_string, Some(date_value), &[], task, module)
                    .map(Some)
            }
            // Number(value) — ES spec: ToNumber coercion, returns primitive
            "Number" => {
                let first = args.first().copied().unwrap_or(Value::i32(0));
                let n = self.js_to_number_from_primitive(first)?;
                Ok(Some(if n == (n as i32) as f64 && !n.is_nan() {
                    Value::i32(n as i32)
                } else {
                    Value::f64(n)
                }))
            }
            // String(value) — ES spec: ToString coercion, returns string
            "String" => {
                let first = args.first().copied().unwrap_or(Value::undefined());
                let s = self.js_function_argument_to_string(first, task, module)?;
                Ok(Some(self.alloc_string_value(&s)))
            }
            // Boolean(value) — ES spec: ToBoolean coercion, returns primitive
            "Boolean" => {
                let first = args.first().copied().unwrap_or(Value::undefined());
                Ok(Some(Value::bool(first.is_truthy())))
            }
            // Symbol(desc) — creates a new symbol
            "Symbol" => {
                // Ordinary Symbol() calls create a new unique symbol value directly.
                // They do not use constructor/new-target semantics.
                let description = match args.first().copied().unwrap_or(Value::undefined()) {
                    value if value.is_null() || value.is_undefined() => String::new(),
                    value => self.js_function_argument_to_string(value, task, module)?,
                };
                self.alloc_symbol_object(&description).map(Some)
            }
            // Error constructors: calling without `new` behaves as `new`
            "Error" | "TypeError" | "RangeError" | "ReferenceError" | "SyntaxError"
            | "URIError" | "EvalError" | "AggregateError" => self
                .construct_value_with_new_target(callable, callable, args, task, module)
                .map(Some),
            // RegExp(pattern, flags) without `new` behaves as `new`
            "RegExp" => self
                .construct_value_with_new_target(callable, callable, args, task, module)
                .map(Some),
            // Collections: calling without `new` should throw TypeError per spec,
            // but many test262 patterns use `.call()` so we delegate to construct.
            "Map" | "Set" | "WeakMap" | "WeakSet" | "Promise" | "ArrayBuffer" | "DataView"
            | "Int8Array" | "Uint8Array" | "Int16Array" | "Uint16Array" | "Int32Array"
            | "Uint32Array" | "Float32Array" | "Float64Array" | "BigInt64Array"
            | "BigUint64Array" | "Uint8ClampedArray" => self
                .construct_value_with_new_target(callable, callable, args, task, module)
                .map(Some),
            "Function" => {
                // Function() as a function call creates a new function
                self.construct_value_with_new_target(callable, callable, args, task, module)
                    .map(Some)
            }
            _ => Ok(None),
        }
    }

    fn object_to_string_tag(&self, value: Value) -> &'static str {
        if value.is_undefined() {
            return "Undefined";
        }
        if value.is_null() {
            return "Null";
        }
        if value.as_bool().is_some() {
            return "Boolean";
        }
        if value.as_i32().is_some() || value.as_f64().is_some() {
            return "Number";
        }
        if checked_string_ptr(value).is_some() {
            return "String";
        }
        if checked_array_ptr(value).is_some() {
            return "Array";
        }
        if self.callable_function_info(value).is_some() {
            return "Function";
        }
        if let Some(class_name) = self.nominal_class_name_for_value(value) {
            return object_to_string_tag_from_class_name(&class_name);
        }
        "Object"
    }

    fn seed_builtin_error_prototype_properties(
        &self,
        prototype_val: Value,
        class_name: &str,
    ) -> Option<()> {
        let name = match class_name {
            "Error" | "AggregateError" | "EvalError" | "RangeError" | "ReferenceError"
            | "SyntaxError" | "TypeError" | "URIError" => class_name,
            _ => return Some(()),
        };

        self.define_data_property_on_target(
            prototype_val,
            "name",
            self.alloc_string_value(name),
            true,
            false,
            true,
        )
        .ok()?;

        self.define_data_property_on_target(
            prototype_val,
            "message",
            self.alloc_string_value(String::new()),
            true,
            false,
            true,
        )
        .ok()?;

        Some(())
    }

    fn normalize_dynamic_value(&self, value: Value) -> Value {
        use crate::vm::json::view::{js_classify, JSView};

        match js_classify(value) {
            JSView::Arr(ptr) => {
                let (type_id, elements) = unsafe { ((*ptr).type_id, (*ptr).elements.clone()) };
                let mut array = Array::new(type_id, elements.len());
                for (index, element) in elements.into_iter().enumerate() {
                    let normalized = self.normalize_dynamic_value(element);
                    let _ = array.set(index, normalized);
                }
                let gc_ptr = self.gc.lock().allocate(array);
                unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
            }
            _ => value,
        }
    }

    fn collect_dynamic_entries(&self, value: Value) -> Vec<(String, Value)> {
        use crate::vm::json::view::{js_classify, JSView};

        match js_classify(value) {
            JSView::Struct { ptr, .. } => {
                let obj = unsafe { &*ptr };
                let mut entries = Vec::new();
                let mut fixed_entries_added = false;

                if let Some(nominal_type_id) = obj.nominal_type_id_usize() {
                    let class_metadata = self.class_metadata.read();
                    if let Some(meta) = class_metadata.get(nominal_type_id) {
                        for (index, name) in meta.field_names.iter().enumerate() {
                            if name.is_empty() || index >= obj.field_count() {
                                continue;
                            }
                            if let Some(value) = obj.get_field(index) {
                                entries.push((name.clone(), self.normalize_dynamic_value(value)));
                            }
                        }
                        fixed_entries_added = true;
                    }
                }
                if !fixed_entries_added {
                    if let Some(layout_names) = self.layout_field_names_for_object(obj) {
                        for (index, name) in layout_names.into_iter().enumerate() {
                            if index >= obj.field_count() {
                                break;
                            }
                            if let Some(value) = obj.get_field(index) {
                                entries.push((name, self.normalize_dynamic_value(value)));
                            }
                        }
                    }
                }

                if let Some(dp) = obj.dyn_props() {
                    for key in dp.keys_in_order() {
                        let Some(prop) = dp.get(key) else {
                            continue;
                        };
                        let Some(name) = self.prop_key_name(key) else {
                            continue;
                        };
                        if entries.iter().any(|(existing, _)| existing == &name) {
                            continue;
                        }
                        entries.push((name, self.normalize_dynamic_value(prop.value)));
                    }
                }

                entries
            }
            _ => Vec::new(),
        }
    }

    fn merge_dynamic_entries_into(&self, target: Value, entries: &[(String, Value)]) {
        use crate::vm::json::view::{js_classify, JSView};

        match js_classify(target) {
            JSView::Struct { ptr, .. } => {
                let obj = unsafe { &mut *(ptr as *mut Object) };
                for (key, value) in entries {
                    if let Some(index) = self.get_field_index_for_value(target, key) {
                        let _ = obj.set_field(index, *value);
                    } else {
                        obj.ensure_dyn_props()
                            .insert(self.intern_prop_key(key), DynProp::data(*value));
                    }
                }
            }
            _ => {}
        }
    }

    fn legacy_object_literal_field_index(field_name: &str, field_count: usize) -> Option<usize> {
        let idx = match field_name {
            // Error-like object literal layout: [message, name, stack, cause, ...]
            "message" => 0,
            "name" => 1,
            "stack" => 2,
            "cause" => 3,
            "code" => 4,
            "errno" => 5,
            "syscall" => 6,
            "path" => 7,
            "errors" => 8,
            // Node-compat descriptor Object layout: [value, writable, configurable, enumerable, get, set]
            "value" => 0,
            "writable" => 1,
            "configurable" => 2,
            "enumerable" => 3,
            "get" => 4,
            "set" => 5,
            _ => return None,
        };
        (idx < field_count).then_some(idx)
    }

    pub(in crate::vm::interpreter) fn is_callable_value(value: Value) -> bool {
        if let Some(obj_ptr) = checked_object_ptr(value) {
            let obj = unsafe { &*obj_ptr.as_ptr() };
            return obj.is_callable();
        }
        false
    }

    pub(in crate::vm::interpreter) fn js_callable_builtin_constructor_name(
        &self,
        value: Value,
    ) -> Option<&'static str> {
        let value = self
            .unwrapped_proxy_like(value)
            .map(|proxy| proxy.target)
            .unwrap_or(value);
        // All standard built-in constructors that JS code may call as functions
        // (via `Constructor(value)` or `Function.prototype.call(Constructor, ...)`).
        static CALLABLE_CONSTRUCTORS: &[&str] = &[
            "Object",
            "Array",
            "Date",
            "Number",
            "String",
            "Boolean",
            "Error",
            "TypeError",
            "RangeError",
            "ReferenceError",
            "SyntaxError",
            "URIError",
            "EvalError",
            "AggregateError",
            "Function",
            "RegExp",
            "Map",
            "Set",
            "WeakMap",
            "WeakSet",
            "Promise",
            "Symbol",
            "ArrayBuffer",
            "DataView",
            "Int8Array",
            "Uint8Array",
            "Int16Array",
            "Uint16Array",
            "Int32Array",
            "Uint32Array",
            "Float32Array",
            "Float64Array",
            "BigInt64Array",
            "BigUint64Array",
            "Uint8ClampedArray",
        ];
        for &name in CALLABLE_CONSTRUCTORS {
            if self
                .builtin_global_value(name)
                .is_some_and(|builtin| builtin.raw() == value.raw())
            {
                return Some(name);
            }
        }
        None
    }

    pub(in crate::vm::interpreter) fn js_call_target_supported(&self, value: Value) -> bool {
        Self::is_callable_value(value) || self.js_callable_builtin_constructor_name(value).is_some()
    }

    pub(in crate::vm::interpreter) fn proxy_wrapper_proxy_value(
        &self,
        value: Value,
    ) -> Option<Value> {
        let object_ptr = checked_object_ptr(value)?;
        let object = unsafe { &*object_ptr.as_ptr() };
        let proxy_value = self.get_object_named_field_value(object, "_proxy")?;
        crate::vm::reflect::try_unwrap_proxy(proxy_value)?;
        Some(proxy_value)
    }

    pub(in crate::vm::interpreter) fn unwrapped_proxy_like(
        &self,
        value: Value,
    ) -> Option<crate::vm::reflect::UnwrappedProxy> {
        crate::vm::reflect::try_unwrap_proxy(value).or_else(|| {
            let proxy_value = self.proxy_wrapper_proxy_value(value)?;
            crate::vm::reflect::try_unwrap_proxy(proxy_value)
        })
    }

    pub(in crate::vm::interpreter) fn explicit_object_prototype(
        &self,
        value: Value,
    ) -> Option<Value> {
        // TODO: migrate to Object.prototype field once initialization sets prototypes at allocation
        self.metadata
            .lock()
            .get_metadata(OBJECT_PROTOTYPE_OVERRIDE_METADATA_KEY, value)
    }

    pub(in crate::vm::interpreter) fn set_explicit_object_prototype(
        &self,
        value: Value,
        prototype: Value,
    ) {
        // Write to both kernel and metadata
        if let Some(obj_ptr) = checked_object_ptr(value) {
            let obj = unsafe { &mut *obj_ptr.as_ptr() };
            obj.prototype = prototype;
        }
        self.metadata.lock().define_metadata(
            OBJECT_PROTOTYPE_OVERRIDE_METADATA_KEY.to_string(),
            prototype,
            value,
        );
    }

    pub(in crate::vm::interpreter) fn is_js_object_value(&self, value: Value) -> bool {
        if checked_array_ptr(value).is_some() || self.callable_function_info(value).is_some() {
            return true;
        }
        checked_object_ptr(value).is_some()
            && self.nominal_class_name_for_value(value).as_deref() != Some("Symbol")
    }

    pub(in crate::vm::interpreter) fn is_array_value(&self, value: Value) -> Result<bool, VmError> {
        if let Some(proxy) = self.unwrapped_proxy_like(value) {
            if proxy.handler.is_null() {
                return Err(VmError::TypeError("Proxy has been revoked".to_string()));
            }
            return self.is_array_value(proxy.target);
        }

        Ok(checked_array_ptr(value).is_some())
    }

    fn js_value_supports_extensibility(&self, value: Value) -> bool {
        if checked_array_ptr(value).is_some() || checked_object_ptr(value).is_some() {
            return true;
        }
        self.callable_function_info(value).is_some()
    }

    fn is_js_value_extensible(&self, value: Value) -> bool {
        if !self.js_value_supports_extensibility(value) {
            return false;
        }
        // Property kernel: check OBJECT_FLAG_NOT_EXTENSIBLE
        if let Some(obj_ptr) = checked_object_ptr(value) {
            let obj = unsafe { &*obj_ptr.as_ptr() };
            return !obj.has_flag(crate::vm::object::OBJECT_FLAG_NOT_EXTENSIBLE);
        }
        // Fallback for non-Object values
        self.metadata
            .lock()
            .get_metadata(OBJECT_EXTENSIBLE_METADATA_KEY, value)
            .and_then(|flag| flag.as_bool())
            .unwrap_or(true)
    }

    fn set_js_value_extensible(&self, value: Value, extensible: bool) {
        if !self.js_value_supports_extensibility(value) {
            return;
        }
        // Property kernel: set/clear OBJECT_FLAG_NOT_EXTENSIBLE
        if let Some(obj_ptr) = checked_object_ptr(value) {
            let obj = unsafe { &mut *obj_ptr.as_ptr() };
            if extensible {
                obj.clear_flag(crate::vm::object::OBJECT_FLAG_NOT_EXTENSIBLE);
            } else {
                obj.set_flag(crate::vm::object::OBJECT_FLAG_NOT_EXTENSIBLE);
            }
            return;
        }
        // Fallback for non-Object values
        let mut metadata = self.metadata.lock();
        if extensible {
            metadata.delete_metadata(OBJECT_EXTENSIBLE_METADATA_KEY, value);
        } else {
            metadata.define_metadata(
                OBJECT_EXTENSIBLE_METADATA_KEY.to_string(),
                Value::bool(false),
                value,
            );
        }
    }

    fn has_own_js_property(&self, target: Value, key: &str) -> bool {
        self.resolve_own_property_shape(target, key).is_some()
    }

    fn raw_type_handle_id(value: Value) -> Option<crate::vm::object::TypeHandleId> {
        if !value.is_ptr() {
            return None;
        }
        let header = unsafe { &*header_ptr_from_value_ptr(value.as_ptr::<u8>().unwrap().as_ptr()) };
        if header.type_id() != std::any::TypeId::of::<TypeHandle>() {
            return None;
        }
        let handle_ptr = unsafe { value.as_ptr::<TypeHandle>() }?;
        Some(unsafe { (*handle_ptr.as_ptr()).handle_id })
    }

    fn public_property_target(&self, value: Value) -> Value {
        let Some(obj_ptr) = checked_object_ptr(value) else {
            return value;
        };
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let handle_key = self.intern_prop_key("__raya_type_handle__");
        let Some(handle_value) = obj
            .dyn_props()
            .and_then(|dp| dp.get(handle_key).map(|p| p.value))
        else {
            return value;
        };
        if Self::raw_type_handle_id(handle_value).is_some() {
            handle_value
        } else {
            value
        }
    }

    fn type_handle_nominal_id(&self, value: Value) -> Option<crate::vm::object::NominalTypeId> {
        let handle_id = Self::raw_type_handle_id(value)?;
        self.type_handles
            .read()
            .get(handle_id)
            .map(|entry| entry.nominal_type_id)
    }

    pub(in crate::vm::interpreter) fn constructor_value_for_nominal_type(
        &self,
        nominal_type_id: usize,
    ) -> Option<Value> {
        let class_name = {
            let classes = self.classes.read();
            classes.get_class(nominal_type_id)?.name.clone()
        };
        if let Some(global) = self.builtin_global_value(&class_name) {
            return Some(global);
        }

        if let Some(&slot) = self.class_value_slots.read().get(&nominal_type_id) {
            if let Some(value) = self.globals_by_index.read().get(slot).copied() {
                return Some(value);
            }
        }

        let (layout_id, _) = self.nominal_allocation(nominal_type_id)?;
        let mut class_value_slots = self.class_value_slots.write();
        if let Some(&slot) = class_value_slots.get(&nominal_type_id) {
            if let Some(value) = self.globals_by_index.read().get(slot).copied() {
                return Some(value);
            }
        }

        let handle_id = self
            .type_handles
            .write()
            .register(nominal_type_id as u32, layout_id, None);
        let gc_ptr = self.gc.lock().allocate(TypeHandle {
            handle_id,
            shape_id: None,
        });
        let value = unsafe {
            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).expect("type handle ptr"))
        };
        let mut globals = self.globals_by_index.write();
        let slot = globals.len();
        globals.push(value);
        class_value_slots.insert(nominal_type_id, slot);
        Some(value)
    }

    pub(in crate::vm::interpreter) fn constructor_nominal_type_id(
        &self,
        value: Value,
    ) -> Option<usize> {
        let value = self
            .unwrapped_proxy_like(value)
            .map(|proxy| proxy.target)
            .unwrap_or(value);

        let debug_ctor_resolve = std::env::var("RAYA_DEBUG_CTOR_RESOLVE").is_ok();

        if let Some(global_name) = self.builtin_global_name_for_value(value) {
            let classes = self.classes.read();
            if let Some(class) = classes.get_class_by_name(&global_name) {
                if debug_ctor_resolve {
                    eprintln!(
                        "[ctor-resolve] value={:#x} builtin_global='{}' -> nominal_type_id={} class='{}'",
                        value.raw(),
                        global_name,
                        class.id,
                        class.name
                    );
                }
                return Some(class.id);
            }
        }

        if let Some(nominal_id) = self.type_handle_nominal_id(value) {
            if debug_ctor_resolve {
                eprintln!(
                    "[ctor-resolve] value={:#x} type_handle_nominal_id={}",
                    value.raw(),
                    nominal_id
                );
            }
            return Some(nominal_id as usize);
        }

        None
    }

    fn constructor_static_field_value(&self, constructor: Value, key: &str) -> Option<Value> {
        if matches!(key, "prototype" | "name" | "length") {
            return None;
        }

        let mut current_nominal_type_id = Some(self.constructor_nominal_type_id(constructor)?);
        while let Some(nominal_type_id) = current_nominal_type_id {
            let (parent_id, value) = {
                let classes = self.classes.read();
                let class = classes.get_class(nominal_type_id)?;
                let value = self
                    .class_metadata
                    .read()
                    .get(nominal_type_id)
                    .and_then(|metadata| metadata.get_static_field_index(key))
                    .and_then(|index| class.get_static_field(index));
                (class.parent_id, value)
            };
            if value.is_some() {
                return value;
            }
            current_nominal_type_id = parent_id;
        }
        None
    }

    fn constructor_static_accessor_values(
        &self,
        constructor: Value,
        key: &str,
    ) -> Option<(Option<Value>, Option<Value>)> {
        if matches!(key, "prototype" | "name" | "length") {
            return None;
        }

        let mut current_nominal_type_id = Some(self.constructor_nominal_type_id(constructor)?);
        while let Some(nominal_type_id) = current_nominal_type_id {
            let (parent_id, module, getter_id, setter_id) = {
                let classes = self.classes.read();
                let class = classes.get_class(nominal_type_id)?;
                let getter_id = class
                    .static_members
                    .iter()
                    .rev()
                    .find(|member| {
                        member.name == key
                            && member.kind == crate::vm::object::PrototypeMemberKind::Getter
                    })
                    .map(|member| member.function_id);
                let setter_id = class
                    .static_members
                    .iter()
                    .rev()
                    .find(|member| {
                        member.name == key
                            && member.kind == crate::vm::object::PrototypeMemberKind::Setter
                    })
                    .map(|member| member.function_id);
                (class.parent_id, class.module.clone(), getter_id, setter_id)
            };

            if getter_id.is_some() || setter_id.is_some() {
                let module = module?;
                let make_closure = |func_id: usize| {
                    let closure =
                        Object::new_closure_with_module(func_id, Vec::new(), module.clone());
                    let closure_ptr = self.gc.lock().allocate(closure);
                    unsafe {
                        Value::from_ptr(
                            std::ptr::NonNull::new(closure_ptr.as_ptr())
                                .expect("constructor static accessor ptr"),
                        )
                    }
                };
                return Some((getter_id.map(make_closure), setter_id.map(make_closure)));
            }

            current_nominal_type_id = parent_id;
        }

        None
    }

    pub(in crate::vm::interpreter) fn materialize_constructor_static_method(
        &self,
        constructor: Value,
        key: &str,
    ) -> Option<Value> {
        if matches!(key, "prototype" | "name" | "length") {
            return None;
        }

        let origin_nominal_type_id = self.constructor_nominal_type_id(constructor)?;
        let mut current_nominal_type_id = Some(origin_nominal_type_id);

        while let Some(nominal_type_id) = current_nominal_type_id {
            let (parent_id, module, func_id) = {
                let classes = self.classes.read();
                let class = classes.get_class(nominal_type_id)?;
                let func_id = class
                    .static_members
                    .iter()
                    .rev()
                    .find(|member| {
                        member.name == key
                            && member.kind == crate::vm::object::PrototypeMemberKind::Method
                    })
                    .map(|member| member.function_id);
                (class.parent_id, class.module.clone(), func_id)
            };

            if let Some(func_id) = func_id {
                let module = module?;
                let property_target = if nominal_type_id == origin_nominal_type_id {
                    constructor
                } else {
                    self.constructor_value_for_nominal_type(nominal_type_id)?
                };
                let mut closure =
                    Object::new_closure_with_module(func_id, Vec::new(), module.clone());
                let _ = closure.set_callable_home_object(property_target);
                let closure_ptr = self.gc.lock().allocate(closure);
                let closure_value = unsafe {
                    Value::from_ptr(
                        std::ptr::NonNull::new(closure_ptr.as_ptr())
                            .expect("constructor static method ptr"),
                    )
                };
                let _ = self.define_data_property_on_target(
                    property_target,
                    key,
                    closure_value,
                    true,
                    false,
                    true,
                );
                return Some(closure_value);
            }

            current_nominal_type_id = parent_id;
        }

        None
    }

    fn has_constructor_static_method(&self, constructor: Value, key: &str) -> bool {
        if matches!(key, "prototype" | "name" | "length") {
            return false;
        }

        let mut current_nominal_type_id = match self.constructor_nominal_type_id(constructor) {
            Some(id) => Some(id),
            None => return false,
        };
        while let Some(nominal_type_id) = current_nominal_type_id {
            let (parent_id, has_method) = {
                let classes = self.classes.read();
                let Some(class) = classes.get_class(nominal_type_id) else {
                    return false;
                };
                let has_method = class.static_members.iter().any(|member| {
                    member.name == key
                        && member.kind == crate::vm::object::PrototypeMemberKind::Method
                });
                (class.parent_id, has_method)
            };
            if has_method {
                return true;
            }
            current_nominal_type_id = parent_id;
        }
        false
    }

    pub(in crate::vm::interpreter) fn callable_virtual_property_deleted(
        &self,
        target: Value,
        key: &str,
    ) -> bool {
        self.metadata
            .lock()
            .get_metadata_property(CALLABLE_VIRTUAL_DELETED_METADATA_KEY, target, key)
            .is_some_and(|value| value.is_truthy())
    }

    fn cached_callable_virtual_property_value(&self, target: Value, key: &str) -> Option<Value> {
        self.metadata
            .lock()
            .get_metadata_property(CALLABLE_VIRTUAL_VALUE_METADATA_KEY, target, key)
    }

    fn set_cached_callable_virtual_property_value(&self, target: Value, key: &str, value: Value) {
        self.metadata.lock().define_metadata_property(
            CALLABLE_VIRTUAL_VALUE_METADATA_KEY.to_string(),
            value,
            target,
            key.to_string(),
        );
    }

    pub(in crate::vm::interpreter) fn is_callable_virtual_property(
        &self,
        target: Value,
        key: &str,
    ) -> bool {
        self.callable_virtual_property_value(target, key).is_some()
            || self
                .callable_virtual_accessor_value(target, key, "get")
                .is_some()
            || self
                .callable_virtual_accessor_value(target, key, "set")
                .is_some()
    }

    pub(in crate::vm::interpreter) fn set_callable_virtual_property_deleted(
        &self,
        target: Value,
        key: &str,
        deleted: bool,
    ) {
        let mut metadata = self.metadata.lock();
        if deleted {
            metadata.define_metadata_property(
                CALLABLE_VIRTUAL_DELETED_METADATA_KEY.to_string(),
                Value::bool(true),
                target,
                key.to_string(),
            );
        } else {
            let _ = metadata.delete_metadata_property(
                CALLABLE_VIRTUAL_DELETED_METADATA_KEY,
                target,
                key,
            );
        }
    }

    pub(in crate::vm::interpreter) fn fixed_property_deleted(
        &self,
        target: Value,
        key: &str,
    ) -> bool {
        self.metadata
            .lock()
            .get_metadata_property(FIXED_PROPERTY_DELETED_METADATA_KEY, target, key)
            .is_some_and(|value| value.is_truthy())
    }

    pub(in crate::vm::interpreter) fn set_fixed_property_deleted(
        &self,
        target: Value,
        key: &str,
        deleted: bool,
    ) {
        let mut metadata = self.metadata.lock();
        if deleted {
            metadata.define_metadata_property(
                FIXED_PROPERTY_DELETED_METADATA_KEY.to_string(),
                Value::bool(true),
                target,
                key.to_string(),
            );
        } else {
            let _ =
                metadata.delete_metadata_property(FIXED_PROPERTY_DELETED_METADATA_KEY, target, key);
        }
    }

    pub(in crate::vm::interpreter) fn is_runtime_global_object(&self, target: Value) -> bool {
        self.builtin_global_value("globalThis")
            .is_some_and(|global_obj| global_obj.raw() == target.raw())
    }

    pub(in crate::vm::interpreter) fn builtin_global_property_value(
        &self,
        target: Value,
        key: &str,
    ) -> Option<Value> {
        self.ambient_builtin_global_descriptor(target, key)
            .map(|descriptor| descriptor.value)
    }

    fn builtin_object_constant_descriptor(
        &self,
        target: Value,
        key: &str,
    ) -> Option<AmbientGlobalDescriptor> {
        let target = self.proxy_wrapper_proxy_value(target).unwrap_or(target);
        let public_target = self.public_property_target(target);
        let field_value = self
            .get_own_field_value_by_name(target, key)
            .or_else(|| self.get_own_field_value_by_name(public_target, key))?;
        let is_math_object = self.is_ambient_math_constant_target(target, key)
            || (public_target.raw() != target.raw()
                && self.is_ambient_math_constant_target(public_target, key));
        if is_math_object && matches!(key, "PI" | "E") {
            return Some(AmbientGlobalDescriptor {
                value: field_value,
                writable: false,
                configurable: false,
                enumerable: false,
            });
        }
        None
    }

    pub(in crate::vm::interpreter::opcodes) fn is_ambient_math_constant_target(
        &self,
        target: Value,
        key: &str,
    ) -> bool {
        let target = self.proxy_wrapper_proxy_value(target).unwrap_or(target);
        let public_target = self.public_property_target(target);
        let ambient_math = self.ambient_global_value_sync("Math").map(|math| {
            let math = self.proxy_wrapper_proxy_value(math).unwrap_or(math);
            (math, self.public_property_target(math))
        });
        let matches_target = |candidate: Value| {
            self.builtin_global_name_for_value(candidate).as_deref() == Some("Math")
                || ambient_math.is_some_and(|(math, public_math)| {
                    math.raw() == candidate.raw() || public_math.raw() == candidate.raw()
                })
        };
        matches!(key, "PI" | "E")
            && (matches_target(target)
                || (public_target.raw() != target.raw() && matches_target(public_target)))
    }

    pub(in crate::vm::interpreter) fn set_builtin_global_property(
        &self,
        target: Value,
        key: &str,
        value: Value,
    ) -> bool {
        if self
            .ambient_builtin_global_descriptor(target, key)
            .is_none()
        {
            return false;
        }
        let slot = match self.builtin_global_slots.read().get(key).copied() {
            Some(slot) => slot,
            None => return false,
        };
        let mut globals = self.globals_by_index.write();
        if slot >= globals.len() {
            globals.resize(slot + 1, Value::undefined());
        }
        globals[slot] = value;
        self.set_fixed_property_deleted(target, key, false);
        true
    }

    fn ambient_builtin_global_property_flags(
        &self,
        target: Value,
        key: &str,
    ) -> Option<(bool, bool, bool)> {
        self.ambient_builtin_global_descriptor(target, key)
            .map(|descriptor| {
                (
                    descriptor.writable,
                    descriptor.configurable,
                    descriptor.enumerable,
                )
            })
    }

    fn ambient_builtin_global_descriptor(
        &self,
        target: Value,
        key: &str,
    ) -> Option<AmbientGlobalDescriptor> {
        if !self.is_runtime_global_object(target) || self.fixed_property_deleted(target, key) {
            return None;
        }

        match key {
            "Infinity" => Some(AmbientGlobalDescriptor {
                value: Value::f64(f64::INFINITY),
                writable: false,
                configurable: false,
                enumerable: false,
            }),
            "NaN" => Some(AmbientGlobalDescriptor {
                value: Value::f64(f64::NAN),
                writable: false,
                configurable: false,
                enumerable: false,
            }),
            "undefined" => Some(AmbientGlobalDescriptor {
                value: Value::undefined(),
                writable: false,
                configurable: false,
                enumerable: false,
            }),
            _ => self
                .builtin_global_value(key)
                .map(|value| AmbientGlobalDescriptor {
                    value,
                    writable: true,
                    configurable: true,
                    enumerable: false,
                }),
        }
    }

    pub(in crate::vm::interpreter) fn allow_ambient_builtin_global_noop_write(
        &self,
        target: Value,
        key: &str,
        value: Value,
    ) -> bool {
        self.ambient_builtin_global_descriptor(target, key)
            .is_some_and(|descriptor| {
                !descriptor.writable && value_same_value(descriptor.value, value)
            })
    }

    fn bind_script_global_property(
        &mut self,
        key: &str,
        value: Value,
        configurable: bool,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<(), VmError> {
        if let Some(binding) = self.shared_js_global_binding(key) {
            {
                let mut globals = self.globals_by_index.write();
                if binding.slot >= globals.len() {
                    globals.resize(binding.slot + 1, Value::undefined());
                }
                globals[binding.slot] = value;
            }
            if let Some(binding) = self.js_global_bindings.write().get_mut(key) {
                binding.initialized = true;
            }
        }

        let Some(global_this) = self.ensure_builtin_global_value("globalThis", caller_task)? else {
            return Ok(());
        };

        let has_concrete_own_property = self.get_descriptor_metadata(global_this, key).is_some()
            || self
                .get_own_js_property_value_by_name(global_this, key)
                .is_some()
            || self.own_js_property_flags(global_this, key).is_some()
            || (self.is_runtime_global_object(global_this)
                && self.builtin_global_slots.read().contains_key(key));

        if has_concrete_own_property {
            if let Some((_, configurable, _)) = self.own_js_property_flags(global_this, key) {
                if configurable {
                    self.define_data_property_on_target(
                        global_this,
                        key,
                        value,
                        true,
                        true,
                        configurable,
                    )?;
                    return Ok(());
                }
            }
            return match self.set_property_value_via_js_semantics(
                global_this,
                key,
                value,
                global_this,
                caller_task,
                caller_module,
            )? {
                true => Ok(()),
                false => {
                    if self.allow_ambient_builtin_global_noop_write(global_this, key, value) {
                        return Ok(());
                    }
                    Err(VmError::TypeError(format!(
                        "Cannot assign to non-writable property '{}'",
                        key
                    )))
                }
            };
        }

        if self.is_js_value_extensible(global_this) {
            self.define_data_property_on_target(global_this, key, value, true, true, configurable)?;
        }

        Ok(())
    }

    pub(in crate::vm::interpreter) fn sync_existing_script_global_property(
        &mut self,
        key: &str,
        value: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<(), VmError> {
        let Some(global_this) = self.ensure_builtin_global_value("globalThis", caller_task)? else {
            return Ok(());
        };
        if !self.has_property_via_js_semantics(global_this, key) {
            return Ok(());
        }
        match self.set_property_value_via_js_semantics(
            global_this,
            key,
            value,
            global_this,
            caller_task,
            caller_module,
        )? {
            true => Ok(()),
            false => Ok(()),
        }
    }

    pub(in crate::vm::interpreter) fn current_activation_eval_env(
        &self,
        task: &Arc<Task>,
    ) -> Option<Value> {
        task.current_active_direct_eval_env()
            .or_else(|| task.current_activation_direct_eval_env())
            .or_else(|| {
                task.current_closure().and_then(|closure| {
                    let closure_ptr = unsafe { closure.as_ptr::<Object>() }?;
                    let closure_obj = unsafe { &*closure_ptr.as_ptr() };
                    closure_obj.callable_direct_eval_env()
                })
            })
    }

    fn capture_activation_identifier_assignment_target(
        &mut self,
        task: &Arc<Task>,
        module: &Module,
        key: &str,
    ) -> Result<Option<Value>, VmError> {
        let Some(env) = self.current_activation_eval_env(task) else {
            return Ok(None);
        };
        let mut cursor = Some(env);
        while let Some(current) = cursor {
            if let Some(target) = self.with_env_target(current) {
                let target = self.proxy_wrapper_proxy_value(target).unwrap_or(target);
                if self.with_env_has_binding(target, key, task, module)? {
                    return Ok(Some(target));
                }
            } else if self.resolve_own_property_shape(current, key).is_some() {
                return Ok(None);
            }
            cursor = self.direct_eval_outer_env(current);
        }
        Ok(None)
    }

    fn assign_ambient_identifier_value(
        &mut self,
        task: &Arc<Task>,
        module: &Module,
        key: &str,
        value: Value,
    ) -> Result<(), VmError> {
        let did_set_activation = self.activation_eval_env_set(task, module, key, value)?;
        if did_set_activation {
            return Ok(());
        }

        let strict = self.current_function_is_strict_js(task, module);
        if self.set_shared_js_global_binding_value(key, value, task, module)? {
            return Ok(());
        }

        let has_ambient_binding = self.shared_js_global_binding(key).is_some()
            || self
                .builtin_global_value("globalThis")
                .is_some_and(|global_this| self.has_property_via_js_semantics(global_this, key));

        if strict && !has_ambient_binding {
            return Err(self.raise_task_builtin_error(
                task,
                "ReferenceError",
                format!("{key} is not defined"),
            ));
        }

        self.bind_script_global_property(key, value, true, task, module)
    }

    fn store_identifier_assignment_target(
        &mut self,
        task: &Arc<Task>,
        module: &Module,
        target: Option<Value>,
        key: &str,
        value: Value,
    ) -> Result<(), VmError> {
        if let Some(target) = target {
            let strict = self.current_function_is_strict_js(task, module);
            if !self.with_env_has_binding(target, key, task, module)? {
                if strict {
                    return Err(self.raise_task_builtin_error(
                        task,
                        "ReferenceError",
                        format!("{key} is not defined"),
                    ));
                }
                self.set_property_value_via_js_semantics(target, key, value, target, task, module)?;
                return Ok(());
            }
            match self
                .set_property_value_via_js_semantics(target, key, value, target, task, module)?
            {
                true => Ok(()),
                false if strict => Err(self.raise_task_builtin_error(
                    task,
                    "TypeError",
                    format!("Cannot assign to non-writable property '{key}'"),
                )),
                false => Ok(()),
            }
        } else {
            self.assign_ambient_identifier_value(task, module, key, value)
        }
    }

    fn activation_eval_env_get(
        &mut self,
        task: &Arc<Task>,
        module: &Module,
        key: &str,
    ) -> Result<Option<Value>, VmError> {
        let Some(env) = self.current_activation_eval_env(task) else {
            return Ok(None);
        };
        let mut cursor = Some(env);
        while let Some(current) = cursor {
            if let Some(target) = self.with_env_target(current) {
                let target = self.proxy_wrapper_proxy_value(target).unwrap_or(target);
                if self.with_env_has_binding(target, key, task, module)? {
                    return self.get_property_value_on_receiver_via_js_semantics_with_context(
                        target, key, target, task, module,
                    );
                }
            } else if self.resolve_own_property_shape(current, key).is_some() {
                if self.direct_eval_binding_is_uninitialized(current, key) {
                    return Err(self.raise_task_builtin_error(
                        task,
                        "ReferenceError",
                        format!("{key} is not defined"),
                    ));
                }
                return self.get_own_property_value_via_js_semantics_with_context(
                    current, key, task, module,
                );
            }
            cursor = self.direct_eval_outer_env(current);
        }
        Ok(None)
    }

    fn resolve_direct_eval_binding_env(&self, env: Value, key: &str) -> Option<Value> {
        let mut cursor = Some(env);
        while let Some(current) = cursor {
            if self.resolve_own_property_shape(current, key).is_some() {
                return Some(current);
            }
            cursor = self.direct_eval_outer_env(current);
        }
        None
    }

    fn direct_eval_chain_outer_has_binding(&self, env: Value, key: &str) -> bool {
        let mut cursor = self.direct_eval_outer_env(env);
        while let Some(current) = cursor {
            if self.resolve_own_property_shape(current, key).is_some() {
                return true;
            }
            cursor = self.direct_eval_outer_env(current);
        }
        false
    }

    fn direct_eval_binding_is_lexical(&self, env: Value, key: &str) -> bool {
        self.get_own_js_property_value_by_name(env, &direct_eval_lexical_marker_key(key))
            .is_some_and(|value| value.is_truthy())
    }

    fn direct_eval_binding_is_uninitialized(&self, env: Value, key: &str) -> bool {
        self.get_own_js_property_value_by_name(env, &direct_eval_uninitialized_marker_key(key))
            .is_some_and(|value| value.is_truthy())
    }

    fn direct_eval_binding_is_outer_snapshot(&self, env: Value, key: &str) -> bool {
        self.get_own_js_property_value_by_name(env, &direct_eval_outer_snapshot_marker_key(key))
            .is_some_and(|value| value.is_truthy())
    }

    fn mark_direct_eval_binding_lexical(
        &self,
        env: Value,
        key: &str,
        uninitialized: bool,
    ) -> Result<(), VmError> {
        self.define_data_property_on_target(
            env,
            &direct_eval_lexical_marker_key(key),
            Value::bool(true),
            true,
            false,
            true,
        )?;
        self.define_data_property_on_target(
            env,
            &direct_eval_uninitialized_marker_key(key),
            Value::bool(uninitialized),
            true,
            false,
            true,
        )?;
        Ok(())
    }

    fn clear_direct_eval_binding_uninitialized(
        &self,
        env: Value,
        key: &str,
    ) -> Result<(), VmError> {
        self.define_data_property_on_target(
            env,
            &direct_eval_uninitialized_marker_key(key),
            Value::bool(false),
            true,
            false,
            true,
        )
    }

    fn clear_direct_eval_binding_outer_snapshot(
        &mut self,
        env: Value,
        key: &str,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<(), VmError> {
        let _ = self.delete_property_from_target(
            env,
            self.alloc_string_value(&direct_eval_outer_snapshot_marker_key(key)),
            task,
            module,
        )?;
        Ok(())
    }

    fn direct_eval_chain_has_lexical_binding(&self, env: Value, key: &str) -> bool {
        let mut cursor = Some(env);
        while let Some(current) = cursor {
            if self.resolve_own_property_shape(current, key).is_some()
                && self.direct_eval_binding_is_lexical(current, key)
            {
                return true;
            }
            cursor = self.direct_eval_outer_env(current);
        }
        false
    }

    fn activation_eval_env_set(
        &mut self,
        task: &Arc<Task>,
        module: &Module,
        key: &str,
        value: Value,
    ) -> Result<bool, VmError> {
        let Some(env) = self.current_activation_eval_env(task) else {
            return Ok(false);
        };
        let mut cursor = Some(env);
        while let Some(current) = cursor {
            if let Some(target) = self.with_env_target(current) {
                let target = self.proxy_wrapper_proxy_value(target).unwrap_or(target);
                if self.with_env_has_binding(target, key, task, module)? {
                    return self.set_property_value_via_js_semantics(
                        target, key, value, target, task, module,
                    );
                }
            } else if self.resolve_own_property_shape(current, key).is_some() {
                let written = self.set_property_value_via_js_semantics(
                    current, key, value, current, task, module,
                )?;
                if written && self.direct_eval_binding_is_uninitialized(current, key) {
                    self.clear_direct_eval_binding_uninitialized(current, key)?;
                }
                return Ok(written);
            }
            cursor = self.direct_eval_outer_env(current);
        }
        Ok(false)
    }

    fn activation_eval_env_create_mutable_binding(
        &mut self,
        task: &Arc<Task>,
        module: &Module,
        key: &str,
        value: Value,
    ) -> Result<bool, VmError> {
        let Some(env) = self.current_activation_eval_env(task) else {
            return Ok(false);
        };
        if self.resolve_own_property_shape(env, key).is_some() {
            if self.direct_eval_binding_is_outer_snapshot(env, key) {
                let _ =
                    self.set_property_value_via_js_semantics(env, key, value, env, task, module)?;
                self.clear_direct_eval_binding_outer_snapshot(env, key, task, module)?;
            }
            return Ok(true);
        }
        self.define_data_property_on_target(env, key, value, true, true, true)?;
        Ok(true)
    }

    fn declare_direct_eval_lexical_binding(
        &mut self,
        env: Value,
        key: &str,
    ) -> Result<(), VmError> {
        if self.resolve_own_property_shape(env, key).is_none() {
            self.define_data_property_on_target(env, key, Value::undefined(), true, false, true)?;
        }
        self.mark_direct_eval_binding_lexical(env, key, true)?;
        Ok(())
    }

    fn activation_eval_env_has(&mut self, task: &Arc<Task>, key: &str) -> bool {
        let Some(env) = self.current_activation_eval_env(task) else {
            return false;
        };
        let mut cursor = Some(env);
        while let Some(current) = cursor {
            if let Some(target) = self.with_env_target(current) {
                let current_module = task.current_module();
                if let Ok(true) = self.with_env_has_binding(target, key, task, &current_module) {
                    return true;
                }
            } else if self.resolve_own_property_shape(current, key).is_some() {
                return true;
            }
            cursor = self.direct_eval_outer_env(current);
        }
        false
    }

    fn with_env_has_binding(
        &mut self,
        target: Value,
        key: &str,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<bool, VmError> {
        let target = self.proxy_wrapper_proxy_value(target).unwrap_or(target);
        if !self.has_property_via_js_semantics_with_context(target, key, task, module)? {
            return Ok(false);
        }

        let Some(unscopables) = self.get_property_value_via_js_semantics_with_context(
            target,
            "Symbol.unscopables",
            task,
            module,
        )?
        else {
            return Ok(true);
        };

        if !self.is_js_object_value(unscopables) {
            return Ok(true);
        }

        let blocked = self
            .get_property_value_on_receiver_via_js_semantics_with_context(
                unscopables,
                key,
                unscopables,
                task,
                module,
            )?
            .is_some_and(|value| value.is_truthy());
        Ok(!blocked)
    }

    fn active_direct_eval_uses_script_global_bindings(&self, task: &Arc<Task>) -> bool {
        task.current_active_direct_eval_uses_script_global_bindings()
            || task.current_closure().is_some_and(|closure| {
                let Some(closure_ptr) = (unsafe { closure.as_ptr::<Object>() }) else {
                    return false;
                };
                let closure_obj = unsafe { &*closure_ptr.as_ptr() };
                closure_obj.callable_direct_eval_uses_script_global_bindings()
            })
    }

    fn active_direct_eval_persist_caller_declarations(&self, task: &Arc<Task>) -> bool {
        task.current_active_direct_eval_persist_caller_declarations()
    }

    fn direct_eval_outer_env(&self, env: Value) -> Option<Value> {
        self.get_own_js_property_value_by_name(env, DIRECT_EVAL_OUTER_ENV_KEY)
    }

    fn with_env_target(&self, env: Value) -> Option<Value> {
        self.get_own_js_property_value_by_name(env, WITH_ENV_TARGET_KEY)
    }

    fn set_direct_eval_outer_env(&self, env: Value, outer_env: Value) -> Result<(), VmError> {
        self.define_data_property_on_target(
            env,
            DIRECT_EVAL_OUTER_ENV_KEY,
            outer_env,
            true,
            false,
            true,
        )
    }

    fn set_direct_eval_completion(&self, env: Value, value: Value) -> Result<(), VmError> {
        self.define_data_property_on_target(
            env,
            DIRECT_EVAL_COMPLETION_KEY,
            value,
            true,
            true,
            true,
        )
    }

    fn direct_eval_completion(&self, env: Value) -> Option<Value> {
        self.get_own_js_property_value_by_name(env, DIRECT_EVAL_COMPLETION_KEY)
    }

    fn seal_direct_eval_snapshot_env(&self, env: Value) -> Result<(), VmError> {
        for key in self.js_own_property_names(env) {
            if key == DIRECT_EVAL_OUTER_ENV_KEY
                || key == DIRECT_EVAL_COMPLETION_KEY
                || key.starts_with(DIRECT_EVAL_LEXICAL_MARKER_PREFIX)
                || key.starts_with(DIRECT_EVAL_UNINITIALIZED_MARKER_PREFIX)
                || key.starts_with(DIRECT_EVAL_OUTER_SNAPSHOT_MARKER_PREFIX)
            {
                continue;
            }
            if let Some(value) = self.get_own_js_property_value_by_name(env, &key) {
                self.define_data_property_on_target(env, &key, value, true, true, false)?;
            }
        }
        Ok(())
    }

    fn alloc_direct_eval_runtime_env(&self, outer_env: Value) -> Result<Value, VmError> {
        let env = self.alloc_plain_object()?;
        self.set_direct_eval_outer_env(env, outer_env)?;
        self.set_direct_eval_completion(env, Value::undefined())?;
        Ok(env)
    }

    fn alloc_with_runtime_env(
        &self,
        target: Value,
        outer_env: Option<Value>,
    ) -> Result<Value, VmError> {
        let env = self.alloc_plain_object()?;
        self.define_data_property_on_target(env, WITH_ENV_TARGET_KEY, target, true, false, true)?;
        if let Some(outer_env) = outer_env {
            self.set_direct_eval_outer_env(env, outer_env)?;
        }
        Ok(env)
    }

    fn bind_direct_eval_global_var(
        &mut self,
        key: &str,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<(), VmError> {
        let Some(global_this) = self.ensure_builtin_global_value("globalThis", task)? else {
            return Ok(());
        };
        if self.has_property_via_js_semantics(global_this, key) {
            return Ok(());
        }
        if self.is_js_value_extensible(global_this) {
            self.define_data_property_on_target(
                global_this,
                key,
                Value::undefined(),
                true,
                true,
                true,
            )?;
            let _ = self.set_property_value_via_js_semantics(
                global_this,
                key,
                Value::undefined(),
                global_this,
                task,
                module,
            )?;
        }
        Ok(())
    }

    fn bind_direct_eval_global_function(
        &mut self,
        key: &str,
        value: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<(), VmError> {
        let Some(global_this) = self.builtin_global_value("globalThis") else {
            return Ok(());
        };
        let own_flags = self.own_js_property_flags(global_this, key);
        if own_flags.is_some_and(|(_, configurable, _)| configurable) || own_flags.is_none() {
            self.define_data_property_on_target(global_this, key, value, true, true, true)?;
            return Ok(());
        }
        let _ = self.set_property_value_via_js_semantics(
            global_this,
            key,
            value,
            global_this,
            task,
            module,
        )?;
        Ok(())
    }

    fn activation_eval_env_declare_var(
        &mut self,
        task: &Arc<Task>,
        module: &Module,
        key: &str,
    ) -> Result<(), VmError> {
        let Some(env) = self.current_activation_eval_env(task) else {
            return Ok(());
        };
        if !self.current_function_is_strict_js(task, module)
            && self.direct_eval_chain_has_lexical_binding(env, key)
        {
            return Err(self.raise_task_builtin_error(
                task,
                "SyntaxError",
                format!(
                    "direct eval cannot declare variable '{key}' over an existing lexical binding"
                ),
            ));
        }
        let outer_has_binding = self.direct_eval_chain_outer_has_binding(env, key);
        let local_has_binding = self.resolve_own_property_shape(env, key).is_some();
        if local_has_binding && self.direct_eval_binding_is_outer_snapshot(env, key) {
            let _ = self.set_property_value_via_js_semantics(
                env,
                key,
                Value::undefined(),
                env,
                task,
                module,
            )?;
            self.clear_direct_eval_binding_outer_snapshot(env, key, task, module)?;
        } else if !local_has_binding
            && !(self.active_direct_eval_persist_caller_declarations(task) && outer_has_binding)
        {
            let _ = self.activation_eval_env_create_mutable_binding(
                task,
                module,
                key,
                Value::undefined(),
            )?;
        }
        if self.active_direct_eval_uses_script_global_bindings(task) {
            self.bind_direct_eval_global_var(key, task, module)?;
        }
        Ok(())
    }

    fn activation_eval_env_declare_lexical(
        &mut self,
        task: &Arc<Task>,
        key: &str,
    ) -> Result<(), VmError> {
        let Some(env) = self.current_activation_eval_env(task) else {
            return Err(VmError::RuntimeError(
                "No active eval environment".to_string(),
            ));
        };
        self.declare_direct_eval_lexical_binding(env, key)
    }

    fn predeclare_direct_eval_lexical_declarations(
        &mut self,
        env: Value,
        declarations: &DirectEvalDeclarations,
    ) -> Result<(), VmError> {
        for name in &declarations.lexical_names {
            self.declare_direct_eval_lexical_binding(env, name)?;
        }
        Ok(())
    }

    fn preflight_direct_eval_parameter_var_collisions(
        &self,
        env: Value,
        declarations: &DirectEvalDeclarations,
        task: &Arc<Task>,
    ) -> Result<(), VmError> {
        for name in declarations
            .var_names
            .iter()
            .chain(declarations.function_names.iter())
        {
            let Some(binding_env) = self.resolve_direct_eval_binding_env(env, name) else {
                continue;
            };
            if binding_env.raw() == env.raw()
                && !self.direct_eval_binding_is_outer_snapshot(binding_env, name)
            {
                return Err(self.raise_task_builtin_error(
                    task,
                    "SyntaxError",
                    format!(
                        "direct eval may not declare '{}' during parameter initialization",
                        name
                    ),
                ));
            }
        }
        Ok(())
    }

    fn delete_js_identifier_reference(
        &mut self,
        task: &Arc<Task>,
        module: &Module,
        key: &str,
        resolved_locally: bool,
    ) -> Result<bool, VmError> {
        if self.activation_eval_env_has(task, key) {
            let Some(env) = self.current_activation_eval_env(task) else {
                return Ok(false);
            };
            if let Some(binding_env) = self.resolve_direct_eval_binding_env(env, key) {
                return self.delete_property_from_target(
                    binding_env,
                    self.alloc_string_value(key),
                    task,
                    module,
                );
            }
            return Ok(false);
        }
        if resolved_locally {
            return Ok(false);
        }
        let Some(global_this) = self.builtin_global_value("globalThis") else {
            return Ok(true);
        };
        if self.has_property_via_js_semantics(global_this, key) {
            return self.delete_property_from_target(
                global_this,
                self.alloc_string_value(key),
                task,
                module,
            );
        }
        Ok(true)
    }

    fn raise_unresolved_identifier_error(&mut self, task: &Arc<Task>, name: &str) -> VmError {
        self.raise_task_builtin_error(task, "ReferenceError", format!("{name} is not defined"))
    }

    fn visible_function_name(raw_name: &str) -> String {
        let visible = raw_name.rsplit("::").next().unwrap_or(raw_name);
        match visible {
            "__speciesGetter" => "get [Symbol.species]".to_string(),
            "__symbolIterator" => "[Symbol.iterator]".to_string(),
            _ => visible.to_string(),
        }
    }

    fn prototype_symbol_alias_specs(class_name: &str) -> &'static [(&'static str, &'static str)] {
        match class_name {
            "Array" => &[("Symbol.iterator", "values")],
            "Map" => &[("Symbol.iterator", "entries")],
            "Set" => &[("Symbol.iterator", "values")],
            "String" => &[("Symbol.iterator", "__symbolIterator")],
            _ => &[],
        }
    }

    fn seed_array_unscopables_property(&self, prototype_val: Value) -> Option<()> {
        let layout_id = layout_id_from_ordered_names(&[]);
        let unscopables_ptr = self.gc.lock().allocate(Object::new_dynamic(layout_id, 0));
        let unscopables_val = unsafe {
            Value::from_ptr(
                std::ptr::NonNull::new(unscopables_ptr.as_ptr()).expect("unscopables object ptr"),
            )
        };
        self.set_explicit_object_prototype(unscopables_val, Value::null());

        for key in [
            "copyWithin",
            "entries",
            "fill",
            "find",
            "findIndex",
            "flat",
            "flatMap",
            "includes",
            "keys",
            "values",
            "findLast",
            "findLastIndex",
            "toReversed",
            "toSorted",
            "toSpliced",
        ] {
            self.define_data_property_on_target(
                unscopables_val,
                key,
                Value::bool(true),
                true,
                true,
                true,
            )
            .ok()?;
        }

        self.define_data_property_on_target(
            prototype_val,
            "Symbol.unscopables",
            unscopables_val,
            false,
            false,
            true,
        )
        .ok()?;
        Some(())
    }

    fn should_skip_public_prototype_method_name(class_name: &str, method_name: &str) -> bool {
        class_name == "String" && method_name == "__symbolIterator"
    }

    fn define_prototype_symbol_aliases(
        &self,
        class_name: &str,
        prototype_val: Value,
        methods: &[(String, Value)],
    ) -> Option<()> {
        for (property_name, method_name) in Self::prototype_symbol_alias_specs(class_name) {
            let method_value = methods
                .iter()
                .find(|(candidate, _)| candidate == method_name)
                .map(|(_, value)| *value)?;
            if class_name == "String" && *property_name == "Symbol.iterator" {
                self.define_data_property_on_target(
                    method_value,
                    "name",
                    self.alloc_string_value("[Symbol.iterator]"),
                    false,
                    false,
                    true,
                )
                .ok()?;
                self.define_data_property_on_target(
                    method_value,
                    "length",
                    Value::i32(0),
                    false,
                    false,
                    true,
                )
                .ok()?;
            }
            self.define_data_property_on_target(
                prototype_val,
                property_name,
                method_value,
                true,
                false,
                true,
            )
            .ok()?;
        }
        Some(())
    }

    fn function_native_alias_id(raw_name: &str) -> Option<u16> {
        if raw_name == "Function::constructor" || raw_name.ends_with("::Function::constructor") {
            Some(crate::compiler::native_id::FUNCTION_CONSTRUCTOR_HELPER)
        } else if raw_name.ends_with("Function::call") {
            Some(crate::compiler::native_id::FUNCTION_CALL_HELPER)
        } else if raw_name.ends_with("Function::apply") {
            Some(crate::compiler::native_id::FUNCTION_APPLY_HELPER)
        } else if raw_name.ends_with("Function::bind") {
            Some(crate::compiler::native_id::FUNCTION_BIND_HELPER)
        } else {
            None
        }
    }

    pub(in crate::vm::interpreter) fn native_callable_uses_receiver(&self, native_id: u16) -> bool {
        !matches!(
            native_id,
            crate::compiler::native_id::OBJECT_DEFINE_PROPERTY
                | crate::compiler::native_id::OBJECT_DEFINE_CLASS_PROPERTY
                | crate::compiler::native_id::OBJECT_GET_OWN_PROPERTY_DESCRIPTOR
                | crate::compiler::native_id::OBJECT_DEFINE_PROPERTIES
                | crate::compiler::native_id::OBJECT_DELETE_PROPERTY
                | crate::compiler::native_id::OBJECT_DELETE_PROPERTY_STRICT
                | crate::compiler::native_id::OBJECT_GET_PROTOTYPE_OF
                | crate::compiler::native_id::OBJECT_GET_AMBIENT_GLOBAL
                | crate::compiler::native_id::CRYPTO_HASH
                | crate::compiler::native_id::CRYPTO_HASH_BYTES
                | crate::compiler::native_id::CRYPTO_HMAC
                | crate::compiler::native_id::CRYPTO_HMAC_BYTES
                | crate::compiler::native_id::CRYPTO_RANDOM_BYTES
                | crate::compiler::native_id::CRYPTO_RANDOM_INT
                | crate::compiler::native_id::CRYPTO_RANDOM_UUID
                | crate::compiler::native_id::CRYPTO_TO_HEX
                | crate::compiler::native_id::CRYPTO_FROM_HEX
                | crate::compiler::native_id::CRYPTO_TO_BASE64
                | crate::compiler::native_id::CRYPTO_FROM_BASE64
                | crate::compiler::native_id::CRYPTO_TIMING_SAFE_EQUAL
                | crate::compiler::native_id::CRYPTO_ENCRYPT
                | crate::compiler::native_id::CRYPTO_DECRYPT
                | crate::compiler::native_id::CRYPTO_GENERATE_KEY
                | crate::compiler::native_id::CRYPTO_SIGN
                | crate::compiler::native_id::CRYPTO_VERIFY
                | crate::compiler::native_id::CRYPTO_GENERATE_KEY_PAIR
                | crate::compiler::native_id::CRYPTO_HKDF
                | crate::compiler::native_id::CRYPTO_PBKDF2
        ) && !(crate::compiler::native_id::REFLECT_DEFINE_METADATA
            ..=crate::compiler::native_id::REFLECT_CLONE)
            .contains(&native_id)
    }

    pub(in crate::vm::interpreter) fn native_callable_uses_builtin_this_coercion(
        &self,
        native_id: u16,
    ) -> bool {
        crate::vm::builtin::is_array_method(native_id)
            || crate::vm::builtin::is_string_method(native_id)
            || crate::vm::builtin::is_number_method(native_id)
    }

    pub(in crate::vm::interpreter) fn builtin_native_this_value(
        &mut self,
        receiver: Value,
        native_id: u16,
    ) -> Result<Value, VmError> {
        if !self.native_callable_uses_builtin_this_coercion(native_id) {
            return Ok(receiver);
        }
        // Don't box primitives for native builtin methods — they already
        // handle raw primitive values directly (string handlers expect
        // RayaString, number handlers expect numeric values).  Boxing
        // would wrap the primitive in an object whose toString/valueOf
        // produces the wrong representation (e.g., "[object String]").
        // Boxing is only needed for user-defined methods on primitives,
        // which are not dispatched through this path.
        Ok(receiver)
    }

    fn intrinsic_callable_function_info(&self, target: Value) -> Option<(String, usize)> {
        let target = self
            .unwrapped_proxy_like(target)
            .map(|proxy| proxy.target)
            .unwrap_or(target);

        let raw_ptr = unsafe { target.as_ptr::<u8>() }?;
        let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };

        if header.type_id() == std::any::TypeId::of::<Object>() {
            let co = unsafe { &*target.as_ptr::<Object>()?.as_ptr() };
            let callable_data = co.callable.as_ref()?;
            match &callable_data.kind {
                CallableKind::Closure { func_id } => {
                    let module = co.callable_module()?;
                    if std::env::var("RAYA_DEBUG_DYNAMIC_FUNCTION").is_ok() {
                        eprintln!(
                            "[callable-info] closure target={:#x} func_id={} module={}",
                            target.raw(),
                            func_id,
                            module.metadata.name
                        );
                    }
                    let function = module.functions.get(*func_id)?;
                    if module.metadata.name.starts_with("__dynamic_function__/") {
                        return Some(("anonymous".to_string(), function.visible_length));
                    }
                    return Some((
                        Self::visible_function_name(&function.name),
                        function.visible_length,
                    ));
                }
                CallableKind::BoundMethod { func_id, .. } => {
                    let module = co.callable_module()?;
                    if std::env::var("RAYA_DEBUG_BIND_METHOD").is_ok() {
                        eprintln!(
                            "[bind-method-callable-info] target={:#x} func_id={} module={}",
                            target.raw(),
                            func_id,
                            module.metadata.name
                        );
                    }
                    let function = module.functions.get(*func_id)?;
                    return Some((
                        Self::visible_function_name(&function.name),
                        function.visible_length,
                    ));
                }
                CallableKind::BoundNative { native_id, .. } => {
                    let raw_name = crate::compiler::native_id::native_name(*native_id);
                    let visible_name = raw_name.rsplit('.').next().unwrap_or(raw_name).to_string();
                    let arity = match *native_id {
                        crate::compiler::native_id::FUNCTION_CALL_HELPER => 1,
                        crate::compiler::native_id::FUNCTION_APPLY_HELPER => 2,
                        crate::compiler::native_id::FUNCTION_BIND_HELPER => 1,
                        _ => 0,
                    };
                    return Some((visible_name, arity));
                }
                CallableKind::Bound {
                    visible_name,
                    visible_length,
                    ..
                } => {
                    let length = if let Some(v) = visible_length.as_i32() {
                        v.max(0) as usize
                    } else if let Some(v) = visible_length.as_i64() {
                        v.max(0) as usize
                    } else if let Some(v) = visible_length.as_f64() {
                        if !v.is_finite() {
                            usize::MAX
                        } else {
                            v.max(0.0).floor().min(usize::MAX as f64) as usize
                        }
                    } else {
                        0
                    };
                    return Some((visible_name.clone(), length));
                }
            }
        }

        if let Some(nominal_type_id) = self.constructor_nominal_type_id(target) {
            let classes = self.classes.read();
            let class = classes.get_class(nominal_type_id)?;
            let visible_name = class.name.clone();
            let builtin_arity = crate::vm::builtins::builtin_visible_constructor_length(
                &visible_name,
            )
            .or_else(|| {
                crate::vm::builtins::get_all_signatures()
                    .iter()
                    .flat_map(|sig| sig.classes.iter())
                    .find(|sig| sig.name == visible_name)
                    .and_then(|sig| sig.constructor.map(|ctor| ctor.len()))
            });
            let runtime_arity = class
                .get_constructor()
                .and_then(|constructor_id| {
                    class
                        .module
                        .as_ref()
                        .and_then(|module| module.functions.get(constructor_id))
                        .map(|function| function.visible_length)
                })
                .unwrap_or(0);
            let arity = builtin_arity.unwrap_or(runtime_arity);
            return Some((visible_name, arity));
        }

        None
    }

    pub(in crate::vm::interpreter) fn callable_function_info(
        &self,
        target: Value,
    ) -> Option<(String, usize)> {
        self.intrinsic_callable_function_info(target)
    }

    fn callable_function_family_name(&self, target: Value) -> Option<&'static str> {
        let target = self
            .unwrapped_proxy_like(target)
            .map(|proxy| proxy.target)
            .unwrap_or(target);
        let co_ptr = checked_object_ptr(target)?;
        let co = unsafe { &*co_ptr.as_ptr() };
        let callable = co.callable.as_ref()?;
        match &callable.kind {
            CallableKind::Closure { func_id } | CallableKind::BoundMethod { func_id, .. } => {
                let module = co.callable_module()?;
                let function = module.functions.get(*func_id)?;
                Some(match (function.is_async, function.is_generator) {
                    (true, true) => "AsyncGeneratorFunction",
                    (true, false) => "AsyncFunction",
                    (false, true) => "GeneratorFunction",
                    (false, false) => "Function",
                })
            }
            _ => None,
        }
    }

    fn callable_observable_name_with_context(
        &mut self,
        target: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<String, VmError> {
        let observed = self.get_property_value_via_js_semantics_with_context(
            target,
            "name",
            caller_task,
            caller_module,
        )?;
        let Some(value) = observed else {
            return Ok(String::new());
        };
        if let Some(ptr) = checked_string_ptr(value) {
            return Ok(unsafe { &*ptr.as_ptr() }.data.clone());
        }
        Ok(String::new())
    }

    fn callable_observable_length_with_context(
        &mut self,
        target: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
        bound_arg_count: usize,
    ) -> Result<Value, VmError> {
        if !self.has_own_property_via_js_semantics(target, "length") {
            return Ok(Value::i32(0));
        }
        let observed = self.get_own_property_value_via_js_semantics_with_context(
            target,
            "length",
            caller_task,
            caller_module,
        )?;
        let Some(value) = observed else {
            return Ok(Value::i32(0));
        };
        let number = if let Some(v) = value.as_i32() {
            v as f64
        } else if let Some(v) = value.as_i64() {
            v as f64
        } else if let Some(v) = value.as_f64() {
            v
        } else {
            return Ok(Value::i32(0));
        };
        if number.is_nan() || number == 0.0 {
            return Ok(Value::i32(0));
        }
        if number == f64::INFINITY {
            return Ok(Value::f64(f64::INFINITY));
        }
        if number == f64::NEG_INFINITY {
            return Ok(Value::i32(0));
        }
        let length = (number.floor() - bound_arg_count as f64).max(0.0);
        if length <= i32::MAX as f64 {
            Ok(Value::i32(length as i32))
        } else {
            Ok(Value::f64(length))
        }
    }

    pub(in crate::vm::interpreter) fn callable_is_constructible(&self, target: Value) -> bool {
        let target = self
            .unwrapped_proxy_like(target)
            .map(|proxy| proxy.target)
            .unwrap_or(target);

        let Some(raw_ptr) = (unsafe { target.as_ptr::<u8>() }) else {
            return false;
        };
        let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };

        if header.type_id() == std::any::TypeId::of::<Object>() {
            let Some(co_ptr) = (unsafe { target.as_ptr::<Object>() }) else {
                return false;
            };
            let co = unsafe { &*co_ptr.as_ptr() };
            if let Some(ref cd) = co.callable {
                match &cd.kind {
                    CallableKind::Closure { func_id } => {
                        let Some(module) = co.callable_module() else {
                            return false;
                        };
                        return module
                            .functions
                            .get(*func_id)
                            .is_some_and(|f| f.is_constructible);
                    }
                    CallableKind::Bound { target: t, .. } => {
                        return self.callable_is_constructible(*t);
                    }
                    _ => {}
                }
            }
        }

        self.constructor_nominal_type_id(target).is_some()
            || self.builtin_global_name_for_value(target).is_some()
    }

    fn callable_exposes_default_prototype(&self, target: Value) -> bool {
        let target = self
            .unwrapped_proxy_like(target)
            .map(|proxy| proxy.target)
            .unwrap_or(target);

        let Some(raw_ptr) = (unsafe { target.as_ptr::<u8>() }) else {
            return false;
        };
        let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };

        if header.type_id() == std::any::TypeId::of::<Object>() {
            let Some(co_ptr) = (unsafe { target.as_ptr::<Object>() }) else {
                return false;
            };
            let co = unsafe { &*co_ptr.as_ptr() };
            if let Some(ref cd) = co.callable {
                match &cd.kind {
                    CallableKind::Closure { func_id } => {
                        let Some(module) = co.callable_module() else {
                            return false;
                        };
                        return module
                            .functions
                            .get(*func_id)
                            .is_some_and(|f| f.is_constructible || f.is_generator);
                    }
                    CallableKind::Bound { target: t, .. } => {
                        return self.callable_exposes_default_prototype(*t);
                    }
                    _ => {}
                }
            }
        }

        self.constructor_nominal_type_id(target).is_some()
            || self.builtin_global_name_for_value(target).is_some()
    }

    pub(in crate::vm::interpreter) fn callable_is_strict_js(&self, target: Value) -> bool {
        let target = self
            .unwrapped_proxy_like(target)
            .map(|proxy| proxy.target)
            .unwrap_or(target);

        let Some(raw_ptr) = (unsafe { target.as_ptr::<u8>() }) else {
            return false;
        };
        let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };

        if header.type_id() == std::any::TypeId::of::<Object>() {
            let Some(co_ptr) = (unsafe { target.as_ptr::<Object>() }) else {
                return false;
            };
            let co = unsafe { &*co_ptr.as_ptr() };
            if let Some(ref cd) = co.callable {
                match &cd.kind {
                    CallableKind::Closure { func_id }
                    | CallableKind::BoundMethod { func_id, .. } => {
                        let Some(module) = co.callable_module() else {
                            return false;
                        };
                        return module
                            .functions
                            .get(*func_id)
                            .is_some_and(|f| f.is_strict_js);
                    }
                    CallableKind::Bound { target: t, .. } => {
                        return self.callable_is_strict_js(*t);
                    }
                    _ => return false,
                }
            }
        }

        false
    }

    pub(in crate::vm::interpreter) fn callable_is_arrow_function(&self, target: Value) -> bool {
        let target = self
            .unwrapped_proxy_like(target)
            .map(|proxy| proxy.target)
            .unwrap_or(target);

        let Some(raw_ptr) = (unsafe { target.as_ptr::<u8>() }) else {
            return false;
        };
        let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };

        if header.type_id() == std::any::TypeId::of::<Object>() {
            let Some(co_ptr) = (unsafe { target.as_ptr::<Object>() }) else {
                return false;
            };
            let co = unsafe { &*co_ptr.as_ptr() };
            if let Some(ref cd) = co.callable {
                match &cd.kind {
                    CallableKind::Closure { func_id }
                    | CallableKind::BoundMethod { func_id, .. } => {
                        let Some(module) = co.callable_module() else {
                            return false;
                        };
                        return module
                            .functions
                            .get(*func_id)
                            .is_some_and(|f| f.name.starts_with("__arrow_"));
                    }
                    CallableKind::Bound { target: t, .. } => {
                        return self.callable_is_arrow_function(*t);
                    }
                    _ => return false,
                }
            }
        }

        false
    }

    fn callable_has_legacy_caller_arguments_own_props(&self, target: Value) -> bool {
        let target = self
            .unwrapped_proxy_like(target)
            .map(|proxy| proxy.target)
            .unwrap_or(target);

        let Some(raw_ptr) = (unsafe { target.as_ptr::<u8>() }) else {
            return false;
        };
        let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };

        if header.type_id() != std::any::TypeId::of::<Object>() {
            return false;
        }

        let Some(co_ptr) = (unsafe { target.as_ptr::<Object>() }) else {
            return false;
        };
        let co = unsafe { &*co_ptr.as_ptr() };
        let Some(cd) = co.callable.as_ref() else {
            return false;
        };

        match &cd.kind {
            CallableKind::Closure { func_id } => {
                let Some(module) = co.callable_module() else {
                    return false;
                };
                let Some(function) = module.functions.get(*func_id) else {
                    return false;
                };
                !function.is_strict_js
                    && !function.is_async
                    && !function.is_generator
                    && !function.name.starts_with("__arrow_")
                    && !module.metadata.name.starts_with("__dynamic_function__/")
            }
            CallableKind::BoundMethod { .. }
            | CallableKind::BoundNative { .. }
            | CallableKind::Bound { .. } => false,
        }
    }

    fn callable_matches_function_identity(
        &self,
        value: Value,
        func_id: usize,
        module: &Module,
    ) -> bool {
        let Some(obj_ptr) = checked_object_ptr(value) else {
            return false;
        };
        let obj = unsafe { &*obj_ptr.as_ptr() };
        if !obj.is_callable() || obj.callable_func_id() != Some(func_id) {
            return false;
        }
        obj.callable_module()
            .as_ref()
            .is_some_and(|callable_module| callable_module.checksum == module.checksum)
    }

    fn current_callable_value_for_function(
        &self,
        stack: &Stack,
        module: &Module,
        task: &Arc<Task>,
        func_id: usize,
    ) -> Option<Value> {
        if let Some(closure) = task.current_closure() {
            if self.callable_matches_function_identity(closure, func_id, module) {
                return Some(closure);
            }
        }

        if let Some(frame) = stack.current_frame() {
            if let Some(closure) = frame.closure {
                if self.callable_matches_function_identity(closure, func_id, module) {
                    return Some(closure);
                }
            }

            let frame_end = frame.base_pointer.saturating_add(frame.local_count);
            for slot in frame.base_pointer..frame_end {
                let Ok(value) = stack.peek_at(slot) else {
                    continue;
                };
                if self.callable_matches_function_identity(value, func_id, module) {
                    return Some(value);
                }
            }
        }

        for value in self.globals_by_index.read().iter().copied() {
            if self.callable_matches_function_identity(value, func_id, module) {
                return Some(value);
            }
        }

        None
    }

    pub(in crate::vm::interpreter) fn current_js_code_is_strict(
        &self,
        task: &Arc<Task>,
        module: &Module,
    ) -> bool {
        if task.current_active_direct_eval_is_strict() {
            return true;
        }
        module
            .functions
            .get(task.current_func_id())
            .is_some_and(|function| function.is_strict_js)
    }

    pub(in crate::vm::interpreter) fn callable_uses_builtin_this_coercion(
        &self,
        target: Value,
    ) -> bool {
        let target = self
            .unwrapped_proxy_like(target)
            .map(|proxy| proxy.target)
            .unwrap_or(target);

        let Some(raw_ptr) = (unsafe { target.as_ptr::<u8>() }) else {
            return false;
        };
        let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };

        if header.type_id() == std::any::TypeId::of::<Object>() {
            let Some(co_ptr) = (unsafe { target.as_ptr::<Object>() }) else {
                return false;
            };
            let co = unsafe { &*co_ptr.as_ptr() };
            if let Some(ref cd) = co.callable {
                match &cd.kind {
                    CallableKind::Closure { func_id }
                    | CallableKind::BoundMethod { func_id, .. } => {
                        let Some(module) = co.callable_module() else {
                            return false;
                        };
                        return module
                            .functions
                            .get(*func_id)
                            .is_some_and(|function| function.uses_builtin_this_coercion);
                    }
                    CallableKind::Bound { target: t, .. } => {
                        return self.callable_uses_builtin_this_coercion(*t);
                    }
                    _ => return false,
                }
            }
        }

        false
    }

    fn box_js_this_primitive(&mut self, this_value: Value) -> Result<Option<Value>, VmError> {
        // Raya models JS symbols as dedicated symbol-instance objects already, so
        // re-boxing them for `this` coercion produces a wrapper that loses the
        // instance's own field layout/prototype semantics.
        if self.is_symbol_value(this_value) {
            return Ok(None);
        }
        if let Some(constructor) = self.builtin_global_value("BigInt") {
            if checked_bigint_ptr(this_value).is_some() {
                return self
                    .alloc_boxed_primitive_object(constructor, "BigInt", this_value)
                    .map(Some);
            }
        }
        if let Some(constructor) = self.builtin_global_value("Number") {
            if this_value.as_i32().is_some()
                || this_value.as_i64().is_some()
                || this_value.as_f64().is_some()
            {
                let numeric = this_value
                    .as_f64()
                    .or_else(|| this_value.as_i64().map(|value| value as f64))
                    .or_else(|| this_value.as_i32().map(|value| value as f64))
                    .map(Value::f64)
                    .unwrap_or(this_value);
                return self
                    .alloc_boxed_primitive_object(constructor, "Number", numeric)
                    .map(Some);
            }
        }
        if let Some(boolean) = this_value.as_bool() {
            if let Some(constructor) = self.builtin_global_value("Boolean") {
                return self
                    .alloc_boxed_primitive_object(constructor, "Boolean", Value::bool(boolean))
                    .map(Some);
            }
        }
        if let Some(string_ptr) = checked_string_ptr(this_value) {
            if let Some(constructor) = self.builtin_global_value("String") {
                let string_value = unsafe { Value::from_ptr(string_ptr) };
                return self
                    .alloc_boxed_primitive_object(constructor, "String", string_value)
                    .map(Some);
            }
        }
        Ok(None)
    }

    pub(in crate::vm::interpreter) fn js_this_value_for_callable(
        &mut self,
        callable: Value,
        explicit_this: Option<Value>,
    ) -> Result<Value, VmError> {
        let this_value = explicit_this.unwrap_or(Value::undefined());
        if self.callable_uses_builtin_this_coercion(callable) {
            if let Some(boxed) = self.box_js_this_primitive(this_value)? {
                return Ok(boxed);
            }
            return Ok(this_value);
        }
        if self.callable_is_strict_js(callable) {
            return Ok(this_value);
        }
        if this_value.is_null() || this_value.is_undefined() {
            return Ok(self
                .builtin_global_value("globalThis")
                .unwrap_or(Value::undefined()));
        }
        if let Some(boxed) = self.box_js_this_primitive(this_value)? {
            return Ok(boxed);
        }
        Ok(this_value)
    }

    fn callable_native_alias_id(&self, callable: Value) -> Option<u16> {
        let raw_ptr = unsafe { callable.as_ptr::<u8>() }?;
        let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };

        if header.type_id() == std::any::TypeId::of::<Object>() {
            let co = unsafe { &*callable.as_ptr::<Object>()?.as_ptr() };
            let cd = co.callable.as_ref()?;
            match &cd.kind {
                CallableKind::Closure { func_id } => {
                    let module = co.callable_module()?;
                    let function = module.functions.get(*func_id)?;
                    return Self::function_native_alias_id(&function.name);
                }
                CallableKind::BoundMethod { func_id, .. } => {
                    let module = co.callable_module()?;
                    let function = module.functions.get(*func_id)?;
                    return Self::function_native_alias_id(&function.name);
                }
                CallableKind::BoundNative { native_id, .. } => {
                    return Some(*native_id);
                }
                CallableKind::Bound { target, .. } => {
                    return self.callable_native_alias_id(*target);
                }
            }
        }

        None
    }

    pub(in crate::vm::interpreter) fn callable_uses_js_this_slot(&self, callable: Value) -> bool {
        let Some(raw_ptr) = (unsafe { callable.as_ptr::<u8>() }) else {
            return false;
        };
        let header = unsafe { &*header_ptr_from_value_ptr(raw_ptr.as_ptr()) };
        if header.type_id() == std::any::TypeId::of::<Object>() {
            let co = unsafe { &*callable.as_ptr::<Object>().unwrap().as_ptr() };
            if let Some(ref cd) = co.callable {
                match &cd.kind {
                    CallableKind::Closure { func_id }
                    | CallableKind::BoundMethod { func_id, .. } => {
                        let Some(module) = co.callable_module() else {
                            return false;
                        };
                        return module
                            .functions
                            .get(*func_id)
                            .map(|f| f.uses_js_this_slot)
                            .unwrap_or(false);
                    }
                    CallableKind::BoundNative { native_id, .. } => {
                        return self.native_callable_uses_receiver(*native_id);
                    }
                    CallableKind::Bound { target, .. } => {
                        return self.callable_uses_js_this_slot(*target);
                    }
                }
            }
        }
        false
    }

    fn alloc_bound_function(
        &mut self,
        target: Value,
        this_arg: Value,
        bound_args: Vec<Value>,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Value, VmError> {
        let rebind_call_helper = self.callable_native_alias_id(target)
            == Some(crate::compiler::native_id::FUNCTION_CALL_HELPER);
        let target_name =
            self.callable_observable_name_with_context(target, caller_task, caller_module)?;
        let visible_name = format!("bound {}", target_name);
        let visible_length = self.callable_observable_length_with_context(
            target,
            caller_task,
            caller_module,
            bound_args.len(),
        )?;
        let bound = Object::new_bound_function(
            target,
            this_arg,
            bound_args,
            visible_name,
            visible_length,
            rebind_call_helper,
        );
        let bound_ptr = self.gc.lock().allocate(bound);
        Ok(unsafe {
            Value::from_ptr(std::ptr::NonNull::new(bound_ptr.as_ptr()).expect("bound function ptr"))
        })
    }

    pub(in crate::vm::interpreter) fn dispatch_call_with_explicit_this(
        &mut self,
        stack: &mut Stack,
        target_callable: Value,
        this_arg: Value,
        rest_args: Vec<Value>,
        module: &Module,
        task: &Arc<Task>,
        non_callable_message: &'static str,
    ) -> OpcodeResult {
        if self.callable_native_alias_id(target_callable)
            == Some(crate::compiler::native_id::FUNCTION_CALL_HELPER)
        {
            let rebound_target = this_arg;
            let rebound_this = rest_args.first().copied().unwrap_or(Value::undefined());
            let rebound_rest = if rest_args.len() > 1 {
                rest_args[1..].to_vec()
            } else {
                Vec::new()
            };
            return self.dispatch_call_with_explicit_this(
                stack,
                rebound_target,
                rebound_this,
                rebound_rest,
                module,
                task,
                non_callable_message,
            );
        }

        match self.callable_frame_for_value(
            target_callable,
            stack,
            &rest_args,
            Some(this_arg),
            ReturnAction::PushReturnValue,
            module,
            task,
        ) {
            Ok(Some(frame)) => frame,
            Ok(None) => OpcodeResult::Error(VmError::TypeError(non_callable_message.to_string())),
            Err(error) => OpcodeResult::Error(error),
        }
    }

    fn delete_property_from_target(
        &mut self,
        target: Value,
        key: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<bool, VmError> {
        let (key_str, _) =
            self.property_key_parts_with_context(key, "Object.deleteProperty", task, module)?;
        let Some(key_name) = key_str else {
            return Ok(true);
        };

        if let Some(proxy) = crate::vm::reflect::try_unwrap_proxy(target) {
            if proxy.handler.is_null() {
                return Err(VmError::TypeError("Proxy has been revoked".to_string()));
            }
            if let Some(trap) = self.get_field_value_by_name(proxy.handler, "deleteProperty") {
                if !trap.is_undefined() && !trap.is_null() {
                    let trap_result = self
                        .invoke_proxy_property_trap_with_context(
                            trap,
                            proxy.handler,
                            proxy.target,
                            &key_name,
                            &[],
                            task,
                            module,
                        )?
                        .is_truthy();
                    self.enforce_proxy_delete_invariants(
                        proxy.target,
                        &key_name,
                        trap_result,
                        task,
                        module,
                    )?;
                    return Ok(trap_result);
                }
            }
            return self.delete_property_from_target(proxy.target, key, task, module);
        }

        if !target.is_ptr() {
            return Ok(true);
        }

        let own_shape = self.resolve_own_property_shape(target, &key_name);
        if let Some(shape) = own_shape {
            if !shape.configurable {
                return Ok(false);
            }
        } else {
            return Ok(true);
        }

        if let Some(kind) = self.exotic_adapter_kind(target) {
            if let Some(result) = self.exotic_delete_own_property(kind, target, &key_name)? {
                return Ok(result);
            }
        }

        let has_runtime_global_source = self.is_runtime_global_object(target)
            && self.builtin_global_slots.read().contains_key(&key_name);
        let has_callable_virtual_source = self.is_callable_virtual_property(target, &key_name);
        let has_constructor_static_source = self.has_constructor_static_method(target, &key_name);
        let has_fixed_field_source = self.get_field_index_for_value(target, &key_name).is_some();

        let mut removed = false;
        if let Some(obj_ptr) = checked_object_ptr(target) {
            let obj = unsafe { &mut *obj_ptr.as_ptr() };
            if let Some(dp) = obj.dyn_props_mut() {
                removed = dp.remove(self.intern_prop_key(&key_name)).is_some();
            }
        }

        let dynamic_value_removed = {
            let mut metadata = self.metadata.lock();
            let value_removed = metadata.delete_metadata_property(
                NON_OBJECT_DYNAMIC_VALUE_METADATA_KEY,
                target,
                &key_name,
            );
            let descriptor_removed = metadata.delete_metadata_property(
                NON_OBJECT_DESCRIPTOR_METADATA_KEY,
                target,
                &key_name,
            );
            value_removed || descriptor_removed
        };

        if removed || dynamic_value_removed {
            self.set_callable_virtual_property_deleted(
                target,
                &key_name,
                has_callable_virtual_source,
            );
            self.set_fixed_property_deleted(
                target,
                &key_name,
                has_runtime_global_source
                    || has_constructor_static_source
                    || has_fixed_field_source,
            );
            return Ok(true);
        }

        if has_runtime_global_source {
            self.set_fixed_property_deleted(target, &key_name, true);
            return Ok(true);
        }

        if has_callable_virtual_source {
            self.set_callable_virtual_property_deleted(target, &key_name, true);
            return Ok(true);
        }

        if let Some(index) = self.get_field_index_for_value(target, &key_name) {
            if let Some(obj_ptr) = checked_object_ptr(target) {
                let obj = unsafe { &mut *obj_ptr.as_ptr() };
                let _ = obj.set_field(index, Value::undefined());
            }
            self.set_fixed_property_deleted(target, &key_name, true);
            return Ok(true);
        }

        Ok(false)
    }

    fn make_arguments_object(
        &mut self,
        stack: &Stack,
        module: &Module,
        task: &Arc<Task>,
    ) -> Result<Value, VmError> {
        let current_func_id = task.current_func_id();
        let locals_base = task.current_locals_base();
        let current_arg_count = task.current_arg_count();
        let function = module.functions.get(current_func_id).ok_or_else(|| {
            VmError::RuntimeError(format!(
                "Object.getArgumentsObject could not resolve function {}",
                current_func_id
            ))
        })?;

        let user_arg_offset = usize::from(function.uses_js_this_slot);
        let user_arg_count = current_arg_count.saturating_sub(user_arg_offset);
        let mut values = Vec::with_capacity(user_arg_count);
        for index in 0..user_arg_count {
            values.push(
                stack
                    .peek_at(locals_base + user_arg_offset + index)
                    .unwrap_or(Value::undefined()),
            );
        }

        let mut mapped_refcells = vec![None; user_arg_count];
        for (index, &local_idx) in function.js_arguments_mapping.iter().enumerate() {
            if index >= user_arg_count || local_idx == u16::MAX {
                continue;
            }
            mapped_refcells[index] = Some(
                stack
                    .peek_at(locals_base + local_idx as usize)
                    .unwrap_or(Value::undefined()),
            );
        }

        let callee = if function.is_strict_js {
            None
        } else {
            let callee_value = self
                .current_callable_value_for_function(stack, module, task, current_func_id)
                .unwrap_or_else(|| {
                    let closure = Object::new_closure_with_module(
                        current_func_id,
                        Vec::new(),
                        Arc::new(module.clone()),
                    );
                    let closure_ptr = self.gc.lock().allocate(closure);
                    unsafe {
                        Value::from_ptr(NonNull::new(closure_ptr.as_ptr()).expect("closure ptr"))
                    }
                });
            Some(ArgumentsDataProperty::new(callee_value, true, false, true))
        };

        let mut object = Object::new_dynamic(layout_id_from_ordered_names(&[]), 0);
        object.header.exotic_kind = ExoticKind::Arguments;
        object.arguments = Some(Box::new(ArgumentsObjectData {
            values,
            mapped_refcells,
            indexed: vec![ArgumentsIndexedProperty::mapped_default(); user_arg_count],
            length: Some(ArgumentsDataProperty::new(
                if user_arg_count <= i32::MAX as usize {
                    Value::i32(user_arg_count as i32)
                } else {
                    Value::f64(user_arg_count as f64)
                },
                true,
                false,
                true,
            )),
            callee,
            strict_poison: function.is_strict_js,
        }));
        if let Some(object_ctor) = self.builtin_global_value("Object") {
            if let Some(prototype) = self.constructor_prototype_value(object_ctor) {
                object.prototype = prototype;
            }
        }
        let object_ptr = self.gc.lock().allocate(object);
        let arguments_value =
            unsafe { Value::from_ptr(NonNull::new(object_ptr.as_ptr()).expect("arguments ptr")) };

        if let Some(array_ctor) = self.builtin_global_value("Array") {
            if let Some(array_prototype) = self.constructor_prototype_value(array_ctor) {
                if let Some(iterator) = self
                    .get_field_value_by_name(array_prototype, "Symbol.iterator")
                    .or_else(|| self.get_field_value_by_name(array_prototype, "values"))
                {
                    let _ = self.define_data_property_on_target(
                        arguments_value,
                        "Symbol.iterator",
                        iterator,
                        true,
                        false,
                        true,
                    );
                }
            }
        }

        Ok(arguments_value)
    }

    fn arguments_descriptor_record(&self, descriptor: Value) -> JsPropertyDescriptorRecord {
        let mut record = JsPropertyDescriptorRecord::default();
        if self.descriptor_field_present(descriptor, "value") {
            record.has_value = true;
            record.value = self
                .get_field_value_by_name(descriptor, "value")
                .unwrap_or(Value::undefined());
        }
        if self.descriptor_field_present(descriptor, "writable") {
            record.has_writable = true;
            record.writable = self.descriptor_flag(descriptor, "writable", false);
        }
        if self.descriptor_field_present(descriptor, "enumerable") {
            record.has_enumerable = true;
            record.enumerable = self.descriptor_flag(descriptor, "enumerable", false);
        }
        if self.descriptor_field_present(descriptor, "configurable") {
            record.has_configurable = true;
            record.configurable = self.descriptor_flag(descriptor, "configurable", false);
        }
        if self.descriptor_field_present(descriptor, "get") {
            record.has_get = true;
            record.get = self
                .get_field_value_by_name(descriptor, "get")
                .unwrap_or(Value::undefined());
        }
        if self.descriptor_field_present(descriptor, "set") {
            record.has_set = true;
            record.set = self
                .get_field_value_by_name(descriptor, "set")
                .unwrap_or(Value::undefined());
        }
        record
    }

    fn arguments_exotic_index_value(
        &self,
        arguments: &ArgumentsObjectData,
        index: usize,
    ) -> Option<Value> {
        let property = arguments.indexed.get(index)?;
        if property.deleted {
            return None;
        }
        if let Some(Some(refcell_value)) = arguments.mapped_refcells.get(index) {
            if let Some(refcell_ptr) = unsafe { refcell_value.as_ptr::<RefCell>() } {
                let refcell = unsafe { &*refcell_ptr.as_ptr() };
                return Some(refcell.get());
            }
        }
        arguments.values.get(index).copied()
    }

    fn arguments_exotic_disconnect_index_mapping(
        &self,
        arguments: &mut ArgumentsObjectData,
        index: usize,
        current_value: Value,
    ) {
        if let Some(mapped) = arguments.mapped_refcells.get_mut(index) {
            *mapped = None;
        }
        if let Some(value_slot) = arguments.values.get_mut(index) {
            *value_slot = current_value;
        }
    }

    fn validate_existing_data_descriptor(
        &self,
        key: &str,
        current: ArgumentsDataDescriptorState,
        record: JsPropertyDescriptorRecord,
    ) -> Result<(), VmError> {
        if !current.configurable {
            if record.has_configurable && record.configurable {
                return Err(VmError::TypeError(format!(
                    "Cannot redefine non-configurable property '{}'",
                    key
                )));
            }
            if record.has_enumerable && record.enumerable != current.enumerable {
                return Err(VmError::TypeError(format!(
                    "Cannot redefine non-configurable property '{}'",
                    key
                )));
            }
            if !current.writable {
                if record.has_writable && record.writable {
                    return Err(VmError::TypeError(format!(
                        "Cannot redefine non-writable property '{}'",
                        key
                    )));
                }
                if record.has_value && !value_same_value(record.value, current.value) {
                    return Err(VmError::TypeError(format!(
                        "Cannot redefine non-writable property '{}'",
                        key
                    )));
                }
            }
        }
        Ok(())
    }

    fn validate_existing_property_descriptor(
        &self,
        key: &str,
        current: OrdinaryOwnProperty,
        record: JsPropertyDescriptorRecord,
    ) -> Result<(), VmError> {
        match current {
            OrdinaryOwnProperty::Data {
                value,
                writable,
                enumerable,
                configurable,
            } => {
                if !configurable {
                    if record.has_configurable && record.configurable {
                        return Err(VmError::TypeError(format!(
                            "Cannot redefine non-configurable property '{}'",
                            key
                        )));
                    }
                    if record.has_enumerable && record.enumerable != enumerable {
                        return Err(VmError::TypeError(format!(
                            "Cannot redefine non-configurable property '{}'",
                            key
                        )));
                    }
                    if record.is_accessor() {
                        return Err(VmError::TypeError(format!(
                            "Cannot redefine non-configurable property '{}'",
                            key
                        )));
                    }
                    if !writable {
                        if record.has_writable && record.writable {
                            return Err(VmError::TypeError(format!(
                                "Cannot redefine non-writable property '{}'",
                                key
                            )));
                        }
                        if record.has_value && !value_same_value(record.value, value) {
                            return Err(VmError::TypeError(format!(
                                "Cannot redefine non-writable property '{}'",
                                key
                            )));
                        }
                    }
                }
            }
            OrdinaryOwnProperty::Accessor {
                get,
                set,
                enumerable,
                configurable,
            } => {
                if !configurable {
                    if record.has_configurable && record.configurable {
                        return Err(VmError::TypeError(format!(
                            "Cannot redefine non-configurable property '{}'",
                            key
                        )));
                    }
                    if record.has_enumerable && record.enumerable != enumerable {
                        return Err(VmError::TypeError(format!(
                            "Cannot redefine non-configurable property '{}'",
                            key
                        )));
                    }
                    if record.is_data() {
                        return Err(VmError::TypeError(format!(
                            "Cannot redefine non-configurable property '{}'",
                            key
                        )));
                    }
                    if record.has_get && !value_same_value(record.get, get) {
                        return Err(VmError::TypeError(format!(
                            "Cannot redefine non-configurable property '{}'",
                            key
                        )));
                    }
                    if record.has_set && !value_same_value(record.set, set) {
                        return Err(VmError::TypeError(format!(
                            "Cannot redefine non-configurable property '{}'",
                            key
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    fn ordinary_own_property_from_descriptor_value(
        &self,
        descriptor: Value,
    ) -> Option<OrdinaryOwnProperty> {
        let record = self.descriptor_record_from_descriptor(descriptor);
        if record.is_accessor() {
            Some(OrdinaryOwnProperty::Accessor {
                get: if record.has_get {
                    record.get
                } else {
                    Value::undefined()
                },
                set: if record.has_set {
                    record.set
                } else {
                    Value::undefined()
                },
                enumerable: if record.has_enumerable {
                    record.enumerable
                } else {
                    false
                },
                configurable: if record.has_configurable {
                    record.configurable
                } else {
                    false
                },
            })
        } else if record.is_data() {
            Some(OrdinaryOwnProperty::Data {
                value: if record.has_value {
                    record.value
                } else {
                    Value::undefined()
                },
                writable: if record.has_writable {
                    record.writable
                } else {
                    false
                },
                enumerable: if record.has_enumerable {
                    record.enumerable
                } else {
                    false
                },
                configurable: if record.has_configurable {
                    record.configurable
                } else {
                    false
                },
            })
        } else {
            None
        }
    }

    fn shape_from_ordinary_property(
        &self,
        source: JsOwnPropertySource,
        property: OrdinaryOwnProperty,
    ) -> JsOwnPropertyShape {
        match property {
            OrdinaryOwnProperty::Data {
                writable,
                enumerable,
                configurable,
                ..
            } => JsOwnPropertyShape::data(source, writable, configurable, enumerable),
            OrdinaryOwnProperty::Accessor {
                enumerable,
                configurable,
                ..
            } => JsOwnPropertyShape::accessor(source, configurable, enumerable),
        }
    }

    fn arguments_exotic_define_own_property(
        &mut self,
        target: Value,
        key: &str,
        descriptor: Value,
    ) -> Result<Option<()>, VmError> {
        let Some(obj_ptr) = checked_object_ptr(target) else {
            return Ok(None);
        };
        let record = self.arguments_descriptor_record(descriptor);

        if key == "length" {
            let obj = unsafe { &mut *obj_ptr.as_ptr() };
            let Some(arguments) = obj.arguments.as_deref_mut() else {
                return Ok(None);
            };
            let Some(length) = arguments.length.as_mut() else {
                return Ok(None);
            };
            if record.is_accessor() {
                return Err(VmError::TypeError(
                    "Invalid property descriptor for 'length': cannot mix accessors and value"
                        .to_string(),
                ));
            }
            self.validate_existing_data_descriptor(
                key,
                ArgumentsDataDescriptorState {
                    value: length.value,
                    writable: length.writable,
                    enumerable: length.enumerable,
                    configurable: length.configurable,
                },
                record,
            )?;
            if record.has_value {
                length.value = record.value;
            }
            if record.has_writable {
                length.writable = record.writable;
            }
            if record.has_enumerable {
                length.enumerable = record.enumerable;
            }
            if record.has_configurable {
                length.configurable = record.configurable;
            }
            return Ok(Some(()));
        }

        if key == "callee" {
            let mut install_accessor = None;
            {
                let obj = unsafe { &mut *obj_ptr.as_ptr() };
                let Some(arguments) = obj.arguments.as_deref_mut() else {
                    return Ok(None);
                };
                if arguments.strict_poison {
                    let only_same_shape = (!record.has_configurable || !record.configurable)
                        && (!record.has_enumerable || !record.enumerable)
                        && !record.has_value
                        && !record.has_writable;
                    if !only_same_shape {
                        return Err(VmError::TypeError(
                            "Cannot redefine non-configurable property 'callee'".to_string(),
                        ));
                    }
                    return Ok(Some(()));
                }
                if record.is_accessor() {
                    if arguments.callee.is_none() {
                        return Ok(None);
                    }
                    arguments.callee = None;
                    install_accessor = Some((
                        record.get,
                        record.set,
                        record.enumerable,
                        record.configurable,
                    ));
                } else {
                    let Some(callee) = arguments.callee.as_mut() else {
                        return Ok(None);
                    };
                    self.validate_existing_data_descriptor(
                        key,
                        ArgumentsDataDescriptorState {
                            value: callee.value,
                            writable: callee.writable,
                            enumerable: callee.enumerable,
                            configurable: callee.configurable,
                        },
                        record,
                    )?;
                    if record.has_value {
                        callee.value = record.value;
                    }
                    if record.has_writable {
                        callee.writable = record.writable;
                    }
                    if record.has_enumerable {
                        callee.enumerable = record.enumerable;
                    }
                    if record.has_configurable {
                        callee.configurable = record.configurable;
                    }
                }
            }
            if let Some((get, set, enumerable, configurable)) = install_accessor {
                let obj = unsafe { &mut *obj_ptr.as_ptr() };
                let prop_key = self.intern_prop_key(key);
                obj.ensure_dyn_props().insert(
                    prop_key,
                    DynProp::accessor(get, set, enumerable, configurable),
                );
            }
            return Ok(Some(()));
        }

        let Some(index) = parse_js_array_index_name(key) else {
            return Ok(None);
        };
        let mut install_accessor = None;
        {
            let obj = unsafe { &mut *obj_ptr.as_ptr() };
            let Some(arguments) = obj.arguments.as_deref_mut() else {
                return Ok(None);
            };
            let Some(property) = arguments.indexed.get(index).copied() else {
                return Ok(None);
            };
            if property.deleted {
                return Ok(None);
            }

            let current_value = self
                .arguments_exotic_index_value(arguments, index)
                .unwrap_or(Value::undefined());
            if record.is_accessor() {
                self.validate_existing_data_descriptor(
                    key,
                    ArgumentsDataDescriptorState {
                        value: current_value,
                        writable: property.writable,
                        enumerable: property.enumerable,
                        configurable: property.configurable,
                    },
                    record,
                )?;
                self.arguments_exotic_disconnect_index_mapping(arguments, index, current_value);
                if let Some(index_state) = arguments.indexed.get_mut(index) {
                    index_state.deleted = true;
                }
                install_accessor = Some((
                    record.get,
                    record.set,
                    record.enumerable,
                    record.configurable,
                ));
            } else {
                self.validate_existing_data_descriptor(
                    key,
                    ArgumentsDataDescriptorState {
                        value: current_value,
                        writable: property.writable,
                        enumerable: property.enumerable,
                        configurable: property.configurable,
                    },
                    record,
                )?;

                let next_value = if record.has_value {
                    record.value
                } else {
                    current_value
                };
                let disconnect_mapping = record.has_writable && !record.writable;

                if let Some(index_state) = arguments.indexed.get_mut(index) {
                    if record.has_writable {
                        index_state.writable = record.writable;
                    }
                    if record.has_enumerable {
                        index_state.enumerable = record.enumerable;
                    }
                    if record.has_configurable {
                        index_state.configurable = record.configurable;
                    }
                }

                if let Some(value_slot) = arguments.values.get_mut(index) {
                    *value_slot = next_value;
                }
                if disconnect_mapping {
                    self.arguments_exotic_disconnect_index_mapping(arguments, index, next_value);
                } else if let Some(Some(refcell_value)) = arguments.mapped_refcells.get(index) {
                    if let Some(refcell_ptr) = unsafe { refcell_value.as_ptr::<RefCell>() } {
                        let refcell = unsafe { &mut *refcell_ptr.as_ptr() };
                        refcell.set(next_value);
                    }
                }
            }
        }

        if let Some((get, set, enumerable, configurable)) = install_accessor {
            let obj = unsafe { &mut *obj_ptr.as_ptr() };
            let prop_key = self.intern_prop_key(key);
            obj.ensure_dyn_props().insert(
                prop_key,
                DynProp::accessor(get, set, enumerable, configurable),
            );
        }

        Ok(Some(()))
    }

    fn array_exotic_current_own_property(
        &self,
        target: Value,
        key: &str,
    ) -> Option<OrdinaryOwnProperty> {
        if let Some(descriptor) = self.metadata_descriptor_property(target, key) {
            let mut property = self.ordinary_own_property_from_descriptor_value(descriptor)?;
            if let OrdinaryOwnProperty::Data { value, .. } = &mut property {
                if key == "length" {
                    if let Some(array_ptr) = checked_array_ptr(target) {
                        let array = unsafe { &*array_ptr.as_ptr() };
                        *value = if array.len() <= i32::MAX as usize {
                            Value::i32(array.len() as i32)
                        } else {
                            Value::f64(array.len() as f64)
                        };
                    }
                } else if let Some(index) = parse_js_array_index_name(key) {
                    if let Some(array_ptr) = checked_array_ptr(target) {
                        let array = unsafe { &*array_ptr.as_ptr() };
                        *value = array.get(index).unwrap_or(Value::undefined());
                    }
                } else if let Some(current) = self.metadata_data_property_value(target, key) {
                    *value = current;
                }
            }
            return Some(property);
        }

        let array_ptr = checked_array_ptr(target)?;
        let array = unsafe { &*array_ptr.as_ptr() };

        if key == "length" {
            return Some(OrdinaryOwnProperty::Data {
                value: if array.len() <= i32::MAX as usize {
                    Value::i32(array.len() as i32)
                } else {
                    Value::f64(array.len() as f64)
                },
                writable: self.is_field_writable(target, "length"),
                enumerable: false,
                configurable: false,
            });
        }

        if let Some(index) = parse_js_array_index_name(key) {
            let value = array.get(index)?;
            return Some(OrdinaryOwnProperty::Data {
                value,
                writable: true,
                enumerable: true,
                configurable: true,
            });
        }

        let value = self.metadata_data_property_value(target, key)?;
        let (writable, configurable, enumerable) =
            self.property_attributes_from_descriptor_metadata(target, key, (true, true, true));
        Some(OrdinaryOwnProperty::Data {
            value,
            writable,
            enumerable,
            configurable,
        })
    }

    fn array_exotic_define_own_property_with_context(
        &mut self,
        target: Value,
        key: &str,
        descriptor: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Option<()>, VmError> {
        let Some(array_ptr) = checked_array_ptr(target) else {
            return Ok(None);
        };

        if key == "length" {
            self.apply_array_length_descriptor_with_context(
                target,
                descriptor,
                caller_task,
                caller_module,
            )?;
            return Ok(Some(()));
        }

        let record = self.descriptor_record_from_descriptor(descriptor);
        let current = self.array_exotic_current_own_property(target, key);
        if let Some(current_property) = current {
            self.validate_existing_property_descriptor(key, current_property, record)?;
        } else if !self.is_js_value_extensible(target) {
            return Err(VmError::TypeError(format!(
                "Cannot define property '{}': object is not extensible",
                key
            )));
        }

        if let Some(index) = parse_js_array_index_name(key) {
            let array = unsafe { &mut *array_ptr.as_ptr() };
            if index >= array.length {
                array.resize_holey(index + 1);
            }
        }

        let next = self.apply_descriptor_record_to_ordinary_property(current, record);
        match next {
            OrdinaryOwnProperty::Accessor { .. } => {
                if let Some(index) = parse_js_array_index_name(key) {
                    let array = unsafe { &mut *array_ptr.as_ptr() };
                    let _ = array.delete_index(index);
                } else {
                    let mut metadata = self.metadata.lock();
                    let _ = metadata.delete_metadata_property(
                        NON_OBJECT_DYNAMIC_VALUE_METADATA_KEY,
                        target,
                        key,
                    );
                }
                let descriptor_value =
                    self.synthesize_descriptor_from_ordinary_own_property(next)?;
                self.set_descriptor_metadata(target, key, descriptor_value);
            }
            OrdinaryOwnProperty::Data { value, .. } => {
                if let Some(index) = parse_js_array_index_name(key) {
                    let array = unsafe { &mut *array_ptr.as_ptr() };
                    array.set(index, value).map_err(VmError::RuntimeError)?;
                } else {
                    self.metadata.lock().define_metadata_property(
                        NON_OBJECT_DYNAMIC_VALUE_METADATA_KEY.to_string(),
                        value,
                        target,
                        key.to_string(),
                    );
                }
                let descriptor_value =
                    self.synthesize_descriptor_from_ordinary_own_property(next)?;
                self.set_descriptor_metadata(target, key, descriptor_value);
            }
        }

        self.set_callable_virtual_property_deleted(target, key, false);
        self.set_fixed_property_deleted(target, key, false);
        Ok(Some(()))
    }

    fn synthesize_arguments_exotic_descriptor(
        &self,
        target: Value,
        key: &str,
    ) -> Result<Option<Value>, VmError> {
        let Some(obj_ptr) = checked_object_ptr(target) else {
            return Ok(None);
        };
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let Some(arguments) = obj.arguments.as_deref() else {
            return Ok(None);
        };

        let descriptor = self.alloc_object_descriptor()?;

        if key == "length" {
            let Some(length) = arguments.length.as_ref() else {
                return Ok(None);
            };
            self.set_internal_descriptor_field(descriptor, "value", length.value)?;
            self.set_internal_descriptor_field(
                descriptor,
                "writable",
                Value::bool(length.writable),
            )?;
            self.set_internal_descriptor_field(
                descriptor,
                "enumerable",
                Value::bool(length.enumerable),
            )?;
            self.set_internal_descriptor_field(
                descriptor,
                "configurable",
                Value::bool(length.configurable),
            )?;
            return Ok(Some(descriptor));
        }

        if key == "callee" {
            if arguments.strict_poison {
                self.set_internal_descriptor_field(descriptor, "get", Value::undefined())?;
                self.set_internal_descriptor_field(descriptor, "set", Value::undefined())?;
                self.set_internal_descriptor_field(descriptor, "enumerable", Value::bool(false))?;
                self.set_internal_descriptor_field(descriptor, "configurable", Value::bool(false))?;
                return Ok(Some(descriptor));
            }
            let Some(callee) = arguments.callee.as_ref() else {
                return Ok(None);
            };
            self.set_internal_descriptor_field(descriptor, "value", callee.value)?;
            self.set_internal_descriptor_field(
                descriptor,
                "writable",
                Value::bool(callee.writable),
            )?;
            self.set_internal_descriptor_field(
                descriptor,
                "enumerable",
                Value::bool(callee.enumerable),
            )?;
            self.set_internal_descriptor_field(
                descriptor,
                "configurable",
                Value::bool(callee.configurable),
            )?;
            return Ok(Some(descriptor));
        }

        if key == "caller" {
            return Ok(None);
        }

        let Some(index) = parse_js_array_index_name(key) else {
            return Ok(None);
        };
        let Some(property) = arguments.indexed.get(index) else {
            return Ok(None);
        };
        if property.deleted {
            return Ok(None);
        }
        let value = self
            .arguments_exotic_index_value(arguments, index)
            .unwrap_or(Value::undefined());
        self.set_internal_descriptor_field(descriptor, "value", value)?;
        self.set_internal_descriptor_field(descriptor, "writable", Value::bool(property.writable))?;
        self.set_internal_descriptor_field(
            descriptor,
            "enumerable",
            Value::bool(property.enumerable),
        )?;
        self.set_internal_descriptor_field(
            descriptor,
            "configurable",
            Value::bool(property.configurable),
        )?;
        Ok(Some(descriptor))
    }

    fn arguments_exotic_get(&self, target: Value, key: &str) -> Result<Option<Value>, VmError> {
        let Some(obj_ptr) = checked_object_ptr(target) else {
            return Ok(None);
        };
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let Some(arguments) = obj.arguments.as_deref() else {
            return Ok(None);
        };

        if key == "length" {
            return Ok(arguments.length.as_ref().map(|length| length.value));
        }
        if key == "callee" {
            if arguments.strict_poison {
                return Err(VmError::TypeError(format!(
                    "'{}' is not accessible on strict mode arguments objects",
                    key
                )));
            }
            return Ok(arguments.callee.as_ref().map(|callee| callee.value));
        }
        if key == "caller" {
            return Ok(None);
        }

        let Some(index) = parse_js_array_index_name(key) else {
            return Ok(None);
        };
        Ok(self.arguments_exotic_index_value(arguments, index))
    }

    fn arguments_exotic_set(
        &mut self,
        target: Value,
        key: &str,
        value: Value,
    ) -> Result<Option<bool>, VmError> {
        let Some(obj_ptr) = checked_object_ptr(target) else {
            return Ok(None);
        };
        let obj = unsafe { &mut *obj_ptr.as_ptr() };
        let Some(arguments) = obj.arguments.as_deref_mut() else {
            return Ok(None);
        };

        if key == "length" {
            let Some(length) = arguments.length.as_mut() else {
                return Ok(None);
            };
            if !length.writable {
                return Ok(Some(false));
            }
            length.value = value;
            return Ok(Some(true));
        }
        if key == "callee" {
            if arguments.strict_poison {
                return Err(VmError::TypeError(format!(
                    "'{}' is not writable on strict mode arguments objects",
                    key
                )));
            }
            let Some(callee) = arguments.callee.as_mut() else {
                return Ok(None);
            };
            if !callee.writable {
                return Ok(Some(false));
            }
            callee.value = value;
            return Ok(Some(true));
        }
        if key == "caller" {
            return Ok(None);
        }

        let Some(index) = parse_js_array_index_name(key) else {
            return Ok(None);
        };
        let Some(index_state) = arguments.indexed.get(index).copied() else {
            return Ok(None);
        };
        if index_state.deleted {
            return Ok(None);
        }
        if !index_state.writable {
            return Ok(Some(false));
        }

        if let Some(slot) = arguments.values.get_mut(index) {
            *slot = value;
        }
        if let Some(Some(refcell_value)) = arguments.mapped_refcells.get(index) {
            if let Some(refcell_ptr) = unsafe { refcell_value.as_ptr::<RefCell>() } {
                let refcell = unsafe { &mut *refcell_ptr.as_ptr() };
                refcell.set(value);
                return Ok(Some(true));
            }
        }
        Ok(Some(true))
    }

    fn arguments_exotic_delete(&mut self, target: Value, key: &str) -> Option<bool> {
        let obj_ptr = checked_object_ptr(target)?;
        let obj = unsafe { &mut *obj_ptr.as_ptr() };
        let arguments = obj.arguments.as_deref_mut()?;

        if key == "length" {
            let length = arguments.length.as_ref()?;
            if !length.configurable {
                return Some(false);
            }
            arguments.length = None;
            return Some(true);
        }
        if let Some(index) = parse_js_array_index_name(key) {
            let property = arguments.indexed.get(index)?;
            if property.deleted {
                return None;
            }
            if !property.configurable {
                return Some(false);
            }
            let current_value = self
                .arguments_exotic_index_value(arguments, index)
                .unwrap_or(Value::undefined());
            self.arguments_exotic_disconnect_index_mapping(arguments, index, current_value);
            if let Some(slot) = arguments.indexed.get_mut(index) {
                slot.deleted = true;
            }
            return Some(true);
        }
        if key == "callee" {
            if arguments.strict_poison {
                return Some(false);
            }
            let callee = arguments.callee.as_ref()?;
            if !callee.configurable {
                return Some(false);
            }
            arguments.callee = None;
            return Some(true);
        }
        if key == "caller" {
            return None;
        }
        None
    }

    fn arguments_exotic_property_flags(
        &self,
        target: Value,
        key: &str,
    ) -> Option<(bool, bool, bool)> {
        let obj_ptr = checked_object_ptr(target)?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let arguments = obj.arguments.as_deref()?;

        if key == "length" {
            let length = arguments.length.as_ref()?;
            return Some((length.writable, length.configurable, length.enumerable));
        }
        if key == "callee" {
            if arguments.strict_poison {
                return Some((false, false, false));
            }
            let callee = arguments.callee.as_ref()?;
            return Some((callee.writable, callee.configurable, callee.enumerable));
        }
        if key == "caller" {
            return None;
        }
        let index = parse_js_array_index_name(key)?;
        let property = arguments.indexed.get(index)?;
        if property.deleted {
            return None;
        }
        Some((
            property.writable,
            property.configurable,
            property.enumerable,
        ))
    }

    pub(in crate::vm::interpreter) fn builtin_global_value(&self, name: &str) -> Option<Value> {
        let slot = self.builtin_global_slots.read().get(name).copied()?;
        self.globals_by_index.read().get(slot).copied()
    }

    fn store_builtin_global_value(&self, name: &str, value: Value) {
        let mut slots = self.builtin_global_slots.write();
        let mut globals = self.globals_by_index.write();
        if let Some(&slot) = slots.get(name) {
            if slot >= globals.len() {
                globals.resize(slot + 1, Value::null());
            }
            globals[slot] = value;
            return;
        }
        let slot = globals.len();
        globals.push(value);
        slots.insert(name.to_string(), slot);
    }

    fn refresh_initialized_builtin_exports(&self, module: &Arc<Module>) {
        let Some(layout) = self.module_layouts.read().get(&module.checksum).cloned() else {
            return;
        };

        for export in &module.exports {
            if !matches!(export.symbol_type, crate::compiler::SymbolType::Constant) {
                continue;
            }
            let slot = layout.global_base
                + export
                    .runtime_global_slot
                    .map(|slot| slot as usize)
                    .unwrap_or(export.index);
            let Some(value) = self.globals_by_index.read().get(slot).copied() else {
                continue;
            };
            if std::env::var("RAYA_DEBUG_BUILTIN_INIT").is_ok() {
                eprintln!(
                    "[builtin-init] refresh module={} export={} slot={} value={:#x}",
                    module.metadata.name,
                    export.name,
                    slot,
                    value.raw()
                );
            }
            if value.is_null() || value.is_undefined() {
                continue;
            }
            if let Some(runtime_slot) = export.runtime_global_slot {
                let module_slot = layout.global_base + runtime_slot as usize;
                {
                    let mut globals = self.globals_by_index.write();
                    if module_slot >= globals.len() {
                        globals.resize(module_slot + 1, Value::null());
                    }
                    globals[module_slot] = value;
                }
                self.builtin_global_slots
                    .write()
                    .insert(export.name.clone(), module_slot);
            } else {
                self.store_builtin_global_value(&export.name, value);
            }
        }
    }

    fn lookup_builtin_export_module(&self, name: &str) -> Option<Arc<Module>> {
        let module = self
            .module_registry
            .read()
            .all_modules()
            .into_iter()
            .find(|module| {
                (module.metadata.name.starts_with("__raya_builtin__/")
                    || module.metadata.name.contains("builtins/")
                    || module
                        .metadata
                        .source_file
                        .as_deref()
                        .is_some_and(|path: &str| path.contains("builtins/")))
                    && module.exports.iter().any(|export| export.name == name)
            });
        if std::env::var("RAYA_DEBUG_BUILTIN_INIT").is_ok() {
            eprintln!(
                "[builtin-init] lookup name={} module={}",
                name,
                module
                    .as_ref()
                    .map(|module| module.metadata.name.as_str())
                    .unwrap_or("<none>")
            );
        }
        module
    }

    fn ensure_module_top_level_initialized(
        &mut self,
        module: Arc<Module>,
        caller_task: &Arc<Task>,
    ) -> Result<(), VmError> {
        if self
            .module_layouts
            .read()
            .get(&module.checksum)
            .is_some_and(|layout| layout.initialized)
        {
            return Ok(());
        }

        let Some(main_fn_id) = module
            .functions
            .iter()
            .rposition(|function| function.name == "main")
        else {
            if std::env::var("RAYA_DEBUG_BUILTIN_INIT").is_ok() {
                eprintln!("[builtin-init] module={} has no main", module.metadata.name);
            }
            if let Some(layout) = self.module_layouts.write().get_mut(&module.checksum) {
                layout.initialized = true;
            }
            return Ok(());
        };

        if std::env::var("RAYA_DEBUG_BUILTIN_INIT").is_ok() {
            eprintln!(
                "[builtin-init] running module={} main_fn_id={}",
                module.metadata.name, main_fn_id
            );
        }
        if let Some(layout) = self.module_layouts.write().get_mut(&module.checksum) {
            layout.initialized = true;
        }
        let init_task = Arc::new(Task::new(
            main_fn_id,
            module.clone(),
            Some(caller_task.id()),
        ));
        self.tasks.write().insert(init_task.id(), init_task.clone());
        match self.run(&init_task) {
            ExecutionResult::Completed(value) => {
                init_task.complete(value);
                self.refresh_initialized_builtin_exports(&module);
                if std::env::var("RAYA_DEBUG_BUILTIN_INIT").is_ok() {
                    eprintln!(
                        "[builtin-init] completed module={} result={:#x}",
                        module.metadata.name,
                        value.raw()
                    );
                }
                if let Some(layout) = self.module_layouts.write().get_mut(&module.checksum) {
                    layout.initialized = true;
                }
                Ok(())
            }
            ExecutionResult::Suspended(reason) => {
                if let Some(layout) = self.module_layouts.write().get_mut(&module.checksum) {
                    layout.initialized = false;
                }
                init_task.suspend(reason);
                Err(VmError::RuntimeError(
                    "Builtin module initialization suspended unexpectedly".to_string(),
                ))
            }
            ExecutionResult::Failed(error) => {
                if let Some(layout) = self.module_layouts.write().get_mut(&module.checksum) {
                    layout.initialized = false;
                }
                init_task.fail();
                if !caller_task.has_exception() {
                    if let Some(exception) = init_task.current_exception() {
                        caller_task.set_exception(exception);
                    }
                }
                Err(error)
            }
        }
    }

    fn ensure_builtin_global_value(
        &mut self,
        name: &str,
        caller_task: &Arc<Task>,
    ) -> Result<Option<Value>, VmError> {
        let current = self.builtin_global_value(name);
        if let Some(value) = current.filter(|value| !value.is_null() && !value.is_undefined()) {
            self.finalize_materialized_builtin_object(name, value)?;
            return Ok(current);
        }

        let Some(module) = self.lookup_builtin_export_module(name) else {
            return self.materialize_missing_builtin_constant(name, caller_task);
        };
        self.ensure_module_top_level_initialized(module, caller_task)?;
        let refreshed = self
            .builtin_global_value(name)
            .filter(|value| !value.is_null() && !value.is_undefined());
        if let Some(value) = refreshed {
            self.finalize_materialized_builtin_object(name, value)?;
            Ok(refreshed)
        } else {
            self.materialize_missing_builtin_constant(name, caller_task)
        }
    }

    fn materialize_missing_builtin_constant(
        &mut self,
        name: &str,
        caller_task: &Arc<Task>,
    ) -> Result<Option<Value>, VmError> {
        let value = match name {
            "globalThis" => {
                let value = self.alloc_plain_object()?;
                self.store_builtin_global_value("globalThis", value);
                value
            }
            "Math" => {
                let Some(nominal_type_id) = self
                    .classes
                    .read()
                    .get_class_by_name("__NodeCompatMath")
                    .map(|class| class.id)
                else {
                    return Ok(None);
                };
                let value = self.alloc_nominal_instance_value(nominal_type_id)?;
                self.store_builtin_global_value("Math", value);
                value
            }
            "Reflect" => {
                let Some(nominal_type_id) = self
                    .classes
                    .read()
                    .get_class_by_name("__NodeCompatReflect")
                    .map(|class| class.id)
                else {
                    return Ok(None);
                };
                let value = self.alloc_nominal_instance_value(nominal_type_id)?;
                self.store_builtin_global_value("Reflect", value);
                value
            }
            _ => return Ok(None),
        };

        self.finalize_materialized_builtin_object(name, value)?;

        if name != "globalThis" {
            if let Some(global_this) = self
                .builtin_global_value("globalThis")
                .filter(|global_this| !global_this.is_null() && !global_this.is_undefined())
            {
                let _ =
                    self.define_data_property_on_target(global_this, name, value, true, true, true);
            }
        } else {
            for sibling in ["Math", "Reflect"] {
                let _ = self.ensure_builtin_global_value(sibling, caller_task)?;
            }
        }

        Ok(Some(value))
    }

    fn finalize_materialized_builtin_object(
        &self,
        name: &str,
        value: Value,
    ) -> Result<(), VmError> {
        match name {
            "Math" => {
                for key in ["PI", "E"] {
                    if self.own_js_property_flags(value, key) == Some((false, false, false)) {
                        continue;
                    }
                    let constant = self
                        .get_own_field_value_by_name(value, key)
                        .or_else(|| self.get_own_js_property_value_by_name(value, key))
                        .or_else(|| match key {
                            "PI" => Some(Value::f64(std::f64::consts::PI)),
                            "E" => Some(Value::f64(std::f64::consts::E)),
                            _ => None,
                        })
                        .unwrap_or(Value::undefined());
                    self.define_data_property_on_target(value, key, constant, false, false, false)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn shared_js_global_binding(
        &self,
        name: &str,
    ) -> Option<crate::vm::interpreter::shared_state::JsGlobalBindingRecord> {
        self.js_global_bindings.read().get(name).copied()
    }

    fn shared_js_global_binding_value(&self, name: &str) -> Result<Option<Value>, VmError> {
        let Some(binding) = self.shared_js_global_binding(name) else {
            if std::env::var("RAYA_DEBUG_JS_GLOBAL_BINDINGS").is_ok() {
                eprintln!("[js-global:get] name={} hit=false", name);
            }
            return Ok(None);
        };
        if std::env::var("RAYA_DEBUG_JS_GLOBAL_BINDINGS").is_ok() {
            eprintln!(
                "[js-global:get] name={} hit=true slot={} initialized={} published={}",
                name, binding.slot, binding.initialized, binding.published_to_global_object
            );
        }
        if !binding.initialized {
            return Err(VmError::ReferenceError(format!("{name} is not defined")));
        }
        Ok(Some(
            self.globals_by_index
                .read()
                .get(binding.slot)
                .copied()
                .unwrap_or(Value::undefined()),
        ))
    }

    pub(in crate::vm::interpreter) fn ambient_global_value_sync(
        &self,
        name: &str,
    ) -> Option<Value> {
        self.builtin_global_value(name)
            .or_else(|| {
                let binding = self.shared_js_global_binding(name)?;
                if !binding.initialized {
                    return None;
                }
                self.globals_by_index.read().get(binding.slot).copied()
            })
            .or_else(|| {
                if name == "globalThis" {
                    return None;
                }
                let global_this = self.builtin_global_value("globalThis")?;
                self.get_own_js_property_value_by_name(global_this, name)
            })
    }

    fn intrinsic_class_prototype_value(&self, class_name: &str) -> Option<Value> {
        let lookup_name = {
            let classes = self.classes.read();
            if classes.get_class_by_name(class_name).is_some() {
                class_name.to_string()
            } else {
                boxed_primitive_helper_class_name(class_name)?.to_string()
            }
        };
        let ntid = self.classes.read().get_class_by_name(&lookup_name)?.id;
        if let Some(existing) = self
            .classes
            .read()
            .get_class(ntid)
            .and_then(|class| class.prototype_value)
        {
            return Some(existing);
        }
        self.create_prototype_for_class(ntid, &lookup_name, Value::null())
    }

    fn set_shared_js_global_binding_value(
        &mut self,
        name: &str,
        value: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<bool, VmError> {
        let Some(binding) = self.shared_js_global_binding(name) else {
            return Ok(false);
        };
        if binding.published_to_global_object {
            let Some(global_this) = self.ensure_builtin_global_value("globalThis", caller_task)?
            else {
                return Ok(false);
            };
            let written = self.set_property_value_via_js_semantics(
                global_this,
                name,
                value,
                global_this,
                caller_task,
                caller_module,
            )?;
            if !written {
                return Ok(false);
            }
        }
        {
            let mut globals = self.globals_by_index.write();
            if binding.slot >= globals.len() {
                globals.resize(binding.slot + 1, Value::undefined());
            }
            globals[binding.slot] = value;
        }
        if let Some(binding) = self.js_global_bindings.write().get_mut(name) {
            binding.initialized = true;
        }
        Ok(true)
    }

    /// Look up the nominal type ID for a builtin class by name (e.g., "TypeError", "Error").
    fn builtin_class_nominal_type_id(&self, class_name: &str) -> Option<usize> {
        let classes = self.classes.read();
        classes.get_class_by_name(class_name).map(|c| c.id)
    }

    pub(in crate::vm::interpreter) fn builtin_global_name_for_value(
        &self,
        value: Value,
    ) -> Option<String> {
        let globals = self.globals_by_index.read();
        self.builtin_global_slots
            .read()
            .iter()
            .find_map(|(name, &slot)| {
                globals
                    .get(slot)
                    .copied()
                    .filter(|candidate| candidate.raw() == value.raw())
                    .map(|_| name.clone())
            })
    }

    /// Unified prototype creation for any class with a nominal_type_id.
    ///
    /// This is the single path for creating class prototypes. It always:
    /// - Writes "constructor" to shape slot 0 (never dyn_props)
    /// - Tags the prototype with nominal_type_id for vtable method lookup
    /// - Stores in Class.prototype_value and callable virtual property cache
    /// - Uses builtin_global_value(class_name) for constructor identity when available
    fn create_prototype_for_class(
        &self,
        nominal_type_id: usize,
        class_name: &str,
        constructor_value: Value,
    ) -> Option<Value> {
        // Read class metadata before consulting caches so previously-created
        // generic prototype objects can still be hydrated with class-specific
        // members once the real nominal class is known.
        let (class_module, prototype_members, existing_class_prototype) = {
            let classes = self.classes.read();
            let class = classes.get_class(nominal_type_id)?;
            let class_module = class.module.clone();
            let prototype_members = class.prototype_members.clone();
            let existing = class.prototype_value;
            (class_module, prototype_members, existing)
        };

        // Check caches: Class.prototype_value, then callable virtual property cache.
        if let Some(proto_val) = existing_class_prototype {
            self.hydrate_class_prototype_members(
                proto_val,
                class_name,
                class_module.clone(),
                &prototype_members,
            )?;
            self.ensure_intrinsic_prototype_parent(class_name, proto_val);
            return Some(proto_val);
        }
        if let Some(existing) =
            self.cached_callable_virtual_property_value(constructor_value, "prototype")
        {
            // Fix up nominal_type_id if missing
            if let Some(proto_ptr) = checked_object_ptr(existing) {
                let proto_obj = unsafe { &mut *proto_ptr.as_ptr() };
                if proto_obj.header.nominal_type_id.is_none() {
                    proto_obj.header.nominal_type_id = Some(nominal_type_id as u32);
                }
            }
            // Store in class for future lookups
            {
                let mut classes = self.classes.write();
                if let Some(class) = classes.get_class_mut(nominal_type_id) {
                    class.prototype_value = Some(existing);
                }
            }
            self.hydrate_class_prototype_members(
                existing,
                class_name,
                class_module.clone(),
                &prototype_members,
            )?;
            self.ensure_intrinsic_prototype_parent(class_name, existing);
            return Some(existing);
        }

        // Allocate prototype object with layout ["constructor"]
        let member_names = vec!["constructor".to_string()];
        let prototype_val = if class_name == "Array" {
            let prototype_ptr = self.gc.lock().allocate(Array::new(0, 0));
            unsafe {
                Value::from_ptr(
                    std::ptr::NonNull::new(prototype_ptr.as_ptr()).expect("prototype array ptr"),
                )
            }
        } else {
            let layout_id = layout_id_from_ordered_names(&member_names);
            // Register the layout so structural_field_slot_index_for_object can
            // resolve "constructor" → slot 0 even when nominal_type_id is set.
            self.register_structural_layout_shape(layout_id, &member_names);
            let mut proto_obj = Object::new_dynamic(layout_id, member_names.len());
            proto_obj.header.nominal_type_id = Some(nominal_type_id as u32);
            let prototype_ptr = self.gc.lock().allocate(proto_obj);
            unsafe {
                Value::from_ptr(
                    std::ptr::NonNull::new(prototype_ptr.as_ptr()).expect("prototype object ptr"),
                )
            }
        };

        // Cache in callable virtual property store and Class.prototype_value
        self.set_cached_callable_virtual_property_value(
            constructor_value,
            "prototype",
            prototype_val,
        );
        {
            let mut classes = self.classes.write();
            if let Some(class) = classes.get_class_mut(nominal_type_id) {
                class.prototype_value = Some(prototype_val);
            }
        }

        // Write "constructor" to shape slot 0. Prefer canonical builtin global
        // value so prototype.constructor === the ambient global that user code sees.
        let canonical_ctor = self
            .builtin_global_value(class_name)
            .unwrap_or(constructor_value);
        if let Some(proto_ptr) = checked_object_ptr(prototype_val) {
            let proto_obj = unsafe { &mut *proto_ptr.as_ptr() };
            let _ = proto_obj.set_field(0, canonical_ctor);
            if let Some(meta) = proto_obj.slot_meta.get_mut(0) {
                meta.enumerable = false;
            }
        }

        // Array-specific: "length" property and Symbol.unscopables
        if class_name == "Array" {
            self.define_data_property_on_target(
                prototype_val,
                "length",
                Value::i32(0),
                true,
                false,
                false,
            )
            .ok()?;
            self.seed_array_unscopables_property(prototype_val)?;
        }

        // Set up prototype chain via intrinsic parent resolution
        self.ensure_intrinsic_prototype_parent(class_name, prototype_val);

        // Seed error-specific prototype properties (name, message defaults)
        self.seed_builtin_error_prototype_properties(prototype_val, class_name)?;

        self.hydrate_class_prototype_members(
            prototype_val,
            class_name,
            class_module,
            &prototype_members,
        )?;

        Some(prototype_val)
    }

    fn hydrate_class_prototype_members(
        &self,
        prototype_val: Value,
        class_name: &str,
        class_module: Option<Arc<Module>>,
        prototype_members: &[crate::vm::object::PrototypeMember],
    ) -> Option<()> {
        let has_materialized_own_property = |name: &str| {
            self.ordinary_own_property(prototype_val, name).is_some()
                || self
                    .metadata_descriptor_property(prototype_val, name)
                    .is_some()
                || self
                    .metadata_data_property_value(prototype_val, name)
                    .is_some()
        };

        // Populate own prototype members. Inherited members live on the parent
        // prototype chain; only declare accessors/methods owned by this class.
        let mut method_values = Vec::new();
        let mut accessor_pairs: rustc_hash::FxHashMap<String, (Option<Value>, Option<Value>)> =
            rustc_hash::FxHashMap::default();

        for member in prototype_members {
            if member.name.is_empty() || member.name.starts_with('#') {
                continue;
            }
            // Only skip if the prototype already has a materialized own property.
            // Nominal/vtable visibility is not enough here: builtins like
            // Boolean.prototype need real own properties so JS reflection and
            // boxed-wrapper coercion use the prototype methods instead of
            // inherited Object.prototype fallbacks.
            if has_materialized_own_property(&member.name) {
                continue;
            }

            let closure = if let Some(module) = class_module.clone() {
                Object::new_closure_with_module(member.function_id, Vec::new(), module)
            } else {
                Object::new_closure(member.function_id, Vec::new())
            };
            let closure_ptr = self.gc.lock().allocate(closure);
            let closure_val = unsafe {
                Value::from_ptr(
                    std::ptr::NonNull::new(closure_ptr.as_ptr()).expect("prototype method ptr"),
                )
            };
            match member.kind {
                crate::vm::object::PrototypeMemberKind::Method => {
                    method_values.push((member.name.clone(), closure_val));
                    if Self::should_skip_public_prototype_method_name(class_name, &member.name) {
                        continue;
                    }
                    self.define_data_property_on_target(
                        prototype_val,
                        &member.name,
                        closure_val,
                        true,
                        false,
                        true,
                    )
                    .ok()?;
                }
                crate::vm::object::PrototypeMemberKind::Getter => {
                    accessor_pairs.entry(member.name.clone()).or_default().0 = Some(closure_val);
                }
                crate::vm::object::PrototypeMemberKind::Setter => {
                    accessor_pairs.entry(member.name.clone()).or_default().1 = Some(closure_val);
                }
            }
        }

        for (name, (get, set)) in accessor_pairs {
            if has_materialized_own_property(&name) {
                continue;
            }
            self.define_accessor_property_on_target(
                prototype_val,
                &name,
                get.unwrap_or(Value::undefined()),
                set.unwrap_or(Value::undefined()),
                false,
                true,
            )
            .ok()?;
        }

        self.define_prototype_symbol_aliases(class_name, prototype_val, &method_values)?;
        Some(())
    }

    pub(in crate::vm::interpreter) fn nominal_instance_prototype_value(
        &self,
        value: Value,
    ) -> Option<Value> {
        let debug_proto_resolve = std::env::var("RAYA_DEBUG_PROTO_RESOLVE").is_ok();
        let object_ptr = checked_object_ptr(value)?;
        let object = unsafe { &*object_ptr.as_ptr() };
        let nominal_type_id = object.nominal_type_id_usize()?;
        let class_name = {
            let classes = self.classes.read();
            classes.get_class(nominal_type_id)?.name.clone()
        };
        let constructor_value = self.constructor_value_for_nominal_type(nominal_type_id)?;
        if debug_proto_resolve {
            eprintln!(
                "[proto-resolve] instance={:#x} nominal_type_id={} class='{}' ctor={:#x}",
                value.raw(),
                nominal_type_id,
                class_name,
                constructor_value.raw()
            );
        }
        self.create_prototype_for_class(nominal_type_id, &class_name, constructor_value)
    }

    fn ordinary_object_prototype_value(&self) -> Option<Value> {
        self.ambient_global_value_sync("Object")
            .and_then(|constructor_value| {
                self.object_constructor_prototype_value(constructor_value)
            })
            .or_else(|| self.intrinsic_class_prototype_value("Object"))
    }

    pub(in crate::vm::interpreter) fn prototype_of_value(&self, value: Value) -> Option<Value> {
        let debug_proto_resolve = std::env::var("RAYA_DEBUG_PROTO_RESOLVE").is_ok();
        if checked_array_ptr(value).is_some() {
            if let Some(prototype) = self.explicit_object_prototype(value) {
                if !prototype.is_null() {
                    if debug_proto_resolve {
                        eprintln!(
                            "[proto-of] value={:#x} array-explicit={:#x}",
                            value.raw(),
                            prototype.raw()
                        );
                    }
                    return Some(prototype);
                }
            }
            let prototype = self
                .ambient_global_value_sync("Array")
                .and_then(|ctor| self.array_constructor_prototype_value(ctor))
                .or_else(|| self.intrinsic_class_prototype_value("Array"));
            if debug_proto_resolve {
                eprintln!(
                    "[proto-of] value={:#x} array-fallback -> {:?}",
                    value.raw(),
                    prototype.map(|v| format!("{:#x}", v.raw()))
                );
            }
            return prototype;
        }

        // Property kernel: check Object.prototype field first
        if let Some(obj_ptr) = checked_object_ptr(value) {
            let obj = unsafe { &*obj_ptr.as_ptr() };
            if !obj.prototype.is_null() && obj.prototype != Value::undefined() {
                if debug_proto_resolve {
                    eprintln!(
                        "[proto-of] value={:#x} kernel-proto={:#x}",
                        value.raw(),
                        obj.prototype.raw()
                    );
                }
                return Some(obj.prototype);
            }
        }
        if let Some(prototype) = self.explicit_object_prototype(value) {
            if debug_proto_resolve {
                eprintln!(
                    "[proto-of] value={:#x} explicit={:#x}",
                    value.raw(),
                    prototype.raw()
                );
            }
            return Some(prototype);
        }

        if self.promise_handle_from_value(value).is_some() {
            let prototype = self
                .builtin_global_value("Promise")
                .and_then(|ctor| self.constructor_prototype_value(ctor));
            if debug_proto_resolve {
                eprintln!(
                    "[proto-of] value={:#x} task-promise -> {:?}",
                    value.raw(),
                    prototype.map(|v| format!("{:#x}", v.raw()))
                );
            }
            return prototype;
        }

        if let Some(nominal_type_id) = self.constructor_nominal_type_id(value) {
            let parent_id = {
                let classes = self.classes.read();
                classes
                    .get_class(nominal_type_id)
                    .and_then(|class| class.parent_id)
            };
            if let Some(parent_id) = parent_id {
                let prototype = self.constructor_value_for_nominal_type(parent_id);
                if debug_proto_resolve {
                    eprintln!(
                        "[proto-of] value={:#x} class-parent={} -> {:?}",
                        value.raw(),
                        parent_id,
                        prototype.map(|v| format!("{:#x}", v.raw()))
                    );
                }
                return prototype;
            }
        }

        if self.callable_function_info(value).is_some() {
            if let Some(parent_name) = self
                .builtin_global_name_for_value(value)
                .as_deref()
                .and_then(builtin_error_superclass_name)
            {
                let prototype = self.builtin_global_value(parent_name);
                if debug_proto_resolve {
                    eprintln!(
                        "[proto-of] value={:#x} builtin-error-super='{}' -> {:?}",
                        value.raw(),
                        parent_name,
                        prototype.map(|v| format!("{:#x}", v.raw()))
                    );
                }
                return prototype;
            }
            let prototype = self
                .callable_function_family_name(value)
                .and_then(|family| self.builtin_global_value(family))
                .and_then(|ctor| self.constructor_prototype_value(ctor))
                .or_else(|| {
                    self.builtin_global_value("Function")
                        .and_then(|ctor| self.constructor_prototype_value(ctor))
                });
            if debug_proto_resolve {
                eprintln!(
                    "[proto-of] value={:#x} callable -> {:?}",
                    value.raw(),
                    prototype.map(|v| format!("{:#x}", v.raw()))
                );
            }
            return prototype;
        }

        if checked_object_ptr(value).is_some() {
            if let Some(prototype) = self.nominal_instance_prototype_value(value) {
                if debug_proto_resolve {
                    eprintln!(
                        "[proto-of] value={:#x} nominal -> {:#x}",
                        value.raw(),
                        prototype.raw()
                    );
                }
                return Some(prototype);
            }
            if debug_proto_resolve {
                eprintln!("[proto-of] value={:#x} ordinary-object", value.raw());
            }
            return self.ordinary_object_prototype_value();
        }

        if checked_string_ptr(value).is_some() {
            return self
                .ambient_global_value_sync("String")
                .and_then(|ctor| self.string_constructor_prototype_value(ctor))
                .or_else(|| self.intrinsic_class_prototype_value("String"));
        }

        None
    }

    pub(in crate::vm::interpreter) fn constructor_prototype_value(
        &self,
        constructor: Value,
    ) -> Option<Value> {
        // 1. Fast path: Class.prototype_value (authoritative cache for nominal classes)
        if let Some(ntid) = self.constructor_nominal_type_id(constructor) {
            let classes = self.classes.read();
            if let Some(class) = classes.get_class(ntid) {
                if let Some(proto_val) = class.prototype_value {
                    return Some(proto_val);
                }
            }
            drop(classes);
            // Not yet created — create and cache via the unified path
            let class_name = self
                .classes
                .read()
                .get_class(ntid)
                .map(|c| c.name.clone())
                .unwrap_or_default();
            return self.create_prototype_for_class(ntid, &class_name, constructor);
        }

        // 2. Builtin/helper-class prototype resolution should win over any
        //    earlier generic callable-prototype cache entries so wrapper
        //    constructors (Boolean/Number/String) can hydrate real prototype
        //    members instead of reusing a bare generic object.
        if let Some(global_name) = self.builtin_global_name_for_value(constructor) {
            if let Some(proto) = self.create_prototype_for_class_by_name(&global_name, constructor)
            {
                return Some(proto);
            }
        }

        // 3. Callable virtual property cache (for non-nominal constructors,
        //    user-defined classes, or prototypes set via defineProperty)
        if let Some(existing) =
            self.cached_callable_virtual_property_value(constructor, "prototype")
        {
            self.ensure_prototype_nominal_type_id(constructor, existing);
            return Some(existing);
        }

        // 4. Plain callable closures should always get a generic function
        // prototype. Only nominal class constructors and actual builtin globals
        // should hydrate class-specific prototypes by name.
        self.generic_function_prototype_value(constructor)
    }

    fn constructed_object_prototype_from_constructor(&self, constructor: Value) -> Option<Value> {
        if let Some(prototype) = self.constructor_prototype_value(constructor) {
            if self.is_js_object_value(prototype) {
                return Some(prototype);
            }
        }

        self.builtin_global_value("Object")
            .and_then(|ctor| self.object_constructor_prototype_value(ctor))
    }

    pub(in crate::vm::interpreter) fn set_constructed_object_prototype_from_value(
        &self,
        object: Value,
        prototype: Value,
    ) {
        if !self.js_value_supports_extensibility(object) {
            return;
        }
        if !self.is_js_object_value(prototype) {
            return;
        }
        self.set_explicit_object_prototype(object, prototype);
    }

    pub(in crate::vm::interpreter) fn set_constructed_object_prototype_from_constructor(
        &self,
        object: Value,
        constructor: Value,
    ) {
        if let Some(prototype) = self.constructed_object_prototype_from_constructor(constructor) {
            if std::env::var("RAYA_DEBUG_PROTO_RESOLVE").is_ok() {
                eprintln!(
                    "[set-ctor-proto] object={:#x} ctor={:#x} proto={:#x}",
                    object.raw(),
                    constructor.raw(),
                    prototype.raw()
                );
            }
            self.set_constructed_object_prototype_from_value(object, prototype);
        }
    }

    pub(in crate::vm::interpreter) fn set_array_length_value(
        &self,
        target: Value,
        length_value: Value,
    ) -> Result<(), VmError> {
        let Some(array_ptr) = (unsafe { target.as_ptr::<Array>() }) else {
            return Err(VmError::TypeError(
                "Array length target must be an array".to_string(),
            ));
        };
        let new_len = self.js_array_length_from_property_value_without_context(length_value)?;
        let array = unsafe { &mut *array_ptr.as_ptr() };
        array.resize_holey(new_len);
        Ok(())
    }

    pub(in crate::vm::interpreter) fn set_array_length_value_with_context(
        &mut self,
        target: Value,
        length_value: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<(), VmError> {
        let Some(array_ptr) = (unsafe { target.as_ptr::<Array>() }) else {
            return Err(VmError::TypeError(
                "Array length target must be an array".to_string(),
            ));
        };
        let new_len = self.js_array_length_from_property_value_with_context(
            length_value,
            caller_task,
            caller_module,
        )?;
        let array = unsafe { &mut *array_ptr.as_ptr() };
        array.resize_holey(new_len);
        Ok(())
    }

    fn set_property_value_on_receiver(
        &mut self,
        receiver: Value,
        key: &str,
        value: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<bool, VmError> {
        if let Some(kind) = self.exotic_adapter_kind(receiver) {
            if let Some(updated) = self.exotic_set_property_on_receiver_with_context(
                kind,
                receiver,
                key,
                value,
                caller_task,
                caller_module,
            )? {
                return Ok(updated);
            }
        }

        if self.set_builtin_global_property(receiver, key, value) {
            self.sync_descriptor_value(receiver, key, value);
            return Ok(true);
        }

        if self.get_descriptor_metadata(receiver, key).is_some()
            && checked_object_ptr(receiver).is_none()
        {
            self.metadata.lock().define_metadata_property(
                NON_OBJECT_DYNAMIC_VALUE_METADATA_KEY.to_string(),
                value,
                receiver,
                key.to_string(),
            );
            self.sync_descriptor_value(receiver, key, value);
            self.set_callable_virtual_property_deleted(receiver, key, false);
            self.set_fixed_property_deleted(receiver, key, false);
            return Ok(true);
        }

        if self.callable_function_info(receiver).is_some()
            && self.get_descriptor_metadata(receiver, key).is_none()
        {
            if let Some((writable, configurable, enumerable)) =
                self.callable_virtual_property_descriptor(receiver, key)
            {
                if !writable {
                    return Ok(false);
                }
                // Write to Object.dyn_props if target is a callable
                if let Some(co_ptr) = checked_callable_ptr(receiver) {
                    let prop_key = self.intern_prop_key(key);
                    let co = unsafe { &mut *co_ptr.as_ptr() };
                    co.ensure_dyn_props().insert(
                        prop_key,
                        DynProp::data_with_attrs(value, writable, enumerable, configurable),
                    );
                }
                self.set_cached_callable_virtual_property_value(receiver, key, value);
                self.sync_descriptor_value(receiver, key, value);
                self.set_callable_virtual_property_deleted(receiver, key, false);
                self.set_fixed_property_deleted(receiver, key, false);
                return Ok(true);
            }
        }

        if let Some(obj_ptr) = checked_object_ptr(receiver) {
            let existing_own_shape = self.resolve_own_property_shape(receiver, key);
            let obj = unsafe { &mut *obj_ptr.as_ptr() };
            if let Some(index) = self.get_field_index_for_value(receiver, key) {
                let current = obj.get_field(index).unwrap_or(Value::undefined());
                let uses_runtime_placeholder = current.is_null()
                    && existing_own_shape.is_some_and(|shape| {
                        matches!(
                            shape.source,
                            JsOwnPropertySource::BuiltinNativeMethod
                                | JsOwnPropertySource::CallableVirtual
                                | JsOwnPropertySource::ConstructorStaticMethod
                                | JsOwnPropertySource::NominalMethod
                        )
                    });
                if uses_runtime_placeholder {
                    let prop_key = self.intern_prop_key(key);
                    let dyn_props = obj.ensure_dyn_props();
                    if let Some(existing) = dyn_props.get_mut(prop_key) {
                        existing.value = value;
                    } else if let Some(shape) = existing_own_shape {
                        dyn_props.insert(
                            prop_key,
                            DynProp::data_with_attrs(
                                value,
                                shape.writable,
                                shape.enumerable,
                                shape.configurable,
                            ),
                        );
                    } else {
                        dyn_props.insert(prop_key, DynProp::data(value));
                    }
                } else {
                    obj.set_field(index, value).map_err(VmError::RuntimeError)?;
                }
            } else {
                if !self.is_js_value_extensible(receiver) {
                    return Ok(false);
                }
                let prop_key = self.intern_prop_key(key);
                let dyn_props = obj.ensure_dyn_props();
                if let Some(existing) = dyn_props.get_mut(prop_key) {
                    existing.value = value;
                } else {
                    if let Some(shape) = existing_own_shape {
                        dyn_props.insert(
                            prop_key,
                            DynProp::data_with_attrs(
                                value,
                                shape.writable,
                                shape.enumerable,
                                shape.configurable,
                            ),
                        );
                    } else {
                        dyn_props.insert(prop_key, DynProp::data(value));
                    }
                }
            }
            self.sync_descriptor_value(receiver, key, value);
            self.set_callable_virtual_property_deleted(receiver, key, false);
            self.set_fixed_property_deleted(receiver, key, false);
            return Ok(true);
        }

        if receiver.is_ptr() || self.callable_function_info(receiver).is_some() {
            self.define_data_property_on_target(receiver, key, value, true, true, true)?;
            return Ok(true);
        }

        Ok(false)
    }

    pub(in crate::vm::interpreter) fn set_property_value_via_js_semantics(
        &mut self,
        target: Value,
        key: &str,
        value: Value,
        receiver: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<bool, VmError> {
        if let Some(trap_result) = self.try_proxy_set_property_with_invariants(
            target,
            key,
            value,
            caller_task,
            caller_module,
        )? {
            return Ok(trap_result);
        }
        if let Some(proxy) = crate::vm::reflect::try_unwrap_proxy(target) {
            return self.set_property_value_via_js_semantics(
                proxy.target,
                key,
                value,
                receiver,
                caller_task,
                caller_module,
            );
        }

        if let Some(resolved) = self.resolve_property_record_on_receiver_with_context(
            target,
            key,
            caller_task,
            caller_module,
        )? {
            match resolved.record.shape.kind {
                JsOwnPropertyKind::PoisonedAccessor => {
                    return Err(VmError::TypeError(format!(
                        "'{}' is not writable on strict mode arguments objects",
                        key
                    )));
                }
                JsOwnPropertyKind::Accessor => {
                    if let Some(setter) = resolved.record.setter {
                        let _ = self.invoke_callable_sync_with_this(
                            setter,
                            Some(receiver),
                            &[value],
                            caller_task,
                            caller_module,
                        )?;
                        return Ok(true);
                    }
                    return Ok(false);
                }
                JsOwnPropertyKind::Data => {
                    if !resolved.record.shape.writable {
                        return Ok(false);
                    }
                    return self.set_property_value_on_receiver(
                        receiver,
                        key,
                        value,
                        caller_task,
                        caller_module,
                    );
                }
            }
        }

        self.set_property_value_on_receiver(receiver, key, value, caller_task, caller_module)
    }

    pub(in crate::vm::interpreter) fn try_proxy_like_get_property(
        &mut self,
        value: Value,
        key: &str,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Option<Value>, VmError> {
        // Ordinary property access should only invoke proxy semantics for an
        // actual proxy exotic object. Wrapper classes like the JS-visible
        // `Proxy` helper must behave as normal objects so their own/prototype
        // methods stay reachable.
        if let Some(result) =
            self.try_proxy_get_property_with_invariants(value, key, caller_task, caller_module)?
        {
            return Ok(Some(result));
        }

        let Some(proxy) = crate::vm::reflect::try_unwrap_proxy(value) else {
            return Ok(None);
        };
        if let Some(resolved) = self.resolve_property_record_on_receiver_with_context(
            proxy.target,
            key,
            caller_task,
            caller_module,
        )? {
            let value = self.read_resolved_property_value_with_context(
                key,
                resolved.record,
                value,
                caller_task,
                caller_module,
            )?;
            if key == "prototype" {
                self.ensure_prototype_nominal_type_id(resolved.owner, value);
            }
            return Ok(Some(value));
        }

        Ok(Some(Value::null()))
    }

    fn ensure_intrinsic_prototype_parent(&self, class_name: &str, prototype_val: Value) {
        if self.explicit_object_prototype(prototype_val).is_some() {
            return;
        }

        if class_name == "Object" {
            self.set_explicit_object_prototype(prototype_val, Value::null());
            return;
        }

        if let Some(parent_name) = builtin_error_superclass_name(class_name) {
            if let Some(parent_ctor) = self.builtin_global_value(parent_name) {
                if let Some(parent_proto) = self.constructor_prototype_value(parent_ctor) {
                    self.set_constructed_object_prototype_from_value(prototype_val, parent_proto);
                }
            }
            return;
        }

        if let Some(object_ctor) = self.builtin_global_value("Object") {
            if let Some(object_proto) = self.object_constructor_prototype_value(object_ctor) {
                self.set_constructed_object_prototype_from_value(prototype_val, object_proto);
            }
        }
    }

    fn generic_function_prototype_value(&self, class_value: Value) -> Option<Value> {
        let debug_dynamic_function = std::env::var("RAYA_DEBUG_DYNAMIC_FUNCTION").is_ok();
        if let Some(existing) =
            self.cached_callable_virtual_property_value(class_value, "prototype")
        {
            if debug_dynamic_function {
                eprintln!(
                    "[generic-fn-proto] target={:#x} cached={:#x}",
                    class_value.raw(),
                    existing.raw()
                );
            }
            return Some(existing);
        }
        if !self.callable_exposes_default_prototype(class_value) {
            if debug_dynamic_function {
                eprintln!(
                    "[generic-fn-proto] target={:#x} no-default-prototype",
                    class_value.raw()
                );
            }
            return None;
        }
        if debug_dynamic_function {
            eprintln!(
                "[generic-fn-proto] target={:#x} alloc:start",
                class_value.raw()
            );
        }

        let layout_id = layout_id_from_ordered_names(&["constructor".to_string()]);
        let prototype_ptr = self.gc.lock().allocate(Object::new_dynamic(layout_id, 1));
        let prototype_val = unsafe {
            Value::from_ptr(
                std::ptr::NonNull::new(prototype_ptr.as_ptr()).expect("prototype object ptr"),
            )
        };
        if debug_dynamic_function {
            eprintln!(
                "[generic-fn-proto] target={:#x} alloc:prototype={:#x}",
                class_value.raw(),
                prototype_val.raw()
            );
        }
        self.set_cached_callable_virtual_property_value(class_value, "prototype", prototype_val);
        if debug_dynamic_function {
            eprintln!(
                "[generic-fn-proto] target={:#x} cache:set",
                class_value.raw()
            );
        }

        self.define_data_property_on_target(
            prototype_val,
            "constructor",
            class_value,
            true,
            false,
            true,
        )
        .ok()?;
        if debug_dynamic_function {
            eprintln!(
                "[generic-fn-proto] target={:#x} constructor:set",
                class_value.raw()
            );
        }

        let prototype_parent = self
            .callable_function_family_name(class_value)
            .and_then(|family| match family {
                "GeneratorFunction" => self.default_generator_instance_prototype(false),
                "AsyncGeneratorFunction" => self.default_generator_instance_prototype(true),
                _ => self
                    .builtin_global_value("Object")
                    .and_then(|ctor| self.object_constructor_prototype_value(ctor)),
            });
        if let Some(prototype_parent) = prototype_parent {
            self.set_constructed_object_prototype_from_value(prototype_val, prototype_parent);
            if debug_dynamic_function {
                eprintln!(
                    "[generic-fn-proto] target={:#x} proto-parent:set {:#x}",
                    class_value.raw(),
                    prototype_parent.raw()
                );
            }
        }

        if let Some(class_obj_ptr) = checked_object_ptr(class_value) {
            let class_obj = unsafe { &mut *class_obj_ptr.as_ptr() };
            class_obj.ensure_dyn_props().insert(
                self.intern_prop_key("prototype"),
                DynProp::data_with_attrs(prototype_val, true, false, false),
            );
        }
        if debug_dynamic_function {
            eprintln!("[generic-fn-proto] target={:#x} done", class_value.raw());
        }
        Some(prototype_val)
    }

    pub(in crate::vm::interpreter) fn boxed_primitive_internal_value(
        &self,
        value: Value,
        kind: &str,
    ) -> Option<Value> {
        let kind_value = self.get_own_field_value_by_name(value, "__boxedPrimitiveKind")?;
        let actual_kind = primitive_to_js_string(kind_value)?;
        if actual_kind != kind {
            return None;
        }
        self.get_own_field_value_by_name(value, "__primitiveValue")
    }

    fn alloc_boxed_primitive_object(
        &mut self,
        constructor: Value,
        kind: &str,
        primitive_value: Value,
    ) -> Result<Value, VmError> {
        let member_names = vec![
            "__boxedPrimitiveKind".to_string(),
            "__primitiveValue".to_string(),
        ];
        let layout_id = layout_id_from_ordered_names(&member_names);
        let object_ptr = self
            .gc
            .lock()
            .allocate(Object::new_dynamic(layout_id, member_names.len()));
        let object_value = unsafe {
            Value::from_ptr(
                std::ptr::NonNull::new(object_ptr.as_ptr()).expect("boxed primitive object ptr"),
            )
        };
        self.set_constructed_object_prototype_from_constructor(object_value, constructor);
        let kind_ptr = self.gc.lock().allocate(RayaString::new(kind.to_string()));
        let kind_value = unsafe {
            Value::from_ptr(
                std::ptr::NonNull::new(kind_ptr.as_ptr()).expect("boxed primitive kind ptr"),
            )
        };
        self.define_data_property_on_target(
            object_value,
            "__boxedPrimitiveKind",
            kind_value,
            true,
            false,
            false,
        )?;
        self.define_data_property_on_target(
            object_value,
            "__primitiveValue",
            primitive_value,
            true,
            false,
            false,
        )?;
        Ok(object_value)
    }

    pub(in crate::vm::interpreter) fn try_construct_boxed_primitive(
        &mut self,
        constructor: Value,
        args: &[Value],
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Option<Value>, VmError> {
        let Some(global_name) = self.builtin_global_name_for_value(constructor) else {
            return Ok(None);
        };
        if !matches!(
            global_name.as_str(),
            "BigInt" | "Boolean" | "Number" | "String" | "Symbol"
        ) {
            return Ok(None);
        }
        let primitive_value = self.invoke_callable_sync(constructor, args, task, module)?;
        self.alloc_boxed_primitive_object(constructor, &global_name, primitive_value)
            .map(Some)
    }

    fn js_array_length_from_number(&self, numeric: f64) -> Result<usize, VmError> {
        if !numeric.is_finite()
            || numeric < 0.0
            || numeric > u32::MAX as f64
            || numeric.fract() != 0.0
        {
            return Err(VmError::RangeError("Invalid array length".to_string()));
        }

        Ok(numeric as usize)
    }

    fn js_array_constructor_length_from_value(
        &self,
        value: Value,
    ) -> Result<Option<usize>, VmError> {
        let Some(numeric) = value.as_i32().map(|v| v as f64).or_else(|| value.as_f64()) else {
            return Ok(None);
        };
        self.js_array_length_from_number(numeric).map(Some)
    }

    fn is_js_primitive_value(&self, value: Value) -> bool {
        value.is_undefined()
            || value.is_null()
            || value.as_bool().is_some()
            || value.as_i32().is_some()
            || value.as_f64().is_some()
            || checked_bigint_ptr(value).is_some()
            || checked_string_ptr(value).is_some()
            || self.is_symbol_value(value)
    }

    pub(in crate::vm::interpreter) fn js_to_primitive_with_hint(
        &mut self,
        value: Value,
        hint: &str,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Value, VmError> {
        if self.is_js_primitive_value(value) {
            return Ok(value);
        }

        for kind in ["Boolean", "Number", "String", "BigInt", "Symbol"] {
            if let Some(primitive) = self.boxed_primitive_internal_value(value, kind) {
                return Ok(primitive);
            }
        }

        match self.well_known_symbol_property_value(
            value,
            "Symbol.toPrimitive",
            caller_task,
            caller_module,
        )? {
            Some(exotic) if exotic.is_null() || exotic.is_undefined() => {
                // Per ToPrimitive, null/undefined Symbol.toPrimitive is ignored
                // and ordinary valueOf/toString fallback still applies.
            }
            Some(exotic) if !Self::is_callable_value(exotic) => {
                return Err(VmError::TypeError(
                    "Cannot convert object to primitive value".to_string(),
                ));
            }
            Some(exotic) => {
                let hint_ptr = self.gc.lock().allocate(RayaString::new(hint.to_string()));
                let hint_value = unsafe {
                    Value::from_ptr(std::ptr::NonNull::new(hint_ptr.as_ptr()).expect("hint ptr"))
                };
                self.ephemeral_gc_roots.write().push(hint_value);
                let result = self.invoke_callable_sync_with_this(
                    exotic,
                    Some(value),
                    &[hint_value],
                    caller_task,
                    caller_module,
                );
                let mut ephemeral = self.ephemeral_gc_roots.write();
                if let Some(index) = ephemeral
                    .iter()
                    .rposition(|candidate| *candidate == hint_value)
                {
                    ephemeral.swap_remove(index);
                }
                let primitive = result?;
                if self.is_js_primitive_value(primitive) {
                    return Ok(primitive);
                }
                return Err(VmError::TypeError(
                    "Cannot convert object to primitive value".to_string(),
                ));
            }
            None => {}
        }

        let method_order = if hint == "string" {
            ["toString", "valueOf"]
        } else {
            ["valueOf", "toString"]
        };
        for method_name in method_order {
            let Some(method) = self.get_field_value_by_name(value, method_name) else {
                continue;
            };
            if !Self::is_callable_value(method) {
                continue;
            }
            let primitive = self.invoke_callable_sync_with_this(
                method,
                Some(value),
                &[],
                caller_task,
                caller_module,
            )?;
            if self.is_js_primitive_value(primitive) {
                return Ok(primitive);
            }
        }

        Err(VmError::TypeError(
            "Cannot convert object to primitive value".to_string(),
        ))
    }

    pub(in crate::vm::interpreter) fn js_to_primitive_number_hint(
        &mut self,
        value: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Value, VmError> {
        self.js_to_primitive_with_hint(value, "number", caller_task, caller_module)
    }

    pub(in crate::vm::interpreter) fn js_to_number_from_primitive(
        &self,
        value: Value,
    ) -> Result<f64, VmError> {
        if checked_bigint_ptr(value).is_some() {
            return Err(VmError::TypeError(
                "Cannot convert a BigInt value to a number".to_string(),
            ));
        }
        if self.is_symbol_value(value) {
            return Err(VmError::TypeError(
                "Cannot convert a Symbol value to a number".to_string(),
            ));
        }
        if value.is_undefined() {
            return Ok(f64::NAN);
        }
        if value.is_null() {
            return Ok(0.0);
        }
        if let Some(value) = value.as_bool() {
            return Ok(if value { 1.0 } else { 0.0 });
        }
        if let Some(value) = value.as_i32() {
            return Ok(value as f64);
        }
        if let Some(value) = value.as_f64() {
            return Ok(value);
        }
        if let Some(ptr) = checked_string_ptr(value) {
            let text = unsafe { &*ptr.as_ptr() }.data.trim().to_string();
            if text.is_empty() {
                return Ok(0.0);
            }
            if text == "Infinity" || text == "+Infinity" {
                return Ok(f64::INFINITY);
            }
            if text == "-Infinity" {
                return Ok(f64::NEG_INFINITY);
            }
            if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
                return Ok(u64::from_str_radix(hex, 16)
                    .map(|value| value as f64)
                    .unwrap_or(f64::NAN));
            }
            if let Some(bin) = text.strip_prefix("0b").or_else(|| text.strip_prefix("0B")) {
                return Ok(u64::from_str_radix(bin, 2)
                    .map(|value| value as f64)
                    .unwrap_or(f64::NAN));
            }
            if let Some(oct) = text.strip_prefix("0o").or_else(|| text.strip_prefix("0O")) {
                return Ok(u64::from_str_radix(oct, 8)
                    .map(|value| value as f64)
                    .unwrap_or(f64::NAN));
            }
            return Ok(text.parse::<f64>().unwrap_or(f64::NAN));
        }
        Err(VmError::TypeError(
            "Cannot convert value to number".to_string(),
        ))
    }

    pub(in crate::vm::interpreter) fn js_to_number_with_context(
        &mut self,
        value: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<f64, VmError> {
        let primitive = self.js_to_primitive_number_hint(value, caller_task, caller_module)?;
        self.js_to_number_from_primitive(primitive)
    }

    pub(in crate::vm::interpreter) fn js_to_integer_or_infinity_with_context(
        &mut self,
        value: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<f64, VmError> {
        let number = self.js_to_number_with_context(value, caller_task, caller_module)?;
        if number.is_nan() {
            return Ok(0.0);
        }
        if !number.is_finite() || number == 0.0 {
            return Ok(if number == 0.0 { 0.0 } else { number });
        }
        Ok(if number.is_sign_negative() {
            number.ceil()
        } else {
            number.floor()
        })
    }

    fn js_unary_minus_with_context(
        &mut self,
        value: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Value, VmError> {
        let primitive = self.js_to_primitive_number_hint(value, caller_task, caller_module)?;
        if let Some(bigint_ptr) = checked_bigint_ptr(primitive) {
            return Ok(self.alloc_bigint_value(-unsafe { &*bigint_ptr.as_ptr() }.data.clone()));
        }
        Ok(Value::f64(-self.js_to_number_from_primitive(primitive)?))
    }

    fn js_add_with_context(
        &mut self,
        left: Value,
        right: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Value, VmError> {
        let left_primitive =
            self.js_to_primitive_with_hint(left, "default", caller_task, caller_module)?;
        let right_primitive =
            self.js_to_primitive_with_hint(right, "default", caller_task, caller_module)?;

        if checked_string_ptr(left_primitive).is_some()
            || checked_string_ptr(right_primitive).is_some()
        {
            let left_text =
                self.js_function_argument_to_string(left_primitive, caller_task, caller_module)?;
            let right_text =
                self.js_function_argument_to_string(right_primitive, caller_task, caller_module)?;
            return Ok(self.alloc_string_value(format!("{left_text}{right_text}")));
        }

        enum JsNumeric {
            BigInt(ArbitraryBigInt),
            Number(f64),
        }

        let to_numeric = |this: &mut Self, value: Value| -> Result<JsNumeric, VmError> {
            if let Some(bigint_ptr) = checked_bigint_ptr(value) {
                return Ok(JsNumeric::BigInt(
                    unsafe { &*bigint_ptr.as_ptr() }.data.clone(),
                ));
            }
            Ok(JsNumeric::Number(this.js_to_number_from_primitive(value)?))
        };

        match (
            to_numeric(self, left_primitive)?,
            to_numeric(self, right_primitive)?,
        ) {
            (JsNumeric::BigInt(left_bigint), JsNumeric::BigInt(right_bigint)) => {
                Ok(self.alloc_bigint_value(left_bigint + right_bigint))
            }
            (JsNumeric::Number(left_number), JsNumeric::Number(right_number)) => {
                Ok(Value::f64(left_number + right_number))
            }
            _ => Err(VmError::TypeError(
                "Cannot mix BigInt and Number in addition".to_string(),
            )),
        }
    }

    fn alloc_bigint_value(&mut self, data: ArbitraryBigInt) -> Value {
        let ptr = self.gc.lock().allocate(RayaBigInt::new(data));
        unsafe { Value::from_ptr(std::ptr::NonNull::new(ptr.as_ptr()).expect("bigint ptr")) }
    }

    fn parse_js_bigint_literal_value(&self, source: &str) -> Result<ArbitraryBigInt, VmError> {
        let trimmed = source.trim();
        let literal = trimmed
            .strip_suffix('n')
            .unwrap_or(trimmed)
            .replace('_', "");
        let (radix, digits) = if let Some(hex) = literal
            .strip_prefix("0x")
            .or_else(|| literal.strip_prefix("0X"))
        {
            (16, hex)
        } else if let Some(binary) = literal
            .strip_prefix("0b")
            .or_else(|| literal.strip_prefix("0B"))
        {
            (2, binary)
        } else if let Some(octal) = literal
            .strip_prefix("0o")
            .or_else(|| literal.strip_prefix("0O"))
        {
            (8, octal)
        } else {
            (10, literal.as_str())
        };
        ArbitraryBigInt::parse_bytes(digits.as_bytes(), radix)
            .ok_or_else(|| VmError::SyntaxError(format!("Invalid BigInt literal: {source}")))
    }

    fn js_math_number_arg(
        &mut self,
        args: &[Value],
        index: usize,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<f64, VmError> {
        self.js_to_number_with_context(native_arg(args, index), caller_task, caller_module)
    }

    fn js_usize_arg_with_context(
        &mut self,
        value: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<usize, VmError> {
        let number =
            self.js_to_integer_or_infinity_with_context(value, caller_task, caller_module)?;
        if number.is_nan() || number <= 0.0 {
            return Ok(0);
        }
        if !number.is_finite() || number >= usize::MAX as f64 {
            return Ok(usize::MAX);
        }
        Ok(number as usize)
    }

    fn js_i32_arg_with_context(
        &mut self,
        value: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<i32, VmError> {
        let number =
            self.js_to_integer_or_infinity_with_context(value, caller_task, caller_module)?;
        if number.is_nan() {
            return Ok(0);
        }
        if number <= i32::MIN as f64 {
            return Ok(i32::MIN);
        }
        if number >= i32::MAX as f64 {
            return Ok(i32::MAX);
        }
        Ok(number as i32)
    }

    fn js_math_min_max(
        &mut self,
        args: &[Value],
        want_min: bool,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<f64, VmError> {
        if args.is_empty() {
            return Ok(if want_min {
                f64::INFINITY
            } else {
                f64::NEG_INFINITY
            });
        }

        let mut result = self.js_math_number_arg(args, 0, caller_task, caller_module)?;
        if result.is_nan() {
            return Ok(f64::NAN);
        }

        for index in 1..args.len() {
            let value = self.js_math_number_arg(args, index, caller_task, caller_module)?;
            if value.is_nan() {
                return Ok(f64::NAN);
            }
            if want_min {
                if value < result
                    || (value == 0.0
                        && result == 0.0
                        && value.is_sign_negative()
                        && !result.is_sign_negative())
                {
                    result = value;
                }
            } else if value > result
                || (value == 0.0
                    && result == 0.0
                    && !value.is_sign_negative()
                    && result.is_sign_negative())
            {
                result = value;
            }
        }

        Ok(result)
    }

    fn js_math_round(number: f64) -> f64 {
        if !number.is_finite() || number == 0.0 {
            return number;
        }
        if number < 0.0 && number >= -0.5 {
            return -0.0;
        }
        (number + 0.5).floor()
    }

    fn js_to_uint32(number: f64) -> u32 {
        if !number.is_finite() || number == 0.0 {
            return 0;
        }
        let integer = number.signum() * number.abs().floor();
        integer.rem_euclid(4_294_967_296.0) as u32
    }

    fn js_array_length_from_property_value_without_context(
        &self,
        value: Value,
    ) -> Result<usize, VmError> {
        let primitive = if self.is_js_primitive_value(value) {
            value
        } else {
            let mut boxed_primitive = None;
            for kind in ["Boolean", "Number", "String"] {
                if let Some(primitive) = self.boxed_primitive_internal_value(value, kind) {
                    boxed_primitive = Some(primitive);
                    break;
                }
            }
            match boxed_primitive {
                Some(primitive) => primitive,
                None => {
                    return Err(VmError::TypeError(
                        "Cannot convert object to primitive value".to_string(),
                    ))
                }
            }
        };
        let numeric = self.js_to_number_from_primitive(primitive)?;
        self.js_array_length_from_number(numeric)
    }

    fn js_array_length_from_property_value_with_context(
        &mut self,
        value: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<usize, VmError> {
        let primitive = self.js_to_primitive_number_hint(value, caller_task, caller_module)?;
        let numeric = self.js_to_number_from_primitive(primitive)?;
        self.js_array_length_from_number(numeric)
    }

    fn js_array_set_length_from_property_value_with_context(
        &mut self,
        value: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<usize, VmError> {
        if std::env::var("RAYA_DEBUG_REFLECT_SET").is_ok() {
            eprintln!("[array-set-length] coercing value={:#x}", value.raw());
        }
        let new_len = Self::js_to_uint32(self.js_to_number_with_context(
            value,
            caller_task,
            caller_module,
        )?);
        let number_len = self.js_to_number_with_context(value, caller_task, caller_module)?;
        if std::env::var("RAYA_DEBUG_REFLECT_SET").is_ok() {
            eprintln!(
                "[array-set-length] numeric-coercions uint32={} number={}",
                new_len, number_len
            );
        }
        if new_len as f64 != number_len {
            return Err(VmError::RangeError("Invalid array length".to_string()));
        }
        Ok(new_len as usize)
    }

    fn array_length_value(len: usize) -> Value {
        if len <= i32::MAX as usize {
            Value::i32(len as i32)
        } else {
            Value::f64(len as f64)
        }
    }

    fn store_array_length_descriptor(
        &self,
        target: Value,
        len: usize,
        writable: bool,
    ) -> Result<(), VmError> {
        let descriptor = self.alloc_object_descriptor()?;
        let Some(descriptor_ptr) = (unsafe { descriptor.as_ptr::<Object>() }) else {
            return Err(VmError::RuntimeError(
                "Failed to allocate array length descriptor".to_string(),
            ));
        };
        let descriptor_obj = unsafe { &mut *descriptor_ptr.as_ptr() };
        for (field_name, field_value) in [
            ("value", Self::array_length_value(len)),
            ("writable", Value::bool(writable)),
            ("enumerable", Value::bool(false)),
            ("configurable", Value::bool(false)),
        ] {
            if let Some(field_index) = self.get_field_index_for_value(descriptor, field_name) {
                descriptor_obj
                    .set_field(field_index, field_value)
                    .map_err(VmError::RuntimeError)?;
            }
            self.set_descriptor_field_present(descriptor, field_name, true);
        }
        self.set_descriptor_metadata(target, "length", descriptor);
        self.set_callable_virtual_property_deleted(target, "length", false);
        self.set_fixed_property_deleted(target, "length", false);
        Ok(())
    }

    fn set_array_length_via_array_set_length(
        &mut self,
        target: Value,
        value: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<bool, VmError> {
        if std::env::var("RAYA_DEBUG_REFLECT_SET").is_ok() {
            eprintln!(
                "[array-set-length] start target={:#x} value={:#x} writable={}",
                target.raw(),
                value.raw(),
                self.is_field_writable(target, "length")
            );
        }
        let new_len = self.js_array_set_length_from_property_value_with_context(
            value,
            caller_task,
            caller_module,
        )?;
        if std::env::var("RAYA_DEBUG_REFLECT_SET").is_ok() {
            eprintln!("[array-set-length] coerced new_len={}", new_len);
        }
        if !self.is_field_writable(target, "length") {
            if std::env::var("RAYA_DEBUG_REFLECT_SET").is_ok() {
                eprintln!("[array-set-length] target became non-writable");
            }
            return Ok(false);
        }
        let Some(array_ptr) = checked_array_ptr(target) else {
            return Err(VmError::TypeError(
                "Array length target must be an array".to_string(),
            ));
        };
        let array = unsafe { &mut *array_ptr.as_ptr() };
        array.resize_holey(new_len);
        self.store_array_length_descriptor(target, new_len, true)?;
        Ok(true)
    }

    fn apply_array_length_descriptor_with_context(
        &mut self,
        target: Value,
        descriptor: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<(), VmError> {
        let Some(array_ptr) = checked_array_ptr(target) else {
            return Err(VmError::TypeError(
                "Array length target must be an array".to_string(),
            ));
        };

        let requested_len = if self.descriptor_field_present(descriptor, "value") {
            let value = self
                .get_field_value_by_name(descriptor, "value")
                .unwrap_or(Value::undefined());
            Some(self.js_array_set_length_from_property_value_with_context(
                value,
                caller_task,
                caller_module,
            )?)
        } else {
            None
        };

        if self.descriptor_field_present(descriptor, "get")
            || self.descriptor_field_present(descriptor, "set")
        {
            return Err(VmError::TypeError(
                "Cannot redefine non-configurable property 'length'".to_string(),
            ));
        }
        if self.descriptor_field_present(descriptor, "configurable")
            && self.descriptor_flag(descriptor, "configurable", false)
        {
            return Err(VmError::TypeError(
                "Cannot redefine non-configurable property 'length'".to_string(),
            ));
        }
        if self.descriptor_field_present(descriptor, "enumerable")
            && self.descriptor_flag(descriptor, "enumerable", false)
        {
            return Err(VmError::TypeError(
                "Cannot redefine non-configurable property 'length'".to_string(),
            ));
        }

        let old_len = unsafe { &*array_ptr.as_ptr() }.len();
        let current_writable = self.is_field_writable(target, "length");
        let requested_writable = self
            .descriptor_field_present(descriptor, "writable")
            .then(|| self.descriptor_flag(descriptor, "writable", false));
        if !current_writable && requested_writable == Some(true) {
            return Err(VmError::TypeError(
                "Cannot redefine non-configurable property 'length'".to_string(),
            ));
        }

        let mut final_len = old_len;
        if let Some(new_len) = requested_len {
            if new_len != old_len && !current_writable {
                return Err(VmError::TypeError(
                    "Cannot assign to non-writable property 'length'".to_string(),
                ));
            }
            if new_len != old_len {
                let array = unsafe { &mut *array_ptr.as_ptr() };
                array.resize_holey(new_len);
            }
            final_len = new_len;
        }

        self.store_array_length_descriptor(
            target,
            final_len,
            requested_writable.unwrap_or(current_writable),
        )
    }

    pub(in crate::vm::interpreter) fn object_constructor_prototype_value(
        &self,
        class_value: Value,
    ) -> Option<Value> {
        self.create_prototype_for_class_by_name("Object", class_value)
    }

    pub(in crate::vm::interpreter) fn array_constructor_prototype_value(
        &self,
        class_value: Value,
    ) -> Option<Value> {
        self.create_prototype_for_class_by_name("Array", class_value)
    }

    pub(in crate::vm::interpreter) fn string_constructor_prototype_value(
        &self,
        class_value: Value,
    ) -> Option<Value> {
        self.create_prototype_for_class_by_name("String", class_value)
    }

    pub(in crate::vm::interpreter) fn function_constructor_prototype_value(
        &self,
        class_value: Value,
    ) -> Option<Value> {
        self.create_prototype_for_class_by_name("Function", class_value)
    }

    /// Helper: resolve nominal_type_id from class name and delegate to create_prototype_for_class.
    fn create_prototype_for_class_by_name(
        &self,
        class_name: &str,
        constructor_value: Value,
    ) -> Option<Value> {
        let lookup_name = {
            let classes = self.classes.read();
            if classes.get_class_by_name(class_name).is_some() {
                class_name.to_string()
            } else {
                boxed_primitive_helper_class_name(class_name)?.to_string()
            }
        };
        let ntid = self.classes.read().get_class_by_name(&lookup_name)?.id;
        self.create_prototype_for_class(ntid, &lookup_name, constructor_value)
    }

    fn species_accessor_getter_for_constructor(&self, class_value: Value) -> Option<Value> {
        let builtin = self.builtin_global_value("Array")?;
        if builtin.raw() != class_value.raw() {
            return None;
        }
        self.get_own_field_value_by_name(class_value, "__speciesGetter")
    }

    fn number_constructor_intrinsic_value(&self, target: Value, key: &str) -> Option<Value> {
        let number_ctor = self.builtin_global_value("Number")?;
        if target.raw() != number_ctor.raw() {
            return None;
        }
        intrinsic_number_constructor_constant(key)
    }

    pub(in crate::vm::interpreter) fn callable_virtual_accessor_value(
        &self,
        target: Value,
        key: &str,
        accessor_name: &str,
    ) -> Option<Value> {
        if self.callable_virtual_property_deleted(target, key) {
            return None;
        }
        match (key, accessor_name) {
            ("Symbol.species", "get") => self.species_accessor_getter_for_constructor(target),
            _ => None,
        }
    }

    pub(in crate::vm::interpreter) fn callable_virtual_property_value(
        &self,
        target: Value,
        key: &str,
    ) -> Option<Value> {
        if self.callable_virtual_property_deleted(target, key) {
            return None;
        }
        if key == "prototype" {
            if let Some(value) = self.cached_callable_virtual_property_value(target, key) {
                return Some(value);
            }
            let proto = self.constructor_prototype_value(target)?;
            // Ensure prototype has nominal_type_id for vtable method lookup
            if let Some(proto_ptr) = checked_object_ptr(proto) {
                let proto_obj = unsafe { &mut *proto_ptr.as_ptr() };
                if proto_obj.header.nominal_type_id.is_none() {
                    if let Some(ntid) = self.constructor_nominal_type_id(target) {
                        proto_obj.header.nominal_type_id = Some(ntid as u32);
                    }
                }
            }
            return Some(proto);
        }
        if let Some(value) = self.number_constructor_intrinsic_value(target, key) {
            return Some(value);
        }
        if let Some(value) = self.metadata_data_property_value(target, key) {
            return Some(value);
        }
        if let Some(value) = self.cached_callable_virtual_property_value(target, key) {
            return Some(value);
        }
        if matches!(key, "caller" | "arguments")
            && self.callable_has_legacy_caller_arguments_own_props(target)
        {
            return Some(Value::undefined());
        }
        match key {
            "name" | "length" => self.callable_property_value(target, key),
            _ => None,
        }
    }

    /// Ensure a prototype object has nominal_type_id set from its constructor.
    /// This is needed so DynGetKeyed's vtable method lookup works on prototype objects.
    fn ensure_prototype_nominal_type_id(&self, constructor: Value, prototype: Value) {
        if let Some(proto_ptr) = checked_object_ptr(prototype) {
            let proto_obj = unsafe { &mut *proto_ptr.as_ptr() };
            if proto_obj.header.nominal_type_id.is_none() {
                if let Some(ntid) = self.constructor_nominal_type_id(constructor) {
                    proto_obj.header.nominal_type_id = Some(ntid as u32);
                    if std::env::var("RAYA_DEBUG_PROTO_FIXUP").is_ok() {
                        eprintln!(
                            "[proto-fixup] set nominal_type_id={} on proto={:#x} from ctor={:#x}",
                            ntid,
                            prototype.raw(),
                            constructor.raw()
                        );
                    }
                }
            }
        }
    }

    pub(in crate::vm::interpreter) fn callable_virtual_property_descriptor(
        &self,
        target: Value,
        key: &str,
    ) -> Option<(bool, bool, bool)> {
        if self.callable_virtual_property_deleted(target, key) {
            return None;
        }
        // Check callable Object.dyn_props first (property kernel path)
        if let Some(co_ptr) = checked_callable_ptr(target) {
            let co = unsafe { &*co_ptr.as_ptr() };
            if let Some(ref dp) = co.dyn_props {
                let key_id = self.intern_prop_key(key);
                if let Some(prop) = dp.get(key_id) {
                    return Some((prop.writable, prop.enumerable, prop.configurable));
                }
            }
        }
        if matches!(key, "caller" | "arguments")
            && self.callable_has_legacy_caller_arguments_own_props(target)
        {
            return Some((false, false, false));
        }
        match key {
            key if self
                .number_constructor_intrinsic_value(target, key)
                .is_some() =>
            {
                Some((false, false, false))
            }
            "prototype" if self.constructor_prototype_value(target).is_some() => {
                let writable = self.builtin_global_name_for_value(target).is_none()
                    && self.nominal_class_name_for_value(target).is_none();
                Some((writable, false, false))
            }
            "name" | "length" if self.callable_property_value(target, key).is_some() => {
                Some((false, true, false))
            }
            "Symbol.species"
                if self
                    .species_accessor_getter_for_constructor(target)
                    .is_some() =>
            {
                Some((false, true, false))
            }
            _ => None,
        }
    }

    pub(in crate::vm::interpreter) fn callable_property_value(
        &self,
        target: Value,
        key: &str,
    ) -> Option<Value> {
        if self.callable_virtual_property_deleted(target, key) {
            return None;
        }
        // Check callable Object.dyn_props first (property kernel path)
        if let Some(co_ptr) = checked_callable_ptr(target) {
            let co = unsafe { &*co_ptr.as_ptr() };
            if let Some(ref dp) = co.dyn_props {
                let key_id = self.intern_prop_key(key);
                if let Some(prop) = dp.get(key_id) {
                    return Some(prop.value);
                }
            }
            if let Some(ref cd) = co.callable {
                if let CallableKind::Bound {
                    visible_name,
                    visible_length,
                    ..
                } = &cd.kind
                {
                    return match key {
                        "name" => {
                            let ptr = self
                                .gc
                                .lock()
                                .allocate(RayaString::new(visible_name.clone()));
                            Some(unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(ptr.as_ptr()).unwrap())
                            })
                        }
                        "length" => Some(*visible_length),
                        _ => None,
                    };
                }
            }
        }
        let (mut name, length) = self.callable_function_info(target)?;
        if self.callable_is_arrow_function(target) && name.starts_with("__arrow_") {
            name.clear();
        }
        if std::env::var("RAYA_DEBUG_DYNAMIC_FUNCTION").is_ok() {
            eprintln!(
                "[callable-prop] target={:#x} key={} name={} length={}",
                target.raw(),
                key,
                name,
                length
            );
        }
        match key {
            "name" => {
                let ptr = self.gc.lock().allocate(RayaString::new(name));
                if std::env::var("RAYA_DEBUG_DYNAMIC_FUNCTION").is_ok() {
                    eprintln!("[callable-prop] name:allocated");
                }
                Some(unsafe { Value::from_ptr(std::ptr::NonNull::new(ptr.as_ptr()).unwrap()) })
            }
            "length" => Some(Value::i32(length as i32)),
            _ => None,
        }
    }

    fn nominal_class_name_for_value(&self, value: Value) -> Option<String> {
        let obj_ptr = checked_object_ptr(value)?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let nominal_type_id = obj.nominal_type_id_usize()?;
        let classes = self.classes.read();
        classes
            .get_class(nominal_type_id)
            .map(|class| class.name.clone())
    }

    pub(in crate::vm::interpreter) fn js_function_argument_to_string(
        &mut self,
        value: Value,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<String, VmError> {
        if let Some(text) = primitive_to_js_string(value) {
            return Ok(text);
        }

        if self
            .nominal_class_name_for_value(value)
            .as_deref()
            .is_some_and(|name| name == "Symbol")
        {
            return Err(VmError::TypeError(
                "Cannot convert a Symbol value to a string".to_string(),
            ));
        }

        let primitive = self.js_to_primitive_with_hint(value, "string", task, module)?;
        primitive_to_js_string(primitive).ok_or_else(|| {
            VmError::TypeError("Cannot convert object to primitive value".to_string())
        })
    }

    fn dynamic_js_ambient_builtin_globals(&self) -> FxHashSet<String> {
        self.builtin_global_slots.read().keys().cloned().collect()
    }

    fn collect_direct_eval_binding_names(
        &self,
        module: &crate::parser::ast::Module,
        interner: &crate::parser::Interner,
    ) -> FxHashSet<String> {
        let mut collector = DirectEvalIdentifierCollector {
            interner,
            names: FxHashSet::default(),
        };
        walk_module(&mut collector, module);
        collector.names
    }

    fn direct_eval_declares_arguments(
        &self,
        module: &crate::parser::ast::Module,
        interner: &crate::parser::Interner,
    ) -> bool {
        let mut collector = DirectEvalDeclarationCollector::new(interner);
        walk_module(&mut collector, module);
        collector.declares_arguments
    }

    fn raw_direct_eval_declarations(&self, source: &str) -> Option<DirectEvalDeclarations> {
        let Ok(parser) = Parser::new_with_mode(source, TypeSystemMode::Js) else {
            return None;
        };
        let Ok((ast, interner)) = parser.parse() else {
            return None;
        };
        let mut collector = DirectEvalDeclarationCollector::new(&interner);
        walk_module(&mut collector, &ast);
        let mut declarations: DirectEvalDeclarations = collector.into();
        declarations.source_is_strict = ast
            .statements
            .iter()
            .take_while(|stmt| {
                matches!(
                    stmt,
                    Statement::Expression(crate::parser::ast::ExpressionStatement {
                        expression: Expression::StringLiteral(_),
                        ..
                    })
                )
            })
            .any(|stmt| {
                matches!(
                    stmt,
                    Statement::Expression(crate::parser::ast::ExpressionStatement {
                        expression: Expression::StringLiteral(lit),
                        ..
                    }) if interner.resolve(lit.value) == "use strict"
                )
            });
        Some(declarations)
    }

    fn raw_direct_eval_declares_arguments(&self, source: &str) -> bool {
        self.raw_direct_eval_declarations(source)
            .is_some_and(|collector| collector.declares_arguments)
    }

    fn compile_dynamic_js_module_source(
        &self,
        source: &str,
        module_identity_prefix: &str,
        error_context: &str,
        options: DynamicJsCompileOptions,
    ) -> Result<Arc<Module>, VmError> {
        let dynamic_compile_syntax_error = |stage: &str, error: String| {
            VmError::SyntaxError(format!("{error_context} {stage}: {error}"))
        };
        let debug_dynamic_function = std::env::var("RAYA_DEBUG_DYNAMIC_FUNCTION").is_ok();
        if debug_dynamic_function {
            eprintln!("[dynamic-fn] compile:start source={:?}", source);
        }
        let parser = Parser::new_with_mode(&source, TypeSystemMode::Js)
            .map_err(|error| dynamic_compile_syntax_error("lexer error", format!("{error:?}")))?;
        if debug_dynamic_function {
            eprintln!("[dynamic-fn] compile:parsed-lexer");
        }
        let (ast, interner) = parser
            .parse()
            .map_err(|error| dynamic_compile_syntax_error("parse error", format!("{error:?}")))?;
        let mut early_error_options = EarlyErrorOptions::for_mode(TypeSystemMode::Js);
        early_error_options.allow_top_level_return = false;
        early_error_options.allow_new_target = options.allow_new_target;
        early_error_options.allow_super_property = options.allow_super_property;
        check_early_errors_with_options(&ast, &interner, early_error_options)
            .map_err(|error| dynamic_compile_syntax_error("parse error", format!("{error:?}")))?;
        if debug_dynamic_function {
            eprintln!("[dynamic-fn] compile:parsed-ast");
        }

        let mut direct_eval_binding_names =
            if let Some(_entry_name) = options.direct_eval_entry_function.as_ref() {
                if options.direct_eval_binding_names.is_empty() {
                    self.collect_direct_eval_binding_names(&ast, &interner)
                } else {
                    options.direct_eval_binding_names
                }
            } else {
                options.direct_eval_binding_names
            };
        let mut declaration_collector = DirectEvalDeclarationCollector::new(&interner);
        walk_module(&mut declaration_collector, &ast);
        let direct_eval_declarations: DirectEvalDeclarations = declaration_collector.into();
        for name in direct_eval_declarations
            .var_names
            .iter()
            .chain(direct_eval_declarations.function_names.iter())
            .chain(direct_eval_declarations.lexical_names.iter())
        {
            direct_eval_binding_names.insert(name.clone());
        }
        if direct_eval_declarations.declares_arguments {
            direct_eval_binding_names.insert("arguments".to_string());
        }
        if let Some(entry_name) = options.direct_eval_entry_function.as_ref() {
            direct_eval_binding_names.remove(entry_name);
        }

        let mut type_ctx = TypeContext::new();
        let policy = CheckerPolicy::for_mode(TypeSystemMode::Js);
        let mut binder = Binder::new(&mut type_ctx, &interner)
            .with_mode(TypeSystemMode::Js)
            .with_policy(policy);
        let builtin_sigs = crate::builtins::to_checker_signatures();
        binder.register_builtins(&builtin_sigs);
        if debug_dynamic_function {
            eprintln!("[dynamic-fn] compile:builtin-sigs");
        }

        let mut symbols = binder
            .bind_module(&ast)
            .map_err(|error| dynamic_compile_syntax_error("bind error", format!("{error:?}")))?;
        if debug_dynamic_function {
            eprintln!("[dynamic-fn] compile:bound");
        }

        let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner)
            .with_mode(TypeSystemMode::Js)
            .with_policy(policy);
        let check_result = checker
            .check_module(&ast)
            .map_err(|error| dynamic_compile_syntax_error("type error", format!("{error:?}")))?;
        if debug_dynamic_function {
            eprintln!("[dynamic-fn] compile:checked");
        }

        for ((scope_id, name), ty) in check_result.inferred_types {
            symbols.update_type(ScopeId(scope_id), &name, ty);
        }

        let mut ambient_builtin_globals = self.dynamic_js_ambient_builtin_globals();
        ambient_builtin_globals.extend(direct_eval_binding_names.iter().cloned());
        let module_identity = format!(
            "{}/{}",
            module_identity_prefix,
            DYNAMIC_JS_FUNCTION_COUNTER.fetch_add(1, Ordering::Relaxed)
        );

        let mut compiler = Compiler::new(type_ctx, &interner)
            .with_expr_types(check_result.expr_types)
            .with_type_annotation_types(check_result.type_annotation_types)
            .with_module_identity(module_identity)
            .with_js_this_binding_compat(true)
            .with_allow_unresolved_runtime_fallback(true)
            .with_ambient_builtin_globals(ambient_builtin_globals)
            .with_track_top_level_completion(options.track_top_level_completion)
            .with_emit_script_global_bindings(options.emit_script_global_bindings)
            .with_script_global_bindings_configurable(options.script_global_bindings_configurable)
            .with_source_text(source.to_string());
        if let Some(entry) = options.direct_eval_entry_function {
            compiler = compiler
                .with_direct_eval_entry_function(entry)
                .with_direct_eval_binding_names(direct_eval_binding_names);
        }
        let module = compiler.compile_via_ir(&ast).map_err(|error| {
            VmError::RuntimeError(format!("{} compile error: {}", error_context, error))
        })?;
        if debug_dynamic_function {
            eprintln!("[dynamic-fn] compile:done");
        }
        Ok(Arc::new(module))
    }

    fn preflight_direct_eval_source(
        &self,
        source: &str,
        options: &DynamicJsCompileOptions,
    ) -> Result<(), VmError> {
        let parser = Parser::new_with_mode(source, TypeSystemMode::Js).map_err(|error| {
            VmError::SyntaxError(format!("Dynamic eval lexer error: {error:?}"))
        })?;
        let (ast, interner) = parser.parse().map_err(|error| {
            VmError::SyntaxError(format!("Dynamic eval parse error: {error:?}"))
        })?;
        let mut early_error_options = EarlyErrorOptions::for_mode(TypeSystemMode::Js);
        early_error_options.allow_top_level_return = false;
        early_error_options.allow_new_target = options.allow_new_target;
        early_error_options.allow_super_property = options.allow_super_property;
        check_early_errors_with_options(&ast, &interner, early_error_options).map_err(|error| {
            VmError::SyntaxError(format!("Dynamic eval parse error: {error:?}"))
        })?;
        Ok(())
    }

    fn compile_dynamic_js_function_module(
        &self,
        params_source: &str,
        body_source: &str,
    ) -> Result<Arc<Module>, VmError> {
        let source = format!("function __dynamic_fn__({params_source}) {{\n{body_source}\n}}\n");
        self.compile_dynamic_js_module_source(
            &source,
            "__dynamic_function__",
            "Dynamic Function",
            DynamicJsCompileOptions::default(),
        )
    }

    fn alloc_dynamic_js_closure(
        &mut self,
        function_module: Arc<Module>,
        function_name: &str,
        registration_context: &str,
        missing_symbol_context: &str,
    ) -> Result<Value, VmError> {
        self.register_dynamic_module(function_module.clone())
            .map_err(|message| {
                VmError::RuntimeError(format!("{registration_context}: {message}"))
            })?;
        let func_id = function_module
            .functions
            .iter()
            .position(|function| function.name == function_name)
            .ok_or_else(|| VmError::RuntimeError(missing_symbol_context.to_string()))?;
        let closure = Object::new_closure_with_module(func_id, Vec::new(), function_module);
        let closure_ptr = self.gc.lock().allocate(closure);
        Ok(unsafe {
            Value::from_ptr(
                std::ptr::NonNull::new(closure_ptr.as_ptr()).expect("dynamic function ptr"),
            )
        })
    }

    fn execute_dynamic_js_module_main(
        &mut self,
        dynamic_module: Arc<Module>,
        caller_task: &Arc<Task>,
    ) -> Result<Value, VmError> {
        self.register_dynamic_module(dynamic_module.clone())
            .map_err(|message| {
                VmError::RuntimeError(format!("Dynamic eval module registration error: {message}"))
            })?;
        let main_fn_id = dynamic_module
            .functions
            .iter()
            .rposition(|function| function.name == "main")
            .ok_or_else(|| {
                VmError::RuntimeError(
                    "Dynamic eval compile did not produce main function".to_string(),
                )
            })?;
        let eval_task = Arc::new(Task::new(
            main_fn_id,
            dynamic_module,
            Some(caller_task.id()),
        ));
        self.tasks.write().insert(eval_task.id(), eval_task.clone());
        match self.run(&eval_task) {
            ExecutionResult::Completed(value) => {
                eval_task.complete(value);
                Ok(value)
            }
            ExecutionResult::Suspended(reason) => {
                eval_task.suspend(reason);
                Err(VmError::RuntimeError(
                    "Synchronous dynamic eval suspended unexpectedly".to_string(),
                ))
            }
            ExecutionResult::Failed(error) => {
                eval_task.fail();
                if !caller_task.has_exception() {
                    if let Some(exception) = eval_task.current_exception() {
                        caller_task.set_exception(exception);
                    }
                }
                Err(error)
            }
        }
    }

    fn alloc_dynamic_js_function(
        &mut self,
        args: &[Value],
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Value, VmError> {
        let debug_dynamic_function = std::env::var("RAYA_DEBUG_DYNAMIC_FUNCTION").is_ok();
        if debug_dynamic_function {
            eprintln!("[dynamic-fn] alloc:start argc={}", args.len());
        }
        let mut parts = Vec::with_capacity(args.len());
        for arg in args {
            if debug_dynamic_function {
                eprintln!(
                    "[dynamic-fn] alloc:arg-to-string:start value={:#x}",
                    arg.raw()
                );
            }
            parts.push(self.js_function_argument_to_string(*arg, task, module)?);
            if debug_dynamic_function {
                eprintln!("[dynamic-fn] alloc:arg-to-string:done");
            }
        }
        let body_source = parts.pop().unwrap_or_default();
        let params_source = parts.join(",");
        if debug_dynamic_function {
            eprintln!(
                "[dynamic-fn] alloc:sources params={:?} body={:?}",
                params_source, body_source
            );
        }
        let function_module =
            self.compile_dynamic_js_function_module(&params_source, &body_source)?;
        if debug_dynamic_function {
            eprintln!("[dynamic-fn] alloc:compiled-module");
        }
        let closure_val = self.alloc_dynamic_js_closure(
            function_module,
            "__dynamic_fn__",
            "Dynamic Function module registration error",
            "Dynamic Function compile did not produce __dynamic_fn__",
        )?;
        if debug_dynamic_function {
            eprintln!("[dynamic-fn] alloc:registered-module");
        }
        if debug_dynamic_function {
            eprintln!("[dynamic-fn] alloc:done");
        }
        Ok(closure_val)
    }

    fn current_direct_eval_this_value(
        &self,
        stack: &Stack,
        task: &Arc<Task>,
        module: &Module,
    ) -> Value {
        module
            .functions
            .get(task.current_func_id())
            .filter(|function| function.uses_js_this_slot)
            .and_then(|_| stack.peek_at(task.current_locals_base()).ok())
            .or_else(|| self.builtin_global_value("globalThis"))
            .unwrap_or(Value::undefined())
    }

    fn current_function_is_strict_js(&self, task: &Arc<Task>, module: &Module) -> bool {
        module
            .functions
            .get(task.current_func_id())
            .is_some_and(|function| function.is_strict_js)
    }

    fn current_function_allows_new_target(&self, task: &Arc<Task>, module: &Module) -> bool {
        module
            .functions
            .get(task.current_func_id())
            .is_some_and(|function| function.is_constructible)
    }

    fn current_js_home_object(&self, task: &Arc<Task>) -> Option<Value> {
        task.current_active_js_home_object().or_else(|| {
            task.current_closure().and_then(|closure| {
                let closure_ptr = unsafe { closure.as_ptr::<Object>() }?;
                let closure_obj = unsafe { &*closure_ptr.as_ptr() };
                closure_obj.callable_home_object()
            })
        })
    }

    fn current_js_new_target(&self, task: &Arc<Task>) -> Option<Value> {
        task.current_active_js_new_target().or_else(|| {
            task.current_closure().and_then(|closure| {
                let closure_ptr = unsafe { closure.as_ptr::<Object>() }?;
                let closure_obj = unsafe { &*closure_ptr.as_ptr() };
                closure_obj.callable_new_target()
            })
        })
    }

    fn strip_leading_hashbang_comment<'s>(&self, source: &'s str) -> &'s str {
        if !source.starts_with("#!") {
            return source;
        }
        if let Some(line_end) = source.find('\n') {
            &source[line_end + 1..]
        } else {
            ""
        }
    }

    fn js_source_is_only_comments_and_whitespace(&self, source: &str) -> bool {
        fn is_js_whitespace(ch: char) -> bool {
            matches!(
                ch,
                '\u{0009}'
                    | '\u{000B}'
                    | '\u{000C}'
                    | '\u{0020}'
                    | '\u{00A0}'
                    | '\u{1680}'
                    | '\u{2000}'
                    ..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'
            )
        }

        fn line_terminator_width(slice: &str) -> Option<usize> {
            if slice.starts_with("\r\n") {
                Some(2)
            } else if slice.starts_with('\n')
                || slice.starts_with('\r')
                || slice.starts_with('\u{2028}')
                || slice.starts_with('\u{2029}')
            {
                Some(slice.chars().next().map(char::len_utf8).unwrap_or(0))
            } else {
                None
            }
        }

        let mut pos = 0usize;
        while pos < source.len() {
            let rest = &source[pos..];
            if let Some(width) = line_terminator_width(rest) {
                pos += width;
                continue;
            }
            let Some(ch) = rest.chars().next() else {
                break;
            };
            if is_js_whitespace(ch) {
                pos += ch.len_utf8();
                continue;
            }
            if rest.starts_with("//") {
                pos += 2;
                while pos < source.len() {
                    let line_rest = &source[pos..];
                    if line_terminator_width(line_rest).is_some() {
                        break;
                    }
                    let Some(next) = line_rest.chars().next() else {
                        break;
                    };
                    pos += next.len_utf8();
                }
                continue;
            }
            if rest.starts_with("/*") {
                pos += 2;
                let mut closed = false;
                while pos < source.len() {
                    let block_rest = &source[pos..];
                    if block_rest.starts_with("*/") {
                        pos += 2;
                        closed = true;
                        break;
                    }
                    let Some(next) = block_rest.chars().next() else {
                        break;
                    };
                    pos += next.len_utf8();
                }
                if !closed {
                    return false;
                }
                continue;
            }
            return false;
        }
        true
    }

    fn eval_dynamic_js_source(
        &mut self,
        source: &str,
        options: DynamicJsCompileOptions,
        direct_env: Option<Value>,
        stack: &Stack,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<Value, VmError> {
        let source = self.strip_leading_hashbang_comment(source);
        if self.js_source_is_only_comments_and_whitespace(source) {
            return Ok(Value::undefined());
        }
        let in_parameter_initializer = options.in_parameter_initializer;
        let caller_has_parameter_named_arguments = options.has_parameter_named_arguments;
        let inherited_allow_new_target =
            direct_env.is_some() && self.current_function_allows_new_target(task, module);
        let inherited_allow_super_property =
            direct_env.is_some() && self.current_js_home_object(task).is_some();
        let effective_options = if direct_env.is_some() {
            DynamicJsCompileOptions {
                allow_new_target: options.allow_new_target || inherited_allow_new_target,
                allow_super_property: options.allow_super_property
                    || inherited_allow_super_property,
                ..options.clone()
            }
        } else {
            options.clone()
        };
        self.preflight_direct_eval_source(source, &effective_options)
            .map_err(|error| match error {
                VmError::SyntaxError(message) => {
                    self.raise_task_builtin_error(task, "SyntaxError", message)
                }
                other => other,
            })?;
        let raw_eval_declarations = self.raw_direct_eval_declarations(source);
        let direct_eval_declarations = if direct_env.is_some() {
            raw_eval_declarations.clone()
        } else {
            None
        };
        let caller_is_strict = module
            .functions
            .get(task.current_func_id())
            .filter(|function| function.is_strict_js)
            .is_some();
        let behavior = DirectEvalBehavior {
            is_strict: caller_is_strict
                || direct_eval_declarations
                    .as_ref()
                    .is_some_and(|collector| collector.source_is_strict),
            publish_script_global_bindings: effective_options.uses_script_global_bindings
                && !(caller_is_strict
                    || direct_eval_declarations
                        .as_ref()
                        .is_some_and(|collector| collector.source_is_strict)),
            persist_caller_declarations: direct_env.is_some()
                && !effective_options.uses_script_global_bindings
                && !(caller_is_strict
                    || direct_eval_declarations
                        .as_ref()
                        .is_some_and(|collector| collector.source_is_strict)),
        };
        let inherited_strict_prefix = if caller_is_strict {
            "\"use strict\";\n"
        } else {
            ""
        };
        let caller_has_own_arguments_binding = module
            .functions
            .get(task.current_func_id())
            .is_some_and(|function| function.uses_js_this_slot);
        if direct_env.is_some()
            && caller_has_parameter_named_arguments
            && direct_eval_declarations
                .as_ref()
                .is_some_and(|collector| collector.declares_arguments)
        {
            return Err(self.raise_task_builtin_error(
                task,
                "SyntaxError",
                "direct eval may not declare 'arguments' when the caller parameter environment already binds it",
            ));
        }
        if direct_env.is_some()
            && in_parameter_initializer
            && caller_has_own_arguments_binding
            && direct_eval_declarations
                .as_ref()
                .is_some_and(|collector| collector.declares_arguments)
        {
            return Err(self.raise_task_builtin_error(
                task,
                "SyntaxError",
                "direct eval may not declare 'arguments' during parameter initialization when the caller already provides an arguments binding",
            ));
        }
        if let (Some(env), Some(declarations)) = (
            direct_env.filter(|_| in_parameter_initializer && !behavior.is_strict),
            direct_eval_declarations.as_ref(),
        ) {
            self.preflight_direct_eval_parameter_var_collisions(env, declarations, task)?;
        }
        if direct_env.is_none() {
            let source_is_strict = raw_eval_declarations
                .as_ref()
                .is_some_and(|collector| collector.source_is_strict);
            if let Some(declarations) = &raw_eval_declarations {
                if !source_is_strict {
                    let Some(global_this) = self.ensure_builtin_global_value("globalThis", task)?
                    else {
                        return Err(self.raise_task_builtin_error(
                            task,
                            "ReferenceError",
                            "globalThis is not available",
                        ));
                    };
                    self.preflight_direct_eval_global_declarations(
                        global_this,
                        declarations,
                        task,
                        module,
                    )?;
                }
            }
            let function_module = self
                .compile_dynamic_js_module_source(
                    source,
                    "__eval__",
                    "Dynamic eval",
                    DynamicJsCompileOptions {
                        track_top_level_completion: true,
                        emit_script_global_bindings: !source_is_strict,
                        script_global_bindings_configurable: !source_is_strict,
                        ..effective_options.clone()
                    },
                )
                .map_err(|error| match error {
                    VmError::SyntaxError(message) => {
                        self.raise_task_builtin_error(task, "SyntaxError", message)
                    }
                    VmError::TypeError(message) => {
                        self.raise_task_builtin_error(task, "TypeError", message)
                    }
                    VmError::ReferenceError(message) => {
                        self.raise_task_builtin_error(task, "ReferenceError", message)
                    }
                    other => other,
                })?;
            return self.execute_dynamic_js_module_main(function_module, task);
        }

        let (function_name, wrapped, options, explicit_this) = if direct_env.is_some() {
            (
                "__direct_eval__",
                format!("function __direct_eval__() {{\n{inherited_strict_prefix}{source}\n}}\n"),
                DynamicJsCompileOptions {
                    direct_eval_entry_function: Some("__direct_eval__".to_string()),
                    has_parameter_named_arguments: caller_has_parameter_named_arguments,
                    in_parameter_initializer,
                    uses_script_global_bindings: behavior.publish_script_global_bindings,
                    allow_new_target: effective_options.allow_new_target,
                    allow_super_property: effective_options.allow_super_property,
                    ..DynamicJsCompileOptions::default()
                },
                Some(self.current_direct_eval_this_value(stack, task, module)),
            )
        } else {
            unreachable!("indirect eval returns through raw-module path")
        };
        let uses_script_global_bindings = options.uses_script_global_bindings;
        let function_module = self
            .compile_dynamic_js_module_source(&wrapped, "__eval__", "Dynamic eval", options)
            .map_err(|error| match error {
                VmError::SyntaxError(message) => {
                    self.raise_task_builtin_error(task, "SyntaxError", message)
                }
                VmError::TypeError(message) => {
                    self.raise_task_builtin_error(task, "TypeError", message)
                }
                VmError::ReferenceError(message) => {
                    self.raise_task_builtin_error(task, "ReferenceError", message)
                }
                other => other,
            })?;
        let closure_val = self.alloc_dynamic_js_closure(
            function_module,
            function_name,
            "Dynamic eval module registration error",
            "Dynamic eval compile did not produce wrapper function",
        )?;
        if let Some(caller_snapshot_env) = direct_env {
            let active_env = self.current_activation_eval_env(task);
            let active_with_env = active_env.filter(|env| self.with_env_target(*env).is_some());
            let active_persistent_env = task.current_activation_direct_eval_env();
            if let Some(active_env) = active_env {
                if active_with_env.is_none()
                    && active_persistent_env
                        .is_none_or(|persistent_env| persistent_env.raw() != active_env.raw())
                    && caller_snapshot_env.raw() != active_env.raw()
                    && self.direct_eval_outer_env(caller_snapshot_env).is_none()
                {
                    self.set_direct_eval_outer_env(caller_snapshot_env, active_env)?;
                }
            }
            self.seal_direct_eval_snapshot_env(caller_snapshot_env)?;
            let runtime_outer_env = active_with_env.unwrap_or(caller_snapshot_env);
            let runtime_env = if behavior.publish_script_global_bindings {
                if active_with_env.is_some() {
                    self.alloc_direct_eval_runtime_env(runtime_outer_env)?
                } else {
                    // Script-global direct eval should run against the caller snapshot
                    // itself. Global publication happens after execution, so the eval body
                    // needs live access to the caller-visible bindings without an extra
                    // shadow wrapper in front of them.
                    self.set_direct_eval_completion(caller_snapshot_env, Value::undefined())?;
                    caller_snapshot_env
                }
            } else if behavior.persist_caller_declarations {
                if active_with_env.is_some() {
                    self.alloc_direct_eval_runtime_env(runtime_outer_env)?
                } else if let Some(existing_env) = task.current_activation_direct_eval_env() {
                    if existing_env.raw() != caller_snapshot_env.raw() {
                        self.set_direct_eval_outer_env(existing_env, caller_snapshot_env)?;
                    }
                    existing_env
                } else {
                    // First-entry sloppy direct eval should operate on the caller snapshot
                    // itself so `var` redeclarations and assignments target the caller-owned
                    // binding record. We only introduce a layered runtime env once there is
                    // already persisted eval state to preserve across calls.
                    self.set_direct_eval_completion(caller_snapshot_env, Value::undefined())?;
                    caller_snapshot_env
                }
            } else {
                self.alloc_direct_eval_runtime_env(runtime_outer_env)?
            };
            if let Some(closure_ptr) = unsafe { closure_val.as_ptr::<Object>() } {
                let closure = unsafe { &mut *closure_ptr.as_ptr() };
                let _ = closure.set_callable_direct_eval_env(runtime_env);
                let _ = closure.set_callable_direct_eval_uses_script_global_bindings(
                    behavior.publish_script_global_bindings,
                );
            }
            if behavior.publish_script_global_bindings {
                if let Some(declarations) = &direct_eval_declarations {
                    self.preflight_direct_eval_global_declarations(
                        runtime_env,
                        declarations,
                        task,
                        module,
                    )?;
                }
            }
            if let Some(declarations) = &direct_eval_declarations {
                self.predeclare_direct_eval_lexical_declarations(runtime_env, declarations)?;
            }
            task.push_active_direct_eval_env(
                runtime_env,
                behavior.is_strict,
                behavior.publish_script_global_bindings,
                behavior.persist_caller_declarations,
            );
            let result =
                self.invoke_callable_sync_with_this(closure_val, explicit_this, &[], task, module);
            let _ = task.pop_active_direct_eval_env();
            if behavior.publish_script_global_bindings {
                if let Some(declarations) = &direct_eval_declarations {
                    self.sync_direct_eval_global_bindings(runtime_env, declarations, task, module)?;
                }
            }
            if std::env::var("RAYA_DEBUG_DIRECT_EVAL_RESULT").is_ok() {
                match &result {
                    Ok(value) => {
                        eprintln!(
                            "[direct-eval-result] source={source:?} value_raw={:#x} string={:?}",
                            value.raw(),
                            primitive_to_js_string(*value),
                        );
                    }
                    Err(error) => {
                        eprintln!("[direct-eval-result] source={source:?} error={error}");
                    }
                }
            }
            if result.is_ok() && behavior.persist_caller_declarations {
                task.set_activation_direct_eval_env(
                    task.current_func_id(),
                    task.current_locals_base(),
                    runtime_env,
                );
            }
            result
        } else {
            self.invoke_callable_sync_with_this(closure_val, explicit_this, &[], task, module)
        }
    }

    fn preflight_direct_eval_global_declarations(
        &mut self,
        env: Value,
        declarations: &DirectEvalDeclarations,
        task: &Arc<Task>,
        _module: &Module,
    ) -> Result<(), VmError> {
        let Some(global_this) = self.builtin_global_value("globalThis") else {
            return Ok(());
        };

        for name in declarations
            .var_names
            .iter()
            .chain(declarations.function_names.iter())
        {
            if self.has_property_via_js_semantics(env, name)
                && self.own_js_property_flags(global_this, name).is_none()
            {
                return Err(self.raise_task_builtin_error(
                    task,
                    "SyntaxError",
                    format!(
                        "direct eval cannot create global binding '{}' that collides with a global lexical declaration",
                        name
                    ),
                ));
            }
        }

        for name in &declarations.function_names {
            let definable = match self.own_js_property_flags(global_this, name) {
                None => self.is_js_value_extensible(global_this),
                Some((writable, configurable, enumerable)) => {
                    configurable || (writable && enumerable)
                }
            };
            if !definable {
                return Err(self.raise_task_builtin_error(
                    task,
                    "TypeError",
                    format!("direct eval cannot declare global function '{}'", name),
                ));
            }
        }

        for name in &declarations.var_names {
            let definable = self.own_js_property_flags(global_this, name).is_some()
                || self.is_js_value_extensible(global_this);
            if !definable {
                return Err(self.raise_task_builtin_error(
                    task,
                    "TypeError",
                    format!("direct eval cannot declare global variable '{}'", name),
                ));
            }
        }
        Ok(())
    }

    fn sync_direct_eval_global_bindings(
        &mut self,
        env: Value,
        declarations: &DirectEvalDeclarations,
        task: &Arc<Task>,
        module: &Module,
    ) -> Result<(), VmError> {
        let Some(global_this) = self.builtin_global_value("globalThis") else {
            return Ok(());
        };

        for name in &declarations.var_names {
            let value = self
                .get_property_value_via_js_semantics_with_context(env, name, task, module)?
                .unwrap_or(Value::undefined());
            if !self.has_property_via_js_semantics(global_this, name) {
                if self.is_js_value_extensible(global_this) {
                    self.define_data_property_on_target(
                        global_this,
                        name,
                        value,
                        true,
                        true,
                        true,
                    )?;
                }
            } else {
                let _ = self.set_property_value_via_js_semantics(
                    global_this,
                    name,
                    value,
                    global_this,
                    task,
                    module,
                )?;
            }
        }

        for name in &declarations.function_names {
            let value = self
                .get_property_value_via_js_semantics_with_context(env, name, task, module)?
                .unwrap_or(Value::undefined());
            if !self.has_property_via_js_semantics(global_this, name) {
                self.define_data_property_on_target(global_this, name, value, true, true, true)?;
            }
        }

        Ok(())
    }

    pub(in crate::vm::interpreter) fn collect_apply_arguments(
        &mut self,
        arg_list: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Vec<Value>, VmError> {
        fn alloc_argument_list(capacity: usize) -> Result<Vec<Value>, VmError> {
            let mut values = Vec::new();
            values
                .try_reserve(capacity)
                .map_err(|_| VmError::RangeError("Argument list too large".to_string()))?;
            Ok(values)
        }

        if arg_list.is_null() || arg_list.is_undefined() {
            return Ok(Vec::new());
        }

        if let Some(array_ptr) = checked_array_ptr(arg_list) {
            let array = unsafe { &*array_ptr.as_ptr() };
            let mut values = alloc_argument_list(array.len())?;
            for index in 0..array.len() {
                values.push(array.get(index).unwrap_or(Value::undefined()));
            }
            return Ok(values);
        }

        if !self.js_value_supports_extensibility(arg_list) {
            return Err(VmError::TypeError(
                "Function.prototype.apply expects an array-like argument list".to_string(),
            ));
        }

        let length_value = self
            .get_property_value_via_js_semantics_with_context(
                arg_list,
                "length",
                caller_task,
                caller_module,
            )?
            .unwrap_or(Value::undefined());
        let length_number =
            self.js_to_number_with_context(length_value, caller_task, caller_module)?;
        let length = if length_number.is_nan() || length_number <= 0.0 {
            0
        } else if length_number.is_infinite() {
            usize::MAX
        } else {
            length_number.floor().min(usize::MAX as f64) as usize
        };
        let mut values = alloc_argument_list(length)?;
        for index in 0..length {
            values.push(
                self.get_property_value_via_js_semantics_with_context(
                    arg_list,
                    &index.to_string(),
                    caller_task,
                    caller_module,
                )?
                .unwrap_or(Value::undefined()),
            );
        }
        Ok(values)
    }

    fn nominal_type_id_from_imported_class_value(
        &self,
        module: &Module,
        value: Value,
    ) -> Option<usize> {
        if let Some(global_name) = self.builtin_global_name_for_value(value) {
            let classes = self.classes.read();
            if let Some(class) = classes.get_class_by_name(&global_name) {
                return Some(class.id);
            }
        }

        if let Some(nominal_id) = self.type_handle_nominal_id(value) {
            return Some(nominal_id as usize);
        }

        if let Some(local_nominal_type_id) = value.as_i32() {
            return self
                .resolve_nominal_type_id(module, local_nominal_type_id as usize)
                .ok();
        }
        if let Some(local_nominal_type_id) = value.as_u32() {
            return self
                .resolve_nominal_type_id(module, local_nominal_type_id as usize)
                .ok();
        }
        if let Some(local_nominal_type_id) = value.as_u64() {
            return self
                .resolve_nominal_type_id(module, local_nominal_type_id as usize)
                .ok();
        }

        None
    }

    pub(in crate::vm::interpreter) fn get_field_index_for_value(
        &self,
        obj_val: Value,
        field_name: &str,
    ) -> Option<usize> {
        let obj_ptr = checked_object_ptr(obj_val)?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let nominal_type_id = obj.nominal_type_id_usize();
        let class_metadata = self.class_metadata.read();
        let metadata_index = nominal_type_id
            .and_then(|nominal_type_id| class_metadata.get(nominal_type_id))
            .and_then(|meta| meta.get_field_index(field_name));
        if metadata_index.is_some() {
            return metadata_index;
        }
        if let Some(index) = self.structural_field_slot_index_for_object(obj, field_name) {
            if index < obj.field_count() {
                return Some(index);
            }
        }
        if nominal_type_id.is_some() {
            return None;
        }
        None
    }

    fn get_own_field_value_by_name(&self, obj_val: Value, field_name: &str) -> Option<Value> {
        if self.fixed_property_deleted(obj_val, field_name) {
            return None;
        }
        if let Some(value) = self.metadata_data_property_value(obj_val, field_name) {
            return Some(value);
        }
        if let Some(value) = self.callable_virtual_property_value(obj_val, field_name) {
            return Some(value);
        }
        if self.is_typed_array_like_value(obj_val) {
            if field_name == "length" {
                let len = self.typed_array_live_length_direct(obj_val)?;
                return Some(if len <= i32::MAX as usize {
                    Value::i32(len as i32)
                } else {
                    Value::f64(len as f64)
                });
            }
            if let Some(index) = parse_js_array_index_name(field_name) {
                let len = self.typed_array_live_length_direct(obj_val)?;
                if index >= len {
                    return None;
                }
                return self.typed_array_index_value_direct(obj_val, index);
            }
        }
        let obj_ptr = checked_object_ptr(obj_val)?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        if self.is_descriptor_object(obj_val)
            && matches!(
                field_name,
                "value" | "writable" | "configurable" | "enumerable" | "get" | "set"
            )
            && !self.descriptor_field_present(obj_val, field_name)
        {
            return None;
        }
        let debug_field_lookup = std::env::var("RAYA_DEBUG_FIELD_LOOKUP").is_ok();
        if debug_field_lookup {
            eprintln!(
                "[field.lookup] target={:#x} key={} layout={} nominal={:?} dyn_map={} field_count={}",
                obj_val.raw(),
                field_name,
                obj.layout_id(),
                obj.nominal_type_id(),
                obj.dyn_props().is_some(),
                obj.field_count()
            );
        }
        if let Some(index) = self.get_field_index_for_value(obj_val, field_name) {
            if let Some(value) = obj.get_field(index) {
                if !value.is_null()
                    || self
                        .callable_virtual_property_value(obj_val, field_name)
                        .is_none()
                {
                    return Some(value);
                }
            }
        }
        let key = self.intern_prop_key(field_name);
        if debug_field_lookup {
            eprintln!("[field.lookup] target={:#x} dyn-key={}", obj_val.raw(), key);
        }
        obj.dyn_props().and_then(|dp| dp.get(key).map(|p| p.value))
    }

    fn get_own_js_property_value_by_name_on_target(
        &self,
        target: Value,
        key: &str,
    ) -> Option<Value> {
        if let Some(kind) = self.exotic_adapter_kind(target) {
            match kind {
                JsExoticAdapterKind::Arguments => {
                    if let Ok(Some(value)) = self.arguments_exotic_get(target, key) {
                        return Some(value);
                    }
                }
                JsExoticAdapterKind::Array => {
                    if let Some(array_ptr) = checked_array_ptr(target) {
                        let array = unsafe { &*array_ptr.as_ptr() };
                        if let Some(property) = self.array_exotic_current_own_property(target, key)
                        {
                            if let OrdinaryOwnProperty::Data { value, .. } = property {
                                return Some(value);
                            }
                            return None;
                        }
                        if key == "length" {
                            let len = array.len();
                            return Some(if len <= i32::MAX as usize {
                                Value::i32(len as i32)
                            } else {
                                Value::f64(len as f64)
                            });
                        }
                        if let Some(index) = parse_js_array_index_name(key) {
                            return array.get(index);
                        }
                        if let Some(value) = self.metadata_data_property_value(target, key) {
                            return Some(value);
                        }
                    }
                }
                JsExoticAdapterKind::String => {
                    if key == "length" {
                        let len = self.string_exotic_length(target)?;
                        return Some(if len <= i32::MAX as usize {
                            Value::i32(len as i32)
                        } else {
                            Value::f64(len as f64)
                        });
                    }
                    if let Some(index) = parse_js_array_index_name(key) {
                        return self.string_exotic_index_value(target, index);
                    }
                }
                JsExoticAdapterKind::TypedArray => {
                    if key == "length" {
                        let len = self.typed_array_live_length_direct(target)?;
                        return Some(if len <= i32::MAX as usize {
                            Value::i32(len as i32)
                        } else {
                            Value::f64(len as f64)
                        });
                    }
                    if let Some(index) = parse_js_array_index_name(key) {
                        let len = self.typed_array_live_length_direct(target)?;
                        if index >= len {
                            return None;
                        }
                        return self.typed_array_index_value_direct(target, index);
                    }
                }
            }
        }

        match self.ordinary_own_property(target, key) {
            Some(OrdinaryOwnProperty::Data { value, .. }) => Some(value),
            Some(OrdinaryOwnProperty::Accessor { .. }) => None,
            None => self.metadata_data_property_value(target, key),
        }
    }

    fn get_own_js_property_value_by_name(&self, target: Value, key: &str) -> Option<Value> {
        self.get_own_js_property_value_by_name_on_target(target, key)
            .or_else(|| {
                let public_target = self.public_property_target(target);
                (public_target.raw() != target.raw())
                    .then(|| self.get_own_js_property_value_by_name_on_target(public_target, key))
                    .flatten()
            })
    }

    fn is_typed_array_like_value(&self, target: Value) -> bool {
        let debug_typed_array = std::env::var("RAYA_DEBUG_TYPED_ARRAY_PROP").is_ok();
        let Some(obj_ptr) = checked_object_ptr(target) else {
            return false;
        };
        let Some(mut nominal_type_id) = (unsafe { &*obj_ptr.as_ptr() }).nominal_type_id_usize()
        else {
            return false;
        };

        let classes = self.classes.read();
        loop {
            let Some(class) = classes.get_class(nominal_type_id) else {
                return false;
            };
            if debug_typed_array {
                eprintln!(
                    "[typed-array.kind] target={:#x} nominal={} class={}",
                    target.raw(),
                    nominal_type_id,
                    class.name
                );
            }
            match class.name.as_str() {
                "Uint8Array" | "Int8Array" | "Uint16Array" | "Int16Array" | "Uint32Array"
                | "Int32Array" | "Float16Array" | "Float32Array" | "Float64Array"
                | "Uint8ClampedArray" | "BigInt64Array" | "BigUint64Array" | "TypedArray" => {
                    return true
                }
                _ => {
                    let Some(parent_id) = class.parent_id else {
                        return false;
                    };
                    nominal_type_id = parent_id;
                }
            }
        }
    }

    fn typed_array_runtime_class_name(&self, target: Value) -> Option<String> {
        let obj_ptr = checked_object_ptr(target)?;
        let mut nominal_type_id = unsafe { &*obj_ptr.as_ptr() }.nominal_type_id_usize()?;
        let classes = self.classes.read();
        loop {
            let class = classes.get_class(nominal_type_id)?;
            match class.name.as_str() {
                "Uint8Array" | "Int8Array" | "Uint16Array" | "Int16Array" | "Uint32Array"
                | "Int32Array" | "Float16Array" | "Float32Array" | "Float64Array"
                | "Uint8ClampedArray" | "BigInt64Array" | "BigUint64Array" | "TypedArray" => {
                    return Some(class.name.clone())
                }
                _ => {
                    nominal_type_id = class.parent_id?;
                }
            }
        }
    }

    fn typed_array_bytes_per_element(&self, class_name: &str) -> isize {
        match class_name {
            "Uint8Array" | "Int8Array" | "Uint8ClampedArray" | "TypedArray" => 1,
            "Uint16Array" | "Int16Array" | "Float16Array" => 2,
            "Uint32Array" | "Int32Array" | "Float32Array" => 4,
            "Float64Array" | "BigInt64Array" | "BigUint64Array" => 8,
            _ => 1,
        }
    }

    fn is_symbol_value(&self, value: Value) -> bool {
        self.nominal_class_name_for_value(value)
            .as_deref()
            .is_some_and(|name| name == "Symbol")
    }

    pub(in crate::vm::interpreter) fn symbol_property_key_name(
        &self,
        symbol_value: Value,
    ) -> Option<String> {
        if !self.is_symbol_value(symbol_value) {
            return None;
        }
        let description = self
            .get_field_value_by_name(symbol_value, "key")
            .and_then(|value| unsafe { value.as_ptr::<RayaString>() })
            .map(|ptr| unsafe { &*ptr.as_ptr() }.data.clone())
            .unwrap_or_default();
        if description.starts_with("Symbol.") {
            Some(description)
        } else {
            Some(format!("{SYMBOL_KEY_PREFIX}{:016x}", symbol_value.raw()))
        }
    }

    fn symbol_value_from_property_key_name(&self, key: &str) -> Option<Value> {
        if let Some(raw_hex) = key.strip_prefix(SYMBOL_KEY_PREFIX) {
            let raw = u64::from_str_radix(raw_hex, 16).ok()?;
            let value = unsafe { Value::from_raw(raw) };
            return self.is_symbol_value(value).then_some(value);
        }

        let static_name = key.strip_prefix("Symbol.")?;
        let symbol_ctor = self.builtin_global_value("Symbol")?;
        self.get_field_value_by_name(symbol_ctor, static_name)
            .filter(|value| self.is_symbol_value(*value))
    }

    fn own_exotic_state_value(&self, target: Value, key: &str) -> Option<Value> {
        self.get_own_js_property_value_by_name(target, key)
    }

    fn is_public_string_property_name(name: &str) -> bool {
        !name.starts_with("Symbol.") && !name.starts_with(SYMBOL_KEY_PREFIX)
    }

    fn invoke_proxy_property_trap_with_context(
        &mut self,
        trap: Value,
        handler: Value,
        target: Value,
        key: &str,
        extra_args: &[Value],
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Value, VmError> {
        let key_ptr = self.gc.lock().allocate(RayaString::new(key.to_string()));
        let key_value = unsafe {
            Value::from_ptr(std::ptr::NonNull::new(key_ptr.as_ptr()).expect("proxy key ptr"))
        };
        self.ephemeral_gc_roots.write().push(key_value);
        let mut trap_args = Vec::with_capacity(2 + extra_args.len());
        trap_args.push(target);
        trap_args.push(key_value);
        trap_args.extend_from_slice(extra_args);
        let result = self.invoke_callable_sync_with_this(
            trap,
            Some(handler),
            &trap_args,
            caller_task,
            caller_module,
        );
        let mut ephemeral = self.ephemeral_gc_roots.write();
        if let Some(index) = ephemeral
            .iter()
            .rposition(|candidate| *candidate == key_value)
        {
            ephemeral.swap_remove(index);
        }
        result
    }

    fn proxy_target_own_property_record_with_context(
        &mut self,
        target: Value,
        key: &str,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Option<JsOwnPropertyRecord>, VmError> {
        self.resolve_own_property_record_with_context(target, key, caller_task, caller_module)
    }

    fn proxy_invariant_error(&self, key: &str, op: &str) -> VmError {
        VmError::TypeError(format!("Proxy {op} trap violated invariant for '{key}'"))
    }

    fn enforce_proxy_get_invariants(
        &mut self,
        target: Value,
        key: &str,
        trap_result: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<(), VmError> {
        let Some(record) = self.proxy_target_own_property_record_with_context(
            target,
            key,
            caller_task,
            caller_module,
        )?
        else {
            return Ok(());
        };
        if !record.shape.configurable {
            match record.shape.kind {
                JsOwnPropertyKind::Data => {
                    if !record.shape.writable
                        && !value_same_value(
                            trap_result,
                            record.value.unwrap_or(Value::undefined()),
                        )
                    {
                        return Err(self.proxy_invariant_error(key, "get"));
                    }
                }
                JsOwnPropertyKind::Accessor | JsOwnPropertyKind::PoisonedAccessor => {
                    let getter = record.getter.unwrap_or(Value::undefined());
                    if getter.is_undefined() && !trap_result.is_undefined() {
                        return Err(self.proxy_invariant_error(key, "get"));
                    }
                }
            }
        }
        Ok(())
    }

    fn enforce_proxy_set_invariants(
        &mut self,
        target: Value,
        key: &str,
        value: Value,
        trap_result: bool,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<(), VmError> {
        if !trap_result {
            return Ok(());
        }
        let Some(record) = self.proxy_target_own_property_record_with_context(
            target,
            key,
            caller_task,
            caller_module,
        )?
        else {
            return Ok(());
        };
        if !record.shape.configurable {
            match record.shape.kind {
                JsOwnPropertyKind::Data => {
                    if !record.shape.writable
                        && !value_same_value(value, record.value.unwrap_or(Value::undefined()))
                    {
                        return Err(self.proxy_invariant_error(key, "set"));
                    }
                }
                JsOwnPropertyKind::Accessor | JsOwnPropertyKind::PoisonedAccessor => {
                    let setter = record.setter.unwrap_or(Value::undefined());
                    if setter.is_undefined() {
                        return Err(self.proxy_invariant_error(key, "set"));
                    }
                }
            }
        }
        Ok(())
    }

    fn enforce_proxy_has_invariants(
        &mut self,
        target: Value,
        key: &str,
        trap_result: bool,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<(), VmError> {
        if trap_result {
            return Ok(());
        }
        let Some(record) = self.proxy_target_own_property_record_with_context(
            target,
            key,
            caller_task,
            caller_module,
        )?
        else {
            return Ok(());
        };
        if !record.shape.configurable || !self.is_js_value_extensible(target) {
            return Err(self.proxy_invariant_error(key, "has"));
        }
        Ok(())
    }

    fn enforce_proxy_delete_invariants(
        &mut self,
        target: Value,
        key: &str,
        trap_result: bool,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<(), VmError> {
        if !trap_result {
            return Ok(());
        }
        let Some(record) = self.proxy_target_own_property_record_with_context(
            target,
            key,
            caller_task,
            caller_module,
        )?
        else {
            return Ok(());
        };
        if !record.shape.configurable || !self.is_js_value_extensible(target) {
            return Err(self.proxy_invariant_error(key, "deleteProperty"));
        }
        Ok(())
    }

    fn enforce_proxy_define_invariants(
        &mut self,
        target: Value,
        key: &str,
        descriptor: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<(), VmError> {
        let record = self.validate_descriptor_for_definition(key, descriptor)?;
        let target_record = self.proxy_target_own_property_record_with_context(
            target,
            key,
            caller_task,
            caller_module,
        )?;
        let setting_config_false = record.has_configurable && !record.configurable;

        match target_record {
            None => {
                if !self.is_js_value_extensible(target) || setting_config_false {
                    return Err(self.proxy_invariant_error(key, "defineProperty"));
                }
            }
            Some(current_record) => {
                if setting_config_false && current_record.shape.configurable {
                    return Err(self.proxy_invariant_error(key, "defineProperty"));
                }
                self.validate_existing_property_descriptor(
                    key,
                    current_record.as_ordinary_property(),
                    record,
                )
                .map_err(|_| self.proxy_invariant_error(key, "defineProperty"))?;
            }
        }

        Ok(())
    }

    fn try_proxy_get_property_with_invariants(
        &mut self,
        value: Value,
        key: &str,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Option<Value>, VmError> {
        let Some(proxy) = crate::vm::reflect::try_unwrap_proxy(value) else {
            return Ok(None);
        };

        if proxy.handler.is_null() {
            return Err(VmError::TypeError("Proxy has been revoked".to_string()));
        }

        if let Some(getter) = self.get_field_value_by_name(proxy.handler, "get") {
            let result = self.invoke_proxy_property_trap_with_context(
                getter,
                proxy.handler,
                proxy.target,
                key,
                &[],
                caller_task,
                caller_module,
            )?;
            self.enforce_proxy_get_invariants(
                proxy.target,
                key,
                result,
                caller_task,
                caller_module,
            )?;
            return Ok(Some(result));
        }

        Ok(None)
    }

    fn try_proxy_set_property_with_invariants(
        &mut self,
        target: Value,
        key: &str,
        value: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Option<bool>, VmError> {
        let Some(proxy) = crate::vm::reflect::try_unwrap_proxy(target) else {
            return Ok(None);
        };
        if proxy.handler.is_null() {
            return Err(VmError::TypeError("Proxy has been revoked".to_string()));
        }
        if let Some(setter) = self.get_field_value_by_name(proxy.handler, "set") {
            if !setter.is_undefined() && !setter.is_null() {
                let trap_result = self
                    .invoke_proxy_property_trap_with_context(
                        setter,
                        proxy.handler,
                        proxy.target,
                        key,
                        &[value],
                        caller_task,
                        caller_module,
                    )?
                    .is_truthy();
                self.enforce_proxy_set_invariants(
                    proxy.target,
                    key,
                    value,
                    trap_result,
                    caller_task,
                    caller_module,
                )?;
                return Ok(Some(trap_result));
            }
        }
        Ok(None)
    }

    pub(in crate::vm::interpreter) fn try_proxy_has_property_with_invariants(
        &mut self,
        target: Value,
        key: &str,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Option<bool>, VmError> {
        let Some(proxy) = crate::vm::reflect::try_unwrap_proxy(target) else {
            return Ok(None);
        };
        if proxy.handler.is_null() {
            return Err(VmError::TypeError("Proxy has been revoked".to_string()));
        }
        if let Some(has_trap) = self.get_field_value_by_name(proxy.handler, "has") {
            if !has_trap.is_undefined() && !has_trap.is_null() {
                let trap_result = self
                    .invoke_proxy_property_trap_with_context(
                        has_trap,
                        proxy.handler,
                        proxy.target,
                        key,
                        &[],
                        caller_task,
                        caller_module,
                    )?
                    .is_truthy();
                self.enforce_proxy_has_invariants(
                    proxy.target,
                    key,
                    trap_result,
                    caller_task,
                    caller_module,
                )?;
                return Ok(Some(trap_result));
            }
        }
        Ok(None)
    }

    fn numeric_value_as_isize(&self, value: Value) -> Option<isize> {
        if let Some(v) = value.as_i32() {
            return Some(v as isize);
        }
        if let Some(v) = value.as_i64() {
            return isize::try_from(v).ok();
        }
        if let Some(v) = value.as_f64() {
            if v.is_finite() {
                return Some(v as isize);
            }
        }
        None
    }

    fn numeric_value_as_usize(&self, value: Value) -> Option<usize> {
        let value = self.numeric_value_as_isize(value)?;
        if value < 0 {
            return None;
        }
        Some(value as usize)
    }

    fn typed_array_raw_length_direct(&self, target: Value, bytes_per_element: isize) -> isize {
        let Some(buffer) = self.typed_array_backing_field_value(target, "buffer") else {
            return -1;
        };
        let byte_length = self
            .own_exotic_state_value(buffer, "byteLength")
            .and_then(|value| self.numeric_value_as_isize(value))
            .unwrap_or(0);
        let byte_offset = self
            .typed_array_backing_field_value(target, "byteOffset")
            .and_then(|value| self.numeric_value_as_isize(value))
            .unwrap_or(0);
        let fixed_length = self
            .typed_array_backing_field_value(target, "_fixedLength")
            .and_then(|value| self.numeric_value_as_isize(value))
            .unwrap_or(0);
        let length_tracking = self
            .typed_array_backing_field_value(target, "_lengthTracking")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

        let available = byte_length - byte_offset;
        if available < 0 {
            return -1;
        }
        if length_tracking {
            return (available / bytes_per_element).max(0);
        }
        if fixed_length * bytes_per_element > available {
            return -1;
        }
        fixed_length.max(0)
    }

    fn typed_array_live_length_direct(&self, target: Value) -> Option<usize> {
        if !self.is_typed_array_like_value(target) {
            return None;
        }
        let class_name = self.typed_array_runtime_class_name(target)?;
        let bytes_per_element = self.typed_array_bytes_per_element(&class_name);
        Some(
            self.typed_array_raw_length_direct(target, bytes_per_element)
                .max(0) as usize,
        )
    }

    fn typed_array_live_length_with_context(
        &mut self,
        target: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Option<usize>, VmError> {
        if !self.is_typed_array_like_value(target) {
            return Ok(None);
        }
        if let Some(method) = self.get_field_value_on_target_by_name(target, "__currentLength") {
            if Self::is_callable_value(method) {
                let length = self.invoke_callable_sync_with_this(
                    method,
                    Some(target),
                    &[],
                    caller_task,
                    caller_module,
                )?;
                let normalized = length
                    .as_i32()
                    .map(|value| value as i64)
                    .or_else(|| length.as_f64().map(|value| value as i64))
                    .unwrap_or(0)
                    .max(0) as usize;
                return Ok(Some(normalized));
            }
        }
        Ok(self.typed_array_live_length_direct(target))
    }

    fn array_buffer_byte_at(&self, buffer: Value, offset: usize) -> Option<u8> {
        let bytes = self.own_exotic_state_value(buffer, "_bytes")?;
        let array_ptr = checked_array_ptr(bytes)?;
        let array = unsafe { &*array_ptr.as_ptr() };
        let value = array.get(offset)?;
        if let Some(i) = value.as_i32() {
            return Some(i as u8);
        }
        value.as_f64().map(|f| f as u8)
    }

    fn typed_array_index_value_direct(&self, target: Value, index: usize) -> Option<Value> {
        let debug_typed_array = std::env::var("RAYA_DEBUG_TYPED_ARRAY_PROP").is_ok();
        let len = self.typed_array_live_length_direct(target)?;
        if debug_typed_array {
            eprintln!(
                "[typed-array.direct] target={:#x} index={} len={}",
                target.raw(),
                index,
                len
            );
        }
        if index >= len {
            return Some(Value::undefined());
        }

        let class_name = self.typed_array_runtime_class_name(target)?;
        if debug_typed_array {
            eprintln!(
                "[typed-array.direct] target={:#x} index={} class={}",
                target.raw(),
                index,
                class_name
            );
        }
        match class_name.as_str() {
            "Uint8Array" => {
                let buffer = self.typed_array_backing_field_value(target, "buffer")?;
                let byte_offset = self.numeric_value_as_usize(
                    self.typed_array_backing_field_value(target, "byteOffset")?,
                )?;
                if debug_typed_array {
                    eprintln!(
                        "[typed-array.direct] target={:#x} index={} byteOffset={}",
                        target.raw(),
                        index,
                        byte_offset
                    );
                }
                self.array_buffer_byte_at(buffer, byte_offset + index)
                    .map(|byte| Value::i32(byte as i32))
            }
            "Int8Array" => {
                let inner = self.own_exotic_state_value(target, "_u8")?;
                let value = self.typed_array_index_value_direct(inner, index)?;
                let raw = value.as_i32()?;
                Some(Value::i32(if raw > 127 { raw - 256 } else { raw }))
            }
            "Uint8ClampedArray" => {
                let inner = self.own_exotic_state_value(target, "_u8")?;
                self.typed_array_index_value_direct(inner, index)
            }
            "Uint16Array" => {
                let buffer = self.typed_array_backing_field_value(target, "buffer")?;
                let byte_offset = self.numeric_value_as_usize(
                    self.typed_array_backing_field_value(target, "byteOffset")?,
                )?;
                let base = byte_offset + (index << 1);
                let b0 = self.array_buffer_byte_at(buffer, base)? as i32;
                let b1 = self.array_buffer_byte_at(buffer, base + 1)? as i32;
                Some(Value::i32(b0 | (b1 << 8)))
            }
            "Int16Array" => {
                let inner = self.own_exotic_state_value(target, "_u16")?;
                let value = self.typed_array_index_value_direct(inner, index)?;
                let raw = value.as_i32()?;
                Some(Value::i32(if raw > 32767 { raw - 65536 } else { raw }))
            }
            "Int32Array" => {
                let buffer = self.typed_array_backing_field_value(target, "buffer")?;
                let byte_offset = self.numeric_value_as_usize(
                    self.typed_array_backing_field_value(target, "byteOffset")?,
                )?;
                let base = byte_offset + (index << 2);
                let bytes = [
                    self.array_buffer_byte_at(buffer, base)?,
                    self.array_buffer_byte_at(buffer, base + 1)?,
                    self.array_buffer_byte_at(buffer, base + 2)?,
                    self.array_buffer_byte_at(buffer, base + 3)?,
                ];
                Some(Value::i32(i32::from_le_bytes(bytes)))
            }
            "Uint32Array" => {
                let inner = self.own_exotic_state_value(target, "_i32")?;
                let value = self.typed_array_index_value_direct(inner, index)?;
                let raw = value.as_i32()?;
                if raw < 0 {
                    Some(Value::f64(raw as f64 + 4294967296.0))
                } else {
                    Some(Value::i32(raw))
                }
            }
            "Float32Array" => {
                let inner = self.own_exotic_state_value(target, "_i32")?;
                let value = self.typed_array_index_value_direct(inner, index)?;
                value
                    .as_i32()
                    .map(|raw| Value::f64(raw as f64))
                    .or(Some(value))
            }
            "Float16Array" => {
                let inner = self.own_exotic_state_value(target, "_u16")?;
                let value = self.typed_array_index_value_direct(inner, index)?;
                value
                    .as_i32()
                    .map(|raw| Value::f64(raw as f64))
                    .or(Some(value))
            }
            "Float64Array" => {
                let buffer = self.typed_array_backing_field_value(target, "buffer")?;
                let byte_offset = self.numeric_value_as_usize(
                    self.typed_array_backing_field_value(target, "byteOffset")?,
                )?;
                let base = byte_offset + (index << 3);
                let bytes = [
                    self.array_buffer_byte_at(buffer, base)?,
                    self.array_buffer_byte_at(buffer, base + 1)?,
                    self.array_buffer_byte_at(buffer, base + 2)?,
                    self.array_buffer_byte_at(buffer, base + 3)?,
                    self.array_buffer_byte_at(buffer, base + 4)?,
                    self.array_buffer_byte_at(buffer, base + 5)?,
                    self.array_buffer_byte_at(buffer, base + 6)?,
                    self.array_buffer_byte_at(buffer, base + 7)?,
                ];
                Some(Value::f64(f64::from_le_bytes(bytes)))
            }
            "BigInt64Array" | "BigUint64Array" => {
                let inner = self.own_exotic_state_value(target, "_f64")?;
                self.typed_array_index_value_direct(inner, index)
            }
            "TypedArray" => {
                let buffer = self.typed_array_backing_field_value(target, "buffer")?;
                let byte_offset = self.numeric_value_as_usize(
                    self.typed_array_backing_field_value(target, "byteOffset")?,
                )?;
                self.array_buffer_byte_at(buffer, byte_offset + index)
                    .map(|byte| Value::i32(byte as i32))
            }
            _ => None,
        }
    }

    fn typed_array_index_property_flags(
        &self,
        target: Value,
        key: &str,
    ) -> Option<(bool, bool, bool)> {
        if !self.is_typed_array_like_value(target) {
            return None;
        }
        let index = parse_js_array_index_name(key)?;
        let len = self.typed_array_live_length_direct(target)?;
        (index < len).then_some((true, true, true))
    }

    fn typed_array_index_value_with_context(
        &mut self,
        target: Value,
        index: usize,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Option<Value>, VmError> {
        let debug_typed_array = std::env::var("RAYA_DEBUG_TYPED_ARRAY_PROP").is_ok();
        let Some(len) =
            self.typed_array_live_length_with_context(target, caller_task, caller_module)?
        else {
            if debug_typed_array {
                eprintln!(
                    "[typed-array.get] target={:#x} index={} len=<none>",
                    target.raw(),
                    index
                );
            }
            return Ok(None);
        };
        if debug_typed_array {
            eprintln!(
                "[typed-array.get] target={:#x} index={} len={}",
                target.raw(),
                index,
                len
            );
        }
        if index >= len {
            return Ok(Some(Value::undefined()));
        }
        if let Some(method) = self.get_field_value_on_target_by_name(target, "get") {
            if Self::is_callable_value(method) {
                let index_value = if index <= i32::MAX as usize {
                    Value::i32(index as i32)
                } else {
                    Value::f64(index as f64)
                };
                let value = self.invoke_callable_sync_with_this(
                    method,
                    Some(target),
                    &[index_value],
                    caller_task,
                    caller_module,
                )?;
                if debug_typed_array {
                    eprintln!(
                        "[typed-array.get] target={:#x} index={} via-method={:#x}",
                        target.raw(),
                        index,
                        value.raw()
                    );
                }
                return Ok(Some(value));
            }
        }
        let value = self.typed_array_index_value_direct(target, index);
        if debug_typed_array {
            eprintln!(
                "[typed-array.get] target={:#x} index={} via-direct={:?}",
                target.raw(),
                index,
                value.map(|entry| entry.raw())
            );
        }
        Ok(value)
    }

    fn typed_array_set_index_value_with_context(
        &mut self,
        target: Value,
        index: usize,
        value: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Option<bool>, VmError> {
        let Some(len) =
            self.typed_array_live_length_with_context(target, caller_task, caller_module)?
        else {
            return Ok(None);
        };
        if index >= len {
            return Ok(Some(true));
        }
        if let Some(method) = self.get_field_value_on_target_by_name(target, "set") {
            if Self::is_callable_value(method) {
                let index_value = if index <= i32::MAX as usize {
                    Value::i32(index as i32)
                } else {
                    Value::f64(index as f64)
                };
                let _ = self.invoke_callable_sync_with_this(
                    method,
                    Some(target),
                    &[index_value, value],
                    caller_task,
                    caller_module,
                )?;
                return Ok(Some(true));
            }
        }
        Ok(Some(false))
    }

    fn typed_array_backing_field_value(&self, target: Value, field_name: &str) -> Option<Value> {
        let field_name = match field_name {
            "buffer" => "_buffer",
            "byteOffset" => "_byteOffset",
            "byteLength" => "_byteLength",
            other => other,
        };
        self.get_own_field_value_by_name(target, field_name)
    }

    fn typed_array_define_indexed_property(
        &mut self,
        target: Value,
        key: &str,
        descriptor: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Option<()>, VmError> {
        if !self.is_typed_array_like_value(target) {
            return Ok(None);
        }
        let Some(index) = parse_js_array_index_name(key) else {
            return Ok(None);
        };
        let Some(len) =
            self.typed_array_live_length_with_context(target, caller_task, caller_module)?
        else {
            return Ok(None);
        };
        if index >= len
            || self.descriptor_field_present(descriptor, "get")
            || self.descriptor_field_present(descriptor, "set")
            || (self.descriptor_field_present(descriptor, "configurable")
                && !self.descriptor_flag(descriptor, "configurable", true))
            || (self.descriptor_field_present(descriptor, "enumerable")
                && !self.descriptor_flag(descriptor, "enumerable", true))
            || (self.descriptor_field_present(descriptor, "writable")
                && !self.descriptor_flag(descriptor, "writable", true))
        {
            return Err(VmError::TypeError(format!(
                "Cannot redefine typed array index property '{}'",
                key
            )));
        }
        if let Some(value) = self.get_field_value_by_name(descriptor, "value") {
            match self.typed_array_set_index_value_with_context(
                target,
                index,
                value,
                caller_task,
                caller_module,
            )? {
                Some(true) => {}
                Some(false) => {
                    return Err(VmError::TypeError(format!(
                        "Cannot redefine typed array index property '{}'",
                        key
                    )));
                }
                None => {}
            }
        }
        Ok(Some(()))
    }

    fn typed_array_own_property_value_with_context(
        &mut self,
        target: Value,
        key: &str,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Option<Value>, VmError> {
        let debug_typed_array = std::env::var("RAYA_DEBUG_TYPED_ARRAY_PROP").is_ok();
        let wants_accessor = matches!(key, "buffer" | "byteOffset" | "byteLength" | "length");
        if !wants_accessor && parse_js_array_index_name(key).is_none() {
            return Ok(None);
        }

        if matches!(key, "buffer" | "byteOffset" | "byteLength") {
            let Some(class_name) = self.typed_array_runtime_class_name(target) else {
                return Ok(None);
            };
            let has_backing_slots = self
                .typed_array_backing_field_value(target, "buffer")
                .is_some()
                && self
                    .typed_array_backing_field_value(target, "byteOffset")
                    .is_some()
                && self
                    .typed_array_backing_field_value(target, "byteLength")
                    .is_some();
            if debug_typed_array {
                eprintln!(
                    "[typed-array.own] target={:#x} key={} class={} has_backing_slots={} buffer={:?} byteOffset={:?} byteLength={:?} fixed={:?} length_tracking={:?}",
                    target.raw(),
                    key,
                    class_name,
                    has_backing_slots,
                    self.typed_array_backing_field_value(target, "buffer")
                        .map(|value| value.raw()),
                    self.typed_array_backing_field_value(target, "byteOffset")
                        .map(|value| value.raw()),
                    self.typed_array_backing_field_value(target, "byteLength")
                        .map(|value| value.raw()),
                    self.typed_array_backing_field_value(target, "_fixedLength")
                        .map(|value| value.raw()),
                    self.typed_array_backing_field_value(target, "_lengthTracking")
                        .map(|value| value.raw()),
                );
            }
            if class_name == "TypedArray" || !has_backing_slots {
                return Err(VmError::TypeError(format!(
                    "TypedArray.prototype.{key} called on incompatible receiver"
                )));
            }
            return match key {
                "buffer" => Ok(self.typed_array_backing_field_value(target, "buffer")),
                "byteOffset" => {
                    let bytes_per_element = self.typed_array_bytes_per_element(&class_name);
                    let raw_len = self.typed_array_raw_length_direct(target, bytes_per_element);
                    if raw_len < 0 {
                        Ok(Some(Value::i32(0)))
                    } else {
                        Ok(self.typed_array_backing_field_value(target, "byteOffset"))
                    }
                }
                "byteLength" => {
                    let bytes_per_element = self.typed_array_bytes_per_element(&class_name);
                    let raw_len = self.typed_array_raw_length_direct(target, bytes_per_element);
                    if debug_typed_array {
                        eprintln!(
                            "[typed-array.own] target={:#x} key={} bytes_per_element={} raw_len={}",
                            target.raw(),
                            key,
                            bytes_per_element,
                            raw_len,
                        );
                    }
                    if raw_len < 0 {
                        Ok(Some(Value::i32(0)))
                    } else {
                        let byte_length = raw_len.saturating_mul(bytes_per_element);
                        Ok(Some(if byte_length <= i32::MAX as isize {
                            Value::i32(byte_length as i32)
                        } else {
                            Value::f64(byte_length as f64)
                        }))
                    }
                }
                _ => Ok(None),
            };
        }

        let Some(len) =
            self.typed_array_live_length_with_context(target, caller_task, caller_module)?
        else {
            return Ok(None);
        };

        if key == "length" {
            return Ok(Some(if len <= i32::MAX as usize {
                Value::i32(len as i32)
            } else {
                Value::f64(len as f64)
            }));
        }

        let Some(index) = parse_js_array_index_name(key) else {
            return Ok(None);
        };
        let value = self
            .typed_array_index_value_with_context(target, index, caller_task, caller_module)?
            .unwrap_or(Value::undefined());
        if debug_typed_array {
            eprintln!(
                "[typed-array.prop] target={:#x} key={} value={:#x}",
                target.raw(),
                key,
                value.raw()
            );
        }
        Ok(Some(value))
    }

    fn property_attributes_from_descriptor_metadata(
        &self,
        target: Value,
        key: &str,
        default: (bool, bool, bool),
    ) -> (bool, bool, bool) {
        let Some(descriptor) = self.get_descriptor_metadata(target, key) else {
            return default;
        };
        let is_accessor = self.descriptor_field_present(descriptor, "get")
            || self.descriptor_field_present(descriptor, "set");
        let writable = if is_accessor {
            false
        } else {
            self.descriptor_flag(descriptor, "writable", default.0)
        };
        let configurable = self.descriptor_flag(descriptor, "configurable", default.1);
        let enumerable = self.descriptor_flag(descriptor, "enumerable", default.2);
        (writable, configurable, enumerable)
    }

    fn own_nominal_method_value(&self, target: Value, key: &str) -> Option<Value> {
        let obj_ptr = checked_object_ptr(target)?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let method_slot = obj.nominal_type_id_usize().and_then(|ntid| {
            let classes = self.classes.read();
            if let Some(class) = classes.get_class(ntid) {
                if class.runtime_instance_publication {
                    return None;
                }
                let has_own_accessor = class.prototype_members.iter().any(|member| {
                    member.name == key
                        && matches!(
                            member.kind,
                            crate::vm::object::PrototypeMemberKind::Getter
                                | crate::vm::object::PrototypeMemberKind::Setter
                        )
                });
                if has_own_accessor {
                    return None;
                }
            }
            drop(classes);
            let class_metadata = self.class_metadata.read();
            class_metadata
                .get(ntid)
                .and_then(|meta| meta.get_method_index(key))
                .or_else(|| {
                    drop(class_metadata);
                    let classes = self.classes.read();
                    let class = classes.get_class(ntid)?;
                    let module = class.module.as_ref()?;
                    module
                        .classes
                        .iter()
                        .find(|cd| cd.name == class.name)
                        .and_then(|cd| {
                            cd.methods.iter().find_map(|method| {
                                let plain = method.name.rsplit("::").next().unwrap_or(&method.name);
                                if matches!(
                                    method.kind,
                                    crate::compiler::bytecode::MethodKind::Normal
                                ) && (method.name == key || plain == key)
                                {
                                    Some(method.slot)
                                } else {
                                    None
                                }
                            })
                        })
                })
        })?;
        self.bound_method_value_for_slot(target, method_slot).ok()
    }

    fn nominal_instance_uses_runtime_publication(&self, target: Value) -> bool {
        let Some(obj_ptr) = checked_object_ptr(target) else {
            return false;
        };
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let Some(ntid) = obj.nominal_type_id_usize() else {
            return false;
        };
        let classes = self.classes.read();
        classes
            .get_class(ntid)
            .is_some_and(|class| class.runtime_instance_publication)
    }

    fn own_builtin_native_method_value(&self, target: Value, key: &str) -> Option<Value> {
        if let Some(value) = self.task_promise_method_value(target, key) {
            return Some(value);
        }
        let native_id = crate::vm::interpreter::opcodes::types::builtin_handle_native_method_id(
            self.pinned_handles,
            target,
            key,
        )?;
        let method = Object::new_bound_native(target, native_id);
        let method_ptr = self.gc.lock().allocate(method);
        Some(unsafe { Value::from_ptr(std::ptr::NonNull::new(method_ptr.as_ptr()).unwrap()) })
    }

    fn exotic_adapter_kind(&self, target: Value) -> Option<JsExoticAdapterKind> {
        if checked_object_ptr(target)
            .and_then(|obj_ptr| unsafe { obj_ptr.as_ref() }.arguments.as_deref())
            .is_some()
        {
            return Some(JsExoticAdapterKind::Arguments);
        }
        if checked_array_ptr(target).is_some() {
            return Some(JsExoticAdapterKind::Array);
        }
        if checked_string_ptr(target).is_some() {
            return Some(JsExoticAdapterKind::String);
        }
        if self.is_typed_array_like_value(target) {
            return Some(JsExoticAdapterKind::TypedArray);
        }
        None
    }

    fn string_exotic_length(&self, target: Value) -> Option<usize> {
        let string_ptr = checked_string_ptr(target)?;
        Some(unsafe { &*string_ptr.as_ptr() }.data.chars().count())
    }

    fn string_exotic_index_value(&self, target: Value, index: usize) -> Option<Value> {
        let string_ptr = checked_string_ptr(target)?;
        let data = &unsafe { &*string_ptr.as_ptr() }.data;
        let ch = data.chars().nth(index)?;
        Some(self.alloc_string_value(ch.to_string()))
    }

    fn exotic_own_property_shape(
        &self,
        kind: JsExoticAdapterKind,
        target: Value,
        key: &str,
    ) -> Option<JsOwnPropertyShape> {
        match kind {
            JsExoticAdapterKind::Arguments => {
                let obj_ptr = checked_object_ptr(target)?;
                let obj = unsafe { &*obj_ptr.as_ptr() };
                let arguments = obj.arguments.as_deref()?;
                if key == "length" {
                    let length = arguments.length.as_ref()?;
                    return Some(JsOwnPropertyShape::data(
                        JsOwnPropertySource::ArgumentsExotic,
                        length.writable,
                        length.configurable,
                        length.enumerable,
                    ));
                }
                if key == "callee" {
                    if arguments.strict_poison {
                        return Some(JsOwnPropertyShape::poisoned_accessor(
                            JsOwnPropertySource::ArgumentsExotic,
                            false,
                            false,
                        ));
                    }
                    let callee = arguments.callee.as_ref()?;
                    return Some(JsOwnPropertyShape::data(
                        JsOwnPropertySource::ArgumentsExotic,
                        callee.writable,
                        callee.configurable,
                        callee.enumerable,
                    ));
                }
                if key == "caller" {
                    return None;
                }
                let index = parse_js_array_index_name(key)?;
                let property = arguments.indexed.get(index)?;
                (!property.deleted).then_some(JsOwnPropertyShape::data(
                    JsOwnPropertySource::ArgumentsExotic,
                    property.writable,
                    property.configurable,
                    property.enumerable,
                ))
            }
            JsExoticAdapterKind::Array => {
                let array_ptr = checked_array_ptr(target)?;
                let array = unsafe { &*array_ptr.as_ptr() };
                if let Some(descriptor) = self.metadata_descriptor_property(target, key) {
                    if let Some(property) =
                        self.ordinary_own_property_from_descriptor_value(descriptor)
                    {
                        return Some(self.shape_from_ordinary_property(
                            JsOwnPropertySource::ArrayExotic,
                            property,
                        ));
                    }
                }
                if key == "length" {
                    let (writable, configurable, enumerable) = self
                        .property_attributes_from_descriptor_metadata(
                            target,
                            key,
                            (true, false, false),
                        );
                    let _ = array;
                    return Some(JsOwnPropertyShape::data(
                        JsOwnPropertySource::ArrayExotic,
                        writable,
                        configurable,
                        enumerable,
                    ));
                }
                if let Some(index) = parse_js_array_index_name(key) {
                    if array.get(index).is_some() {
                        let (writable, configurable, enumerable) = self
                            .property_attributes_from_descriptor_metadata(
                                target,
                                key,
                                (true, true, true),
                            );
                        return Some(JsOwnPropertyShape::data(
                            JsOwnPropertySource::ArrayExotic,
                            writable,
                            configurable,
                            enumerable,
                        ));
                    }
                }
                if self.metadata_data_property_value(target, key).is_some() {
                    let (writable, configurable, enumerable) = self
                        .property_attributes_from_descriptor_metadata(
                            target,
                            key,
                            (true, true, true),
                        );
                    return Some(JsOwnPropertyShape::data(
                        JsOwnPropertySource::Metadata,
                        writable,
                        configurable,
                        enumerable,
                    ));
                }
                None
            }
            JsExoticAdapterKind::String => {
                if key == "length" {
                    let (writable, configurable, enumerable) = self
                        .property_attributes_from_descriptor_metadata(
                            target,
                            key,
                            (false, false, false),
                        );
                    return Some(JsOwnPropertyShape::data(
                        JsOwnPropertySource::StringExotic,
                        writable,
                        configurable,
                        enumerable,
                    ));
                }
                if let Some(index) = parse_js_array_index_name(key) {
                    if index < self.string_exotic_length(target)? {
                        let (writable, configurable, enumerable) = self
                            .property_attributes_from_descriptor_metadata(
                                target,
                                key,
                                (false, false, true),
                            );
                        return Some(JsOwnPropertyShape::data(
                            JsOwnPropertySource::StringExotic,
                            writable,
                            configurable,
                            enumerable,
                        ));
                    }
                }
                None
            }
            JsExoticAdapterKind::TypedArray => {
                if key == "length"
                    || matches!(key, "buffer" | "byteOffset" | "byteLength")
                    || self.typed_array_index_property_flags(target, key).is_some()
                {
                    let default = if self.typed_array_index_property_flags(target, key).is_some() {
                        (true, true, true)
                    } else {
                        (false, false, false)
                    };
                    let (writable, configurable, enumerable) =
                        self.property_attributes_from_descriptor_metadata(target, key, default);
                    return Some(JsOwnPropertyShape::data(
                        JsOwnPropertySource::TypedArrayExotic,
                        writable,
                        configurable,
                        enumerable,
                    ));
                }
                None
            }
        }
    }

    fn exotic_own_property_record_with_context(
        &mut self,
        kind: JsExoticAdapterKind,
        target: Value,
        key: &str,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Option<JsOwnPropertyRecord>, VmError> {
        let Some(shape) = self.exotic_own_property_shape(kind, target, key) else {
            return Ok(None);
        };
        let record = match (kind, shape.kind) {
            (JsExoticAdapterKind::Arguments, JsOwnPropertyKind::PoisonedAccessor) => {
                JsOwnPropertyRecord::accessor(shape, None, None)
            }
            (JsExoticAdapterKind::Arguments, JsOwnPropertyKind::Data) => JsOwnPropertyRecord::data(
                shape,
                self.arguments_exotic_get(target, key)?
                    .unwrap_or(Value::undefined()),
            ),
            (JsExoticAdapterKind::Array, JsOwnPropertyKind::Data) => {
                if let Some(descriptor) = self.metadata_descriptor_property(target, key) {
                    if let Some(property) = self.array_exotic_current_own_property(target, key) {
                        match property {
                            OrdinaryOwnProperty::Data { value, .. } => {
                                JsOwnPropertyRecord::data(shape, value)
                            }
                            OrdinaryOwnProperty::Accessor { get, set, .. } => {
                                return Ok(Some(JsOwnPropertyRecord::accessor(
                                    shape,
                                    Some(get),
                                    Some(set),
                                )));
                            }
                        }
                    } else {
                        let _ = descriptor;
                        JsOwnPropertyRecord::data(shape, Value::undefined())
                    }
                } else {
                    let value = if let Some(array_ptr) = checked_array_ptr(target) {
                        let array = unsafe { &*array_ptr.as_ptr() };
                        if key == "length" {
                            if array.len() <= i32::MAX as usize {
                                Value::i32(array.len() as i32)
                            } else {
                                Value::f64(array.len() as f64)
                            }
                        } else if let Some(index) = parse_js_array_index_name(key) {
                            array.get(index).unwrap_or(Value::undefined())
                        } else {
                            self.metadata_data_property_value(target, key)
                                .unwrap_or(Value::undefined())
                        }
                    } else {
                        Value::undefined()
                    };
                    JsOwnPropertyRecord::data(shape, value)
                }
            }
            (JsExoticAdapterKind::Array, JsOwnPropertyKind::Accessor) => {
                let Some(property) = self.array_exotic_current_own_property(target, key) else {
                    return Ok(None);
                };
                let OrdinaryOwnProperty::Accessor { get, set, .. } = property else {
                    return Ok(None);
                };
                JsOwnPropertyRecord::accessor(shape, Some(get), Some(set))
            }
            (JsExoticAdapterKind::String, JsOwnPropertyKind::Data) => {
                let value = if key == "length" {
                    let len = self.string_exotic_length(target).unwrap_or(0);
                    if len <= i32::MAX as usize {
                        Value::i32(len as i32)
                    } else {
                        Value::f64(len as f64)
                    }
                } else if let Some(index) = parse_js_array_index_name(key) {
                    self.string_exotic_index_value(target, index)
                        .unwrap_or(Value::undefined())
                } else {
                    Value::undefined()
                };
                JsOwnPropertyRecord::data(shape, value)
            }
            (JsExoticAdapterKind::TypedArray, JsOwnPropertyKind::Data) => {
                JsOwnPropertyRecord::data(
                    shape,
                    self.typed_array_own_property_value_with_context(
                        target,
                        key,
                        caller_task,
                        caller_module,
                    )?
                    .unwrap_or(Value::undefined()),
                )
            }
            _ => return Ok(None),
        };
        Ok(Some(record))
    }

    fn exotic_set_property_on_receiver_with_context(
        &mut self,
        kind: JsExoticAdapterKind,
        receiver: Value,
        key: &str,
        value: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Option<bool>, VmError> {
        match kind {
            JsExoticAdapterKind::Arguments => self.arguments_exotic_set(receiver, key, value),
            JsExoticAdapterKind::Array => {
                let Some(array_ptr) = checked_array_ptr(receiver) else {
                    return Ok(None);
                };
                if key == "length" {
                    return self
                        .set_array_length_via_array_set_length(
                            receiver,
                            value,
                            caller_task,
                            caller_module,
                        )
                        .map(Some);
                }
                if let Some(index) = parse_js_array_index_name(key) {
                    let array = unsafe { &mut *array_ptr.as_ptr() };
                    if !self.is_js_value_extensible(receiver) && array.get(index).is_none() {
                        return Ok(Some(false));
                    }
                    array.set(index, value).map_err(VmError::RuntimeError)?;
                    return Ok(Some(true));
                }
                Ok(None)
            }
            JsExoticAdapterKind::String => Ok(None),
            JsExoticAdapterKind::TypedArray => {
                let Some(index) = parse_js_array_index_name(key) else {
                    return Ok(None);
                };
                self.typed_array_set_index_value_with_context(
                    receiver,
                    index,
                    value,
                    caller_task,
                    caller_module,
                )
            }
        }
    }

    fn exotic_delete_own_property(
        &mut self,
        kind: JsExoticAdapterKind,
        target: Value,
        key: &str,
    ) -> Result<Option<bool>, VmError> {
        match kind {
            JsExoticAdapterKind::Arguments => Ok(self.arguments_exotic_delete(target, key)),
            JsExoticAdapterKind::Array => {
                let Some(array_ptr) = checked_array_ptr(target) else {
                    return Ok(None);
                };
                if key == "length" {
                    return Ok(Some(false));
                }
                let Some(index) = parse_js_array_index_name(key) else {
                    return Ok(None);
                };
                let array = unsafe { &mut *array_ptr.as_ptr() };
                let _ = array.delete_index(index);
                let mut metadata = self.metadata.lock();
                let _ = metadata.delete_metadata_property(
                    NON_OBJECT_DYNAMIC_VALUE_METADATA_KEY,
                    target,
                    key,
                );
                let _ = metadata.delete_metadata_property(
                    NON_OBJECT_DESCRIPTOR_METADATA_KEY,
                    target,
                    key,
                );
                Ok(Some(true))
            }
            JsExoticAdapterKind::String => Ok(None),
            JsExoticAdapterKind::TypedArray => {
                if self.typed_array_index_property_flags(target, key).is_some() {
                    return Ok(Some(false));
                }
                Ok(None)
            }
        }
    }

    fn resolve_own_property_shape_on_target(
        &self,
        target: Value,
        key: &str,
    ) -> Option<JsOwnPropertyShape> {
        if let Some(kind) = self.exotic_adapter_kind(target) {
            if let Some(shape) = self.exotic_own_property_shape(kind, target, key) {
                return Some(shape);
            }
        }

        if let Some(descriptor) = self.builtin_object_constant_descriptor(target, key) {
            return Some(JsOwnPropertyShape::data(
                JsOwnPropertySource::BuiltinObjectConstant,
                descriptor.writable,
                descriptor.configurable,
                descriptor.enumerable,
            ));
        }

        if let Some(property) = self.ordinary_own_property(target, key) {
            return Some(match property {
                OrdinaryOwnProperty::Data {
                    writable,
                    configurable,
                    enumerable,
                    ..
                } => JsOwnPropertyShape::data(
                    JsOwnPropertySource::Ordinary,
                    writable,
                    configurable,
                    enumerable,
                ),
                OrdinaryOwnProperty::Accessor {
                    configurable,
                    enumerable,
                    ..
                } => JsOwnPropertyShape::accessor(
                    JsOwnPropertySource::Ordinary,
                    configurable,
                    enumerable,
                ),
            });
        }

        if let Some(descriptor) = self.metadata_descriptor_property(target, key) {
            if let Some(property) = self.ordinary_own_property_from_descriptor_value(descriptor) {
                return Some(
                    self.shape_from_ordinary_property(JsOwnPropertySource::Metadata, property),
                );
            }
        }

        if self.metadata_data_property_value(target, key).is_some() {
            let (writable, configurable, enumerable) =
                self.property_attributes_from_descriptor_metadata(target, key, (true, true, true));
            return Some(JsOwnPropertyShape::data(
                JsOwnPropertySource::Metadata,
                writable,
                configurable,
                enumerable,
            ));
        }

        if let Some((writable, configurable, enumerable)) =
            self.ambient_builtin_global_property_flags(target, key)
        {
            return Some(JsOwnPropertyShape::data(
                JsOwnPropertySource::BuiltinGlobal,
                writable,
                configurable,
                enumerable,
            ));
        }

        if self.constructor_static_field_value(target, key).is_some() {
            return Some(JsOwnPropertyShape::data(
                JsOwnPropertySource::ConstructorStaticField,
                true,
                true,
                true,
            ));
        }

        if self
            .constructor_static_accessor_values(target, key)
            .is_some_and(|(get, set)| get.is_some() || set.is_some())
        {
            return Some(JsOwnPropertyShape::accessor(
                JsOwnPropertySource::ConstructorStaticAccessor,
                true,
                false,
            ));
        }

        if self.has_constructor_static_method(target, key) {
            return Some(JsOwnPropertyShape::data(
                JsOwnPropertySource::ConstructorStaticMethod,
                true,
                true,
                false,
            ));
        }

        let callable_get = self.callable_virtual_accessor_value(target, key, "get");
        let callable_set = self.callable_virtual_accessor_value(target, key, "set");
        if callable_get.is_some() || callable_set.is_some() {
            let (_, configurable, enumerable) = self
                .callable_virtual_property_descriptor(target, key)
                .unwrap_or((false, true, false));
            return Some(JsOwnPropertyShape::accessor(
                JsOwnPropertySource::CallableVirtual,
                configurable,
                enumerable,
            ));
        }
        if let Some((writable, configurable, enumerable)) =
            self.callable_virtual_property_descriptor(target, key)
        {
            return Some(JsOwnPropertyShape::data(
                JsOwnPropertySource::CallableVirtual,
                writable,
                configurable,
                enumerable,
            ));
        }

        if self.has_class_vtable_method(target, key) {
            return Some(JsOwnPropertyShape::data(
                JsOwnPropertySource::NominalMethod,
                true,
                true,
                false,
            ));
        }

        if self.task_promise_method_value(target, key).is_some() {
            return Some(JsOwnPropertyShape::data(
                JsOwnPropertySource::BuiltinNativeMethod,
                true,
                true,
                false,
            ));
        }

        if crate::vm::interpreter::opcodes::types::builtin_handle_native_method_id(
            self.pinned_handles,
            target,
            key,
        )
        .is_some()
        {
            return Some(JsOwnPropertyShape::data(
                JsOwnPropertySource::BuiltinNativeMethod,
                true,
                true,
                false,
            ));
        }

        None
    }

    fn resolve_own_property_shape(&self, target: Value, key: &str) -> Option<JsOwnPropertyShape> {
        self.resolve_own_property_shape_on_target(target, key)
            .or_else(|| {
                let public_target = self.public_property_target(target);
                (public_target.raw() != target.raw())
                    .then(|| self.resolve_own_property_shape_on_target(public_target, key))
                    .flatten()
            })
    }

    fn resolve_own_property_record_on_target_with_context(
        &mut self,
        target: Value,
        key: &str,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Option<JsOwnPropertyRecord>, VmError> {
        if let Some(kind) = self.exotic_adapter_kind(target) {
            if let Some(record) = self.exotic_own_property_record_with_context(
                kind,
                target,
                key,
                caller_task,
                caller_module,
            )? {
                return Ok(Some(record));
            }
        }

        if let Some(descriptor) = self.builtin_object_constant_descriptor(target, key) {
            return Ok(Some(JsOwnPropertyRecord::data(
                JsOwnPropertyShape::data(
                    JsOwnPropertySource::BuiltinObjectConstant,
                    descriptor.writable,
                    descriptor.configurable,
                    descriptor.enumerable,
                ),
                descriptor.value,
            )));
        }

        let Some(shape) = self.resolve_own_property_shape_on_target(target, key) else {
            return Ok(None);
        };

        let record = match (shape.source, shape.kind) {
            (JsOwnPropertySource::Ordinary, JsOwnPropertyKind::Data) => {
                let value = match self.ordinary_own_property(target, key) {
                    Some(OrdinaryOwnProperty::Data { value, .. }) => value,
                    _ => Value::undefined(),
                };
                JsOwnPropertyRecord::data(shape, value)
            }
            (JsOwnPropertySource::Ordinary, JsOwnPropertyKind::Accessor) => {
                let (getter, setter) = match self.ordinary_own_property(target, key) {
                    Some(OrdinaryOwnProperty::Accessor { get, set, .. }) => (
                        (!get.is_undefined()).then_some(get),
                        (!set.is_undefined()).then_some(set),
                    ),
                    _ => (None, None),
                };
                JsOwnPropertyRecord::accessor(shape, getter, setter)
            }
            (JsOwnPropertySource::Metadata, JsOwnPropertyKind::Data) => JsOwnPropertyRecord::data(
                shape,
                self.metadata_data_property_value(target, key)
                    .unwrap_or(Value::undefined()),
            ),
            (JsOwnPropertySource::Metadata, JsOwnPropertyKind::Accessor) => {
                let Some(descriptor) = self.metadata_descriptor_property(target, key) else {
                    return Ok(None);
                };
                let Some(property) = self.ordinary_own_property_from_descriptor_value(descriptor)
                else {
                    return Ok(None);
                };
                match property {
                    OrdinaryOwnProperty::Accessor { get, set, .. } => {
                        JsOwnPropertyRecord::accessor(
                            shape,
                            (!get.is_undefined()).then_some(get),
                            (!set.is_undefined()).then_some(set),
                        )
                    }
                    OrdinaryOwnProperty::Data { value, .. } => {
                        JsOwnPropertyRecord::data(shape, value)
                    }
                }
            }
            (JsOwnPropertySource::BuiltinGlobal, JsOwnPropertyKind::Data) => {
                JsOwnPropertyRecord::data(
                    shape,
                    self.builtin_global_property_value(target, key)
                        .unwrap_or(Value::undefined()),
                )
            }
            (JsOwnPropertySource::BuiltinObjectConstant, JsOwnPropertyKind::Data) => {
                JsOwnPropertyRecord::data(
                    shape,
                    self.builtin_object_constant_descriptor(target, key)
                        .map(|descriptor| descriptor.value)
                        .unwrap_or(Value::undefined()),
                )
            }
            (JsOwnPropertySource::CallableVirtual, JsOwnPropertyKind::Data) => {
                JsOwnPropertyRecord::data(
                    shape,
                    self.callable_virtual_property_value(target, key)
                        .unwrap_or(Value::undefined()),
                )
            }
            (JsOwnPropertySource::CallableVirtual, JsOwnPropertyKind::Accessor) => {
                JsOwnPropertyRecord::accessor(
                    shape,
                    self.callable_virtual_accessor_value(target, key, "get"),
                    self.callable_virtual_accessor_value(target, key, "set"),
                )
            }
            (JsOwnPropertySource::NominalMethod, JsOwnPropertyKind::Data) => {
                JsOwnPropertyRecord::data(
                    shape,
                    self.own_nominal_method_value(target, key)
                        .unwrap_or(Value::undefined()),
                )
            }
            (JsOwnPropertySource::BuiltinNativeMethod, JsOwnPropertyKind::Data) => {
                JsOwnPropertyRecord::data(
                    shape,
                    self.own_builtin_native_method_value(target, key)
                        .unwrap_or(Value::undefined()),
                )
            }
            (JsOwnPropertySource::ConstructorStaticField, JsOwnPropertyKind::Data) => {
                JsOwnPropertyRecord::data(
                    shape,
                    self.constructor_static_field_value(target, key)
                        .unwrap_or(Value::undefined()),
                )
            }
            (JsOwnPropertySource::ConstructorStaticAccessor, JsOwnPropertyKind::Accessor) => {
                let (getter, setter) = self
                    .constructor_static_accessor_values(target, key)
                    .unwrap_or((None, None));
                JsOwnPropertyRecord::accessor(shape, getter, setter)
            }
            (JsOwnPropertySource::ConstructorStaticMethod, JsOwnPropertyKind::Data) => {
                JsOwnPropertyRecord::data(
                    shape,
                    self.materialize_constructor_static_method(target, key)
                        .unwrap_or(Value::undefined()),
                )
            }
            _ => return Ok(None),
        };

        Ok(Some(record))
    }

    fn resolve_own_property_record_with_context(
        &mut self,
        target: Value,
        key: &str,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Option<JsOwnPropertyRecord>, VmError> {
        if let Some(record) = self.resolve_own_property_record_on_target_with_context(
            target,
            key,
            caller_task,
            caller_module,
        )? {
            return Ok(Some(record));
        }
        let public_target = self.public_property_target(target);
        if public_target.raw() != target.raw() {
            return self.resolve_own_property_record_on_target_with_context(
                public_target,
                key,
                caller_task,
                caller_module,
            );
        }
        Ok(None)
    }

    fn resolve_property_record_on_receiver_with_context(
        &mut self,
        target: Value,
        key: &str,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Option<JsResolvedPropertyRecord>, VmError> {
        let mut current = Some(target);
        let mut seen = vec![target.raw()];

        while let Some(candidate) = current {
            if let Some(record) = self.resolve_own_property_record_with_context(
                candidate,
                key,
                caller_task,
                caller_module,
            )? {
                return Ok(Some(JsResolvedPropertyRecord {
                    owner: candidate,
                    record,
                }));
            }

            let Some(prototype) = self.prototype_of_value(candidate) else {
                break;
            };
            if prototype.raw() == candidate.raw() || seen.contains(&prototype.raw()) {
                break;
            }
            seen.push(prototype.raw());
            current = Some(prototype);
        }

        Ok(None)
    }

    fn read_resolved_property_value_with_context(
        &mut self,
        key: &str,
        record: JsOwnPropertyRecord,
        receiver: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Value, VmError> {
        match record.shape.kind {
            JsOwnPropertyKind::PoisonedAccessor => Err(VmError::TypeError(format!(
                "'{}' is not accessible on strict mode arguments objects",
                key
            ))),
            JsOwnPropertyKind::Accessor => {
                if let Some(getter) = record.getter {
                    return self.invoke_callable_sync_with_this(
                        getter,
                        Some(receiver),
                        &[],
                        caller_task,
                        caller_module,
                    );
                }
                Ok(Value::undefined())
            }
            JsOwnPropertyKind::Data => Ok(record.value.unwrap_or(Value::undefined())),
        }
    }

    pub(in crate::vm::interpreter::opcodes) fn own_js_property_flags(
        &self,
        target: Value,
        key: &str,
    ) -> Option<(bool, bool, bool)> {
        self.resolve_own_property_shape(target, key)
            .map(|shape| (shape.writable, shape.configurable, shape.enumerable))
    }

    fn get_field_value_on_target_by_name(&self, obj_val: Value, field_name: &str) -> Option<Value> {
        if let Some(value) = self.get_own_js_property_value_by_name(obj_val, field_name) {
            return Some(value);
        }

        if let Some(value) = self.materialize_constructor_static_method(obj_val, field_name) {
            return Some(value);
        }

        if let Some(obj_ptr) = checked_object_ptr(obj_val) {
            let obj = unsafe { &*obj_ptr.as_ptr() };
            if let Some(nominal_type_id) = obj.nominal_type_id_usize() {
                if self.nominal_instance_uses_runtime_publication(obj_val) {
                    return None;
                }
                if let Some(method_slot) =
                    self.nominal_method_slot_by_name(nominal_type_id, field_name)
                {
                    if let Ok(bound) = self.bound_method_value_for_slot(obj_val, method_slot) {
                        return Some(bound);
                    }
                }
            }
        }

        None
    }

    pub(in crate::vm::interpreter) fn get_field_value_by_name(
        &self,
        obj_val: Value,
        field_name: &str,
    ) -> Option<Value> {
        let mut current = Some(obj_val);
        let mut seen = vec![obj_val.raw()];

        while let Some(target) = current {
            if let Some(value) = self.get_field_value_on_target_by_name(target, field_name) {
                return Some(value);
            }

            let Some(prototype) = self.prototype_of_value(target) else {
                break;
            };
            if prototype.raw() == target.raw() || seen.contains(&prototype.raw()) {
                break;
            }
            seen.push(prototype.raw());
            current = Some(prototype);
        }

        None
    }

    fn has_class_vtable_method(&self, target: Value, key: &str) -> bool {
        let Some(obj_ptr) = checked_object_ptr(target) else {
            return false;
        };
        let obj = unsafe { &*obj_ptr.as_ptr() };
        obj.nominal_type_id_usize().is_some_and(|ntid| {
            let classes = self.classes.read();
            if let Some(class) = classes.get_class(ntid) {
                if class.runtime_instance_publication {
                    return false;
                }
                let has_own_accessor = class.prototype_members.iter().any(|member| {
                    member.name == key
                        && matches!(
                            member.kind,
                            crate::vm::object::PrototypeMemberKind::Getter
                                | crate::vm::object::PrototypeMemberKind::Setter
                        )
                });
                if has_own_accessor {
                    return false;
                }
            }
            drop(classes);
            let class_metadata = self.class_metadata.read();
            if class_metadata
                .get(ntid)
                .and_then(|m| m.get_method_index(key))
                .is_some()
            {
                return true;
            }
            drop(class_metadata);
            let classes = self.classes.read();
            classes.get_class(ntid).is_some_and(|class| {
                class.module.as_ref().is_some_and(|module| {
                    module.classes.iter().any(|cd| {
                        cd.name == class.name
                            && cd.methods.iter().any(|m| {
                                let plain = m.name.rsplit("::").next().unwrap_or(&m.name);
                                matches!(m.kind, crate::compiler::bytecode::MethodKind::Normal)
                                    && (m.name == key || plain == key)
                            })
                    })
                })
            })
        })
    }

    pub(in crate::vm::interpreter) fn has_own_property_via_js_semantics(
        &self,
        target: Value,
        key: &str,
    ) -> bool {
        self.resolve_own_property_shape(target, key).is_some()
    }

    fn has_explicit_own_property_via_js_semantics(&self, target: Value, key: &str) -> bool {
        self.resolve_own_property_shape(target, key)
            .is_some_and(|shape| shape.source != JsOwnPropertySource::CallableVirtual)
    }

    pub(in crate::vm::interpreter) fn has_property_via_js_semantics(
        &self,
        target: Value,
        key: &str,
    ) -> bool {
        let mut current = Some(target);
        let mut seen = vec![target.raw()];

        while let Some(candidate) = current {
            if self.resolve_own_property_shape(candidate, key).is_some() {
                return true;
            }

            let Some(prototype) = self.prototype_of_value(candidate) else {
                break;
            };
            if prototype.raw() == candidate.raw() || seen.contains(&prototype.raw()) {
                break;
            }
            seen.push(prototype.raw());
            current = Some(prototype);
        }

        false
    }

    pub(in crate::vm::interpreter) fn has_property_via_js_semantics_with_context(
        &mut self,
        target: Value,
        key: &str,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<bool, VmError> {
        if let Some(result) =
            self.try_proxy_has_property_with_invariants(target, key, caller_task, caller_module)?
        {
            return Ok(result);
        }
        Ok(self.has_property_via_js_semantics(target, key))
    }

    pub(in crate::vm::interpreter) fn get_own_property_value_via_js_semantics_with_context(
        &mut self,
        target: Value,
        key: &str,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Option<Value>, VmError> {
        self.get_own_property_value_on_receiver_via_js_semantics_with_context(
            target,
            key,
            target,
            caller_task,
            caller_module,
        )
    }

    pub(in crate::vm::interpreter) fn get_own_property_value_on_receiver_via_js_semantics_with_context(
        &mut self,
        target: Value,
        key: &str,
        receiver: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Option<Value>, VmError> {
        if let Some(value) =
            self.try_proxy_like_get_property(target, key, caller_task, caller_module)?
        {
            return Ok(Some(value));
        }

        let Some(record) =
            self.resolve_own_property_record_with_context(target, key, caller_task, caller_module)?
        else {
            return Ok(None);
        };
        let value = self.read_resolved_property_value_with_context(
            key,
            record,
            receiver,
            caller_task,
            caller_module,
        )?;
        if key == "prototype" {
            self.ensure_prototype_nominal_type_id(target, value);
        }
        Ok(Some(value))
    }

    pub(in crate::vm::interpreter) fn get_property_value_via_js_semantics_with_context(
        &mut self,
        target: Value,
        key: &str,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Option<Value>, VmError> {
        self.get_property_value_on_receiver_via_js_semantics_with_context(
            target,
            key,
            target,
            caller_task,
            caller_module,
        )
    }

    pub(in crate::vm::interpreter) fn get_property_value_on_receiver_via_js_semantics_with_context(
        &mut self,
        target: Value,
        key: &str,
        receiver: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Option<Value>, VmError> {
        if let Some(value) =
            self.try_proxy_get_property_with_invariants(target, key, caller_task, caller_module)?
        {
            return Ok(Some(value));
        }
        let Some(resolved) = self.resolve_property_record_on_receiver_with_context(
            target,
            key,
            caller_task,
            caller_module,
        )?
        else {
            return Ok(None);
        };
        let value = self.read_resolved_property_value_with_context(
            key,
            resolved.record,
            receiver,
            caller_task,
            caller_module,
        )?;
        if key == "prototype" {
            self.ensure_prototype_nominal_type_id(resolved.owner, value);
        }
        Ok(Some(value))
    }

    fn descriptor_flag(&self, descriptor: Value, field_name: &str, default: bool) -> bool {
        if !self.descriptor_field_present(descriptor, field_name) {
            return default;
        }
        let Some(value) = self.get_field_value_by_name(descriptor, field_name) else {
            return default;
        };
        if let Some(b) = value.as_bool() {
            b
        } else if let Some(i) = value.as_i32() {
            i != 0
        } else {
            default
        }
    }

    fn set_internal_descriptor_field(
        &self,
        descriptor: Value,
        field_name: &str,
        value: Value,
    ) -> Result<(), VmError> {
        let Some(descriptor_ptr) = (unsafe { descriptor.as_ptr::<Object>() }) else {
            return Err(VmError::RuntimeError(
                "Failed to access property descriptor object".to_string(),
            ));
        };
        let descriptor_obj = unsafe { &mut *descriptor_ptr.as_ptr() };
        if let Some(field_index) = self.get_field_index_for_value(descriptor, field_name) {
            descriptor_obj
                .set_field(field_index, value)
                .map_err(VmError::RuntimeError)?;
        }
        self.set_descriptor_field_present(descriptor, field_name, true);
        Ok(())
    }

    fn set_prototype_of_value(&self, target: Value, prototype: Value) -> bool {
        if !self.js_value_supports_extensibility(target) {
            return false;
        }
        if !prototype.is_null() && !self.is_js_object_value(prototype) {
            return false;
        }
        let current = self.prototype_of_value(target).unwrap_or(Value::null());
        if current.raw() == prototype.raw() {
            return true;
        }
        if !self.is_js_value_extensible(target) {
            return false;
        }

        let mut cursor = if prototype.is_null() {
            None
        } else {
            Some(prototype)
        };
        let mut seen = vec![target.raw()];
        while let Some(candidate) = cursor {
            if candidate.raw() == target.raw() || seen.contains(&candidate.raw()) {
                return false;
            }
            seen.push(candidate.raw());
            cursor = self
                .prototype_of_value(candidate)
                .filter(|value| !value.is_null());
        }

        self.set_explicit_object_prototype(target, prototype);
        true
    }

    fn normalize_property_descriptor_with_context(
        &mut self,
        descriptor: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Value, VmError> {
        if self.is_descriptor_object(descriptor) {
            return Ok(descriptor);
        }
        if !descriptor.is_ptr() {
            return Err(VmError::TypeError(
                "Object property descriptor must be an object".to_string(),
            ));
        }

        let mut record = JsPropertyDescriptorRecord::default();

        for field_name in [
            "enumerable",
            "configurable",
            "value",
            "writable",
            "get",
            "set",
        ] {
            if !self.has_property_via_js_semantics(descriptor, field_name) {
                continue;
            }

            let value = self
                .get_property_value_via_js_semantics_with_context(
                    descriptor,
                    field_name,
                    caller_task,
                    caller_module,
                )?
                .unwrap_or(Value::undefined());

            match field_name {
                "enumerable" => {
                    record.has_enumerable = true;
                    record.enumerable = value.is_truthy();
                }
                "configurable" => {
                    record.has_configurable = true;
                    record.configurable = value.is_truthy();
                }
                "value" => {
                    record.has_value = true;
                    record.value = value;
                }
                "writable" => {
                    record.has_writable = true;
                    record.writable = value.is_truthy();
                }
                "get" => {
                    if !value.is_undefined() && !Self::is_callable_value(value) {
                        return Err(VmError::TypeError(
                            "Getter for property descriptor must be callable".to_string(),
                        ));
                    }
                    record.has_get = true;
                    record.get = value;
                }
                "set" => {
                    if !value.is_undefined() && !Self::is_callable_value(value) {
                        return Err(VmError::TypeError(
                            "Setter for property descriptor must be callable".to_string(),
                        ));
                    }
                    record.has_set = true;
                    record.set = value;
                }
                _ => {}
            }
        }

        if (record.has_get || record.has_set) && (record.has_value || record.has_writable) {
            return Err(VmError::TypeError(
                "Invalid property descriptor: cannot mix accessors and value".to_string(),
            ));
        }

        let normalized = self.alloc_object_descriptor()?;
        if record.has_value {
            self.set_internal_descriptor_field(normalized, "value", record.value)?;
        }
        if record.has_writable {
            self.set_internal_descriptor_field(
                normalized,
                "writable",
                Value::bool(record.writable),
            )?;
        }
        if record.has_configurable {
            self.set_internal_descriptor_field(
                normalized,
                "configurable",
                Value::bool(record.configurable),
            )?;
        }
        if record.has_enumerable {
            self.set_internal_descriptor_field(
                normalized,
                "enumerable",
                Value::bool(record.enumerable),
            )?;
        }
        if record.has_get {
            self.set_internal_descriptor_field(normalized, "get", record.get)?;
        }
        if record.has_set {
            self.set_internal_descriptor_field(normalized, "set", record.set)?;
        }

        Ok(normalized)
    }

    fn set_descriptor_metadata(&self, target: Value, key: &str, descriptor: Value) {
        // Write to property kernel only (single source of truth)
        let Some(obj_ptr) = checked_object_ptr(target) else {
            self.metadata.lock().define_metadata_property(
                NON_OBJECT_DESCRIPTOR_METADATA_KEY.to_string(),
                descriptor,
                target,
                key.to_string(),
            );
            return;
        };
        let obj = unsafe { &mut *obj_ptr.as_ptr() };
        let key_id = self.intern_prop_key(key);

        // Extract descriptor fields from the descriptor Value (a JS object)
        let desc_value = self.get_field_value_by_name(descriptor, "value");
        let desc_get = self.get_field_value_by_name(descriptor, "get");
        let desc_set = self.get_field_value_by_name(descriptor, "set");
        let desc_writable = self
            .get_field_value_by_name(descriptor, "writable")
            .and_then(|v| v.as_bool());
        let desc_enumerable = self
            .get_field_value_by_name(descriptor, "enumerable")
            .and_then(|v| v.as_bool());
        let desc_configurable = self
            .get_field_value_by_name(descriptor, "configurable")
            .and_then(|v| v.as_bool());

        let has_accessor = desc_get.is_some() || desc_set.is_some();

        let prop = if has_accessor {
            DynProp::accessor(
                desc_get.unwrap_or(Value::undefined()),
                desc_set.unwrap_or(Value::undefined()),
                desc_enumerable.unwrap_or(false),
                desc_configurable.unwrap_or(false),
            )
        } else {
            DynProp::data_with_attrs(
                desc_value.unwrap_or(Value::undefined()),
                desc_writable.unwrap_or(true),
                desc_enumerable.unwrap_or(true),
                desc_configurable.unwrap_or(true),
            )
        };

        obj.ensure_dyn_props().insert(key_id, prop);
    }

    fn metadata_descriptor_property(&self, target: Value, key: &str) -> Option<Value> {
        let metadata = self.metadata.lock();
        metadata.get_metadata_property(NON_OBJECT_DESCRIPTOR_METADATA_KEY, target, key)
    }

    fn delete_metadata_descriptor_property(&self, target: Value, key: &str) {
        let mut metadata = self.metadata.lock();
        let _ = metadata.delete_metadata_property(NON_OBJECT_DESCRIPTOR_METADATA_KEY, target, key);
    }

    fn metadata_descriptor_property_names(&self, target: Value) -> Vec<String> {
        self.metadata
            .lock()
            .get_property_keys_for_metadata(target, NON_OBJECT_DESCRIPTOR_METADATA_KEY)
    }

    pub(in crate::vm::interpreter) fn define_data_property_on_target(
        &self,
        target: Value,
        key: &str,
        value: Value,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    ) -> Result<(), VmError> {
        let debug_array_prop = std::env::var("RAYA_DEBUG_ARRAY_PROP").is_ok();
        if debug_array_prop {
            eprintln!(
                "[defineData] target={:#x} is_object={} key={} value={:#x} attrs=({}, {}, {})",
                target.raw(),
                checked_object_ptr(target).is_some(),
                key,
                value.raw(),
                writable,
                enumerable,
                configurable
            );
        }
        let descriptor = self.alloc_object_descriptor()?;
        let Some(descriptor_ptr) = (unsafe { descriptor.as_ptr::<Object>() }) else {
            return Err(VmError::RuntimeError(
                "Failed to allocate property descriptor object".to_string(),
            ));
        };
        let descriptor_obj = unsafe { &mut *descriptor_ptr.as_ptr() };
        for (field_name, field_value) in [
            ("value", value),
            ("writable", Value::bool(writable)),
            ("enumerable", Value::bool(enumerable)),
            ("configurable", Value::bool(configurable)),
        ] {
            if let Some(field_index) = self.get_field_index_for_value(descriptor, field_name) {
                descriptor_obj
                    .set_field(field_index, field_value)
                    .map_err(VmError::RuntimeError)?;
            }
            self.set_descriptor_field_present(descriptor, field_name, true);
        }
        let result = self.apply_descriptor_to_target(target, key, descriptor);
        if debug_array_prop {
            eprintln!("[defineData] done result={:?}", result);
        }
        result
    }

    pub(in crate::vm::interpreter) fn define_accessor_property_on_target(
        &self,
        target: Value,
        key: &str,
        get: Value,
        set: Value,
        enumerable: bool,
        configurable: bool,
    ) -> Result<(), VmError> {
        let descriptor = self.alloc_object_descriptor()?;
        let Some(descriptor_ptr) = (unsafe { descriptor.as_ptr::<Object>() }) else {
            return Err(VmError::RuntimeError(
                "Failed to allocate property descriptor object".to_string(),
            ));
        };
        let descriptor_obj = unsafe { &mut *descriptor_ptr.as_ptr() };
        for (field_name, field_value) in [
            ("get", get),
            ("set", set),
            ("enumerable", Value::bool(enumerable)),
            ("configurable", Value::bool(configurable)),
        ] {
            if let Some(field_index) = self.get_field_index_for_value(descriptor, field_name) {
                descriptor_obj
                    .set_field(field_index, field_value)
                    .map_err(VmError::RuntimeError)?;
            }
            self.set_descriptor_field_present(descriptor, field_name, true);
        }
        self.apply_descriptor_to_target(target, key, descriptor)
    }

    pub(in crate::vm::interpreter) fn define_data_property_on_target_with_context(
        &mut self,
        target: Value,
        key: &str,
        value: Value,
        writable: bool,
        enumerable: bool,
        configurable: bool,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<(), VmError> {
        let descriptor = self.alloc_object_descriptor()?;
        let Some(descriptor_ptr) = (unsafe { descriptor.as_ptr::<Object>() }) else {
            return Err(VmError::RuntimeError(
                "Failed to allocate property descriptor object".to_string(),
            ));
        };
        let descriptor_obj = unsafe { &mut *descriptor_ptr.as_ptr() };
        for (field_name, field_value) in [
            ("value", value),
            ("writable", Value::bool(writable)),
            ("enumerable", Value::bool(enumerable)),
            ("configurable", Value::bool(configurable)),
        ] {
            if let Some(field_index) = self.get_field_index_for_value(descriptor, field_name) {
                descriptor_obj
                    .set_field(field_index, field_value)
                    .map_err(VmError::RuntimeError)?;
            }
            self.set_descriptor_field_present(descriptor, field_name, true);
        }
        self.apply_descriptor_to_target_with_context(
            target,
            key,
            descriptor,
            caller_task,
            caller_module,
        )
    }

    pub(in crate::vm::interpreter::opcodes) fn ordinary_own_property(
        &self,
        target: Value,
        key: &str,
    ) -> Option<OrdinaryOwnProperty> {
        if self.fixed_property_deleted(target, key) {
            return None;
        }

        if self.is_descriptor_object(target)
            && matches!(
                key,
                "value" | "writable" | "configurable" | "enumerable" | "get" | "set"
            )
            && !self.descriptor_field_present(target, key)
        {
            return None;
        }

        let obj_ptr = checked_object_ptr(target)?;
        let obj = unsafe { &*obj_ptr.as_ptr() };

        if let Some(slot_idx) = self.get_field_index_for_value(target, key) {
            if let Some(meta) = obj.slot_meta.get(slot_idx) {
                if let Some(accessor) = meta.accessor.as_ref() {
                    return Some(OrdinaryOwnProperty::Accessor {
                        get: accessor.get,
                        set: accessor.set,
                        enumerable: meta.enumerable,
                        configurable: meta.configurable,
                    });
                }
                let value = obj
                    .fields
                    .get(slot_idx)
                    .copied()
                    .unwrap_or(Value::undefined());
                // Builtin/runtime-published members often reserve a fixed slot
                // with a null placeholder. Treat that as "not an ordinary data
                // property" so descriptor/property resolution can fall through
                // to a shadowing dyn prop or, if none exists, the real
                // publication source.
                let runtime_placeholder = value.is_null()
                    && (self
                        .callable_virtual_property_descriptor(target, key)
                        .is_some()
                        || crate::vm::interpreter::opcodes::types::builtin_handle_native_method_id(
                            self.pinned_handles,
                            target,
                            key,
                        )
                        .is_some()
                        || self.has_constructor_static_method(target, key)
                        || self.has_class_vtable_method(target, key));
                if !runtime_placeholder {
                    return Some(OrdinaryOwnProperty::Data {
                        value,
                        writable: meta.writable,
                        enumerable: meta.enumerable,
                        configurable: meta.configurable,
                    });
                }
            }
        }

        let key_id = self.intern_prop_key(key);
        let prop = obj.dyn_props.as_deref().and_then(|dp| dp.get(key_id))?;
        if prop.is_accessor {
            return Some(OrdinaryOwnProperty::Accessor {
                get: prop.get,
                set: prop.set,
                enumerable: prop.enumerable,
                configurable: prop.configurable,
            });
        }

        Some(OrdinaryOwnProperty::Data {
            value: prop.value,
            writable: prop.writable,
            enumerable: prop.enumerable,
            configurable: prop.configurable,
        })
    }

    fn synthesize_descriptor_from_ordinary_own_property(
        &self,
        property: OrdinaryOwnProperty,
    ) -> Result<Value, VmError> {
        let descriptor = self.alloc_object_descriptor()?;
        match property {
            OrdinaryOwnProperty::Data {
                value,
                writable,
                enumerable,
                configurable,
            } => {
                self.set_internal_descriptor_field(descriptor, "value", value)?;
                self.set_internal_descriptor_field(descriptor, "writable", Value::bool(writable))?;
                self.set_internal_descriptor_field(
                    descriptor,
                    "enumerable",
                    Value::bool(enumerable),
                )?;
                self.set_internal_descriptor_field(
                    descriptor,
                    "configurable",
                    Value::bool(configurable),
                )?;
            }
            OrdinaryOwnProperty::Accessor {
                get,
                set,
                enumerable,
                configurable,
            } => {
                self.set_internal_descriptor_field(descriptor, "get", get)?;
                self.set_internal_descriptor_field(descriptor, "set", set)?;
                self.set_internal_descriptor_field(
                    descriptor,
                    "enumerable",
                    Value::bool(enumerable),
                )?;
                self.set_internal_descriptor_field(
                    descriptor,
                    "configurable",
                    Value::bool(configurable),
                )?;
            }
        }
        Ok(descriptor)
    }

    fn get_descriptor_metadata(&self, target: Value, key: &str) -> Option<Value> {
        if let Ok(Some(descriptor)) = self.synthesize_arguments_exotic_descriptor(target, key) {
            return Some(descriptor);
        }

        if let Some(descriptor) = self.metadata_descriptor_property(target, key) {
            return Some(descriptor);
        }

        if let Some(property) = self.ordinary_own_property(target, key) {
            return self
                .synthesize_descriptor_from_ordinary_own_property(property)
                .ok();
        }

        None
    }

    pub(in crate::vm::interpreter) fn metadata_data_property_value(
        &self,
        target: Value,
        key: &str,
    ) -> Option<Value> {
        if self.fixed_property_deleted(target, key) {
            return None;
        }
        let metadata = self.metadata.lock();
        metadata.get_metadata_property(NON_OBJECT_DYNAMIC_VALUE_METADATA_KEY, target, key)
    }

    fn is_descriptor_object(&self, value: Value) -> bool {
        let Some(obj_ptr) = checked_object_ptr(value) else {
            return false;
        };
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let mask_key = self.intern_prop_key(FIELD_PRESENT_MASK_KEY);
        obj.dyn_props
            .as_deref()
            .and_then(|dp| dp.get(mask_key))
            .is_some()
    }

    pub(in crate::vm::interpreter) fn descriptor_field_present(
        &self,
        descriptor: Value,
        field_name: &str,
    ) -> bool {
        if !self.is_descriptor_object(descriptor) {
            return self
                .get_own_field_value_by_name(descriptor, field_name)
                .is_some();
        }
        // For internally-allocated descriptor objects (which have all 6 fields
        // pre-allocated), check the presence bitmask stored in dyn_props.
        let Some(obj_ptr) = checked_object_ptr(descriptor) else {
            return false;
        };
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let mask_key = self.intern_prop_key(FIELD_PRESENT_MASK_KEY);
        let current_mask = obj
            .dyn_props
            .as_deref()
            .and_then(|dp| dp.get(mask_key))
            .and_then(|prop| prop.value.as_i32())
            .unwrap_or(0) as u32;
        let bit = descriptor_field_bit(field_name);
        (current_mask & bit) != 0
    }

    pub(in crate::vm::interpreter) fn set_descriptor_field_present(
        &self,
        descriptor: Value,
        field_name: &str,
        present: bool,
    ) {
        if !self.is_descriptor_object(descriptor) {
            return;
        }
        let Some(obj_ptr) = checked_object_ptr(descriptor) else {
            return;
        };
        let obj = unsafe { &mut *obj_ptr.as_ptr() };
        let mask_key = self.intern_prop_key(FIELD_PRESENT_MASK_KEY);
        let current_mask = obj
            .dyn_props
            .as_deref()
            .and_then(|dp| dp.get(mask_key))
            .and_then(|prop| prop.value.as_i32())
            .unwrap_or(0) as u32;
        let bit = descriptor_field_bit(field_name);
        let next_mask = if present {
            current_mask | bit
        } else {
            current_mask & !bit
        };
        obj.ensure_dyn_props()
            .insert(mask_key, DynProp::data(Value::i32(next_mask as i32)));
    }

    pub(in crate::vm::interpreter) fn is_property_enumerable(
        &self,
        target: Value,
        key: &str,
    ) -> bool {
        if let Some(shape) = self.resolve_own_property_shape(target, key) {
            if std::env::var("RAYA_DEBUG_FORIN_ENUM").is_ok() {
                eprintln!(
                    "[enum-flags] key='{}' w={} c={} e={} source={:?}",
                    key, shape.writable, shape.configurable, shape.enumerable, shape.source
                );
            }
            return shape.enumerable;
        }
        false
    }

    fn validate_descriptor_for_definition(
        &self,
        key: &str,
        descriptor: Value,
    ) -> Result<JsPropertyDescriptorRecord, VmError> {
        let record = self.descriptor_record_from_descriptor(descriptor);

        if record.has_get {
            let getter_val = record.get;
            if !getter_val.is_undefined() && !Self::is_callable_value(getter_val) {
                return Err(VmError::TypeError(format!(
                    "Getter for property '{}' must be callable",
                    key
                )));
            }
        }
        if record.has_set {
            let setter_val = record.set;
            if !setter_val.is_undefined() && !Self::is_callable_value(setter_val) {
                return Err(VmError::TypeError(format!(
                    "Setter for property '{}' must be callable",
                    key
                )));
            }
        }
        if record.is_accessor() && record.has_value {
            return Err(VmError::TypeError(format!(
                "Invalid property descriptor for '{}': cannot mix accessors and value",
                key
            )));
        }

        Ok(record)
    }

    fn apply_generic_descriptor_to_target(
        &self,
        target: Value,
        key: &str,
        descriptor: Value,
    ) -> Result<(), VmError> {
        if let Some(existing) = self.get_descriptor_metadata(target, key) {
            if !self.descriptor_flag(existing, "configurable", true) {
                return Err(VmError::TypeError(format!(
                    "Cannot redefine non-configurable property '{}'",
                    key
                )));
            }
        } else if !self.has_own_js_property(target, key) && !self.is_js_value_extensible(target) {
            return Err(VmError::TypeError(format!(
                "Cannot define property '{}': object is not extensible",
                key
            )));
        }

        let record = self.validate_descriptor_for_definition(key, descriptor)?;
        if self.apply_ordinary_descriptor_record(target, key, record)? {
            if record.has_value
                && self
                    .callable_virtual_property_descriptor(target, key)
                    .is_some()
            {
                self.set_cached_callable_virtual_property_value(target, key, record.value);
            }
            self.set_callable_virtual_property_deleted(target, key, false);
            return Ok(());
        }

        if checked_object_ptr(target).is_none() {
            self.set_descriptor_metadata(target, key, descriptor);
        }

        if record.is_data() {
            let value = if record.has_value {
                record.value
            } else {
                Value::undefined()
            };
            if self.set_builtin_global_property(target, key, value) {
                if self
                    .callable_virtual_property_descriptor(target, key)
                    .is_some()
                {
                    self.set_cached_callable_virtual_property_value(target, key, record.value);
                }
                self.set_callable_virtual_property_deleted(target, key, false);
                self.set_fixed_property_deleted(target, key, false);
                return Ok(());
            }

            self.metadata.lock().define_metadata_property(
                NON_OBJECT_DYNAMIC_VALUE_METADATA_KEY.to_string(),
                value,
                target,
                key.to_string(),
            );
            if self
                .callable_virtual_property_descriptor(target, key)
                .is_some()
            {
                self.set_cached_callable_virtual_property_value(target, key, value);
            }
        } else {
            self.metadata.lock().delete_metadata_property(
                NON_OBJECT_DYNAMIC_VALUE_METADATA_KEY,
                target,
                key,
            );
        }

        self.set_callable_virtual_property_deleted(target, key, false);
        self.set_fixed_property_deleted(target, key, false);
        Ok(())
    }

    fn apply_descriptor_to_target(
        &self,
        target: Value,
        key: &str,
        descriptor: Value,
    ) -> Result<(), VmError> {
        if !descriptor.is_ptr() {
            return Err(VmError::TypeError(
                "Object property descriptor must be an object".to_string(),
            ));
        }
        self.apply_generic_descriptor_to_target(target, key, descriptor)
    }

    fn exotic_define_own_property_with_context(
        &mut self,
        target: Value,
        key: &str,
        descriptor: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<Option<()>, VmError> {
        if let Some(()) = self.array_exotic_define_own_property_with_context(
            target,
            key,
            descriptor,
            caller_task,
            caller_module,
        )? {
            return Ok(Some(()));
        }

        if self
            .typed_array_define_indexed_property(
                target,
                key,
                descriptor,
                caller_task,
                caller_module,
            )?
            .is_some()
        {
            return Ok(Some(()));
        }

        if let Some(()) = self.arguments_exotic_define_own_property(target, key, descriptor)? {
            return Ok(Some(()));
        }

        Ok(None)
    }

    fn apply_descriptor_to_target_with_context(
        &mut self,
        target: Value,
        key: &str,
        descriptor: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<(), VmError> {
        if !descriptor.is_ptr() {
            return Err(VmError::TypeError(
                "Object property descriptor must be an object".to_string(),
            ));
        }

        let descriptor = self.normalize_property_descriptor_with_context(
            descriptor,
            caller_task,
            caller_module,
        )?;

        if let Some(proxy) = self.unwrapped_proxy_like(target) {
            if proxy.handler.is_null() {
                return Err(VmError::TypeError("Proxy has been revoked".to_string()));
            }
            if let Some(trap) = self.get_field_value_by_name(proxy.handler, "defineProperty") {
                if !trap.is_undefined() && !trap.is_null() {
                    let trap_result = self
                        .invoke_proxy_property_trap_with_context(
                            trap,
                            proxy.handler,
                            proxy.target,
                            key,
                            &[descriptor],
                            caller_task,
                            caller_module,
                        )?
                        .is_truthy();
                    if !trap_result {
                        return Err(VmError::TypeError(format!(
                            "Proxy defineProperty trap returned false for '{}'",
                            key
                        )));
                    }
                    self.enforce_proxy_define_invariants(
                        proxy.target,
                        key,
                        descriptor,
                        caller_task,
                        caller_module,
                    )?;
                    return Ok(());
                }
            }
            return self.apply_descriptor_to_target_with_context(
                proxy.target,
                key,
                descriptor,
                caller_task,
                caller_module,
            );
        }

        if let Some(()) = self.exotic_define_own_property_with_context(
            target,
            key,
            descriptor,
            caller_task,
            caller_module,
        )? {
            return Ok(());
        }

        self.apply_generic_descriptor_to_target(target, key, descriptor)
    }

    fn channel_from_handle_arg(&self, value: Value) -> Result<(u64, &ChannelObject), VmError> {
        let Some(handle) = value.as_u64() else {
            return Err(VmError::TypeError(
                "Expected channel handle (u64)".to_string(),
            ));
        };
        if !self.pinned_handles.read().contains(&handle) {
            return Err(VmError::TypeError(
                "Expected channel handle (u64)".to_string(),
            ));
        }
        let ch_ptr = handle as *const ChannelObject;
        if ch_ptr.is_null() {
            return Err(VmError::TypeError(
                "Expected channel handle (u64)".to_string(),
            ));
        }
        Ok((handle, unsafe { &*ch_ptr }))
    }

    fn buffer_handle_from_value(&self, value: Value) -> Result<u64, VmError> {
        let obj_ptr = unsafe { value.as_ptr::<Object>() }
            .ok_or_else(|| VmError::TypeError("Expected Buffer object".to_string()))?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let nominal_type_id = obj
            .nominal_type_id_usize()
            .ok_or_else(|| VmError::TypeError("Expected Buffer object".to_string()))?;
        let classes = self.classes.read();
        let class = classes
            .get_class(nominal_type_id)
            .ok_or_else(|| VmError::RuntimeError("Buffer class metadata missing".to_string()))?;
        if class.name != "Buffer" {
            return Err(VmError::TypeError("Expected Buffer object".to_string()));
        }
        drop(classes);

        let field_index = self
            .get_field_index_for_value(value, "bufferPtr")
            .ok_or_else(|| {
                VmError::RuntimeError("Buffer field 'bufferPtr' not found".to_string())
            })?;
        let handle = obj
            .get_field(field_index)
            .and_then(|f| f.as_u64())
            .ok_or_else(|| {
                VmError::RuntimeError("Buffer.bufferPtr is not a valid handle".to_string())
            })?;
        Ok(handle)
    }

    fn decode_u64_handle(value: Value) -> Option<u64> {
        if let Some(h) = value.as_u64() {
            return Some(h);
        }
        if let Some(i) = value.as_i64() {
            if i >= 0 {
                return Some(i as u64);
            }
        }
        if let Some(i) = value.as_i32() {
            if i >= 0 {
                return Some(i as u64);
            }
        }
        if let Some(f) = value.as_f64() {
            if f.is_finite() && f >= 0.0 && f.fract() == 0.0 && f <= u64::MAX as f64 {
                return Some(f as u64);
            }
        }
        None
    }

    fn map_handle_from_value(&self, value: Value) -> Result<u64, VmError> {
        if let Some(handle) = Self::decode_u64_handle(value) {
            return Ok(handle);
        }
        let obj_ptr = unsafe { value.as_ptr::<Object>() }
            .ok_or_else(|| VmError::TypeError("Expected Map object or map handle".to_string()))?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let field_index = self
            .get_field_index_for_value(value, "mapPtr")
            .ok_or_else(|| VmError::RuntimeError("Map field 'mapPtr' not found".to_string()))?;
        let raw = obj
            .get_field(field_index)
            .ok_or_else(|| VmError::RuntimeError("Map.mapPtr is missing".to_string()))?;
        Self::decode_u64_handle(raw)
            .ok_or_else(|| VmError::RuntimeError("Map.mapPtr is not a valid handle".to_string()))
    }

    fn set_handle_from_value(&self, value: Value) -> Result<u64, VmError> {
        if let Some(handle) = Self::decode_u64_handle(value) {
            return Ok(handle);
        }
        let obj_ptr = unsafe { value.as_ptr::<Object>() }
            .ok_or_else(|| VmError::TypeError("Expected Set object or set handle".to_string()))?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let field_index = self
            .get_field_index_for_value(value, "setPtr")
            .ok_or_else(|| VmError::RuntimeError("Set field 'setPtr' not found".to_string()))?;
        let raw = obj
            .get_field(field_index)
            .ok_or_else(|| VmError::RuntimeError("Set.setPtr is missing".to_string()))?;
        Self::decode_u64_handle(raw)
            .ok_or_else(|| VmError::RuntimeError("Set.setPtr is not a valid handle".to_string()))
    }

    pub(in crate::vm::interpreter) fn regexp_handle_from_value(
        &self,
        value: Value,
    ) -> Result<u64, VmError> {
        if let Some(handle) = Self::decode_u64_handle(value) {
            return Ok(handle);
        }
        let obj_ptr = unsafe { value.as_ptr::<Object>() }.ok_or_else(|| {
            VmError::TypeError("Expected RegExp object or regexp handle".to_string())
        })?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let field_index = self
            .get_field_index_for_value(value, "regexpPtr")
            .ok_or_else(|| {
                VmError::RuntimeError("RegExp field 'regexpPtr' not found".to_string())
            })?;
        let raw = obj
            .get_field(field_index)
            .ok_or_else(|| VmError::RuntimeError("RegExp.regexpPtr is missing".to_string()))?;
        Self::decode_u64_handle(raw).ok_or_else(|| {
            VmError::RuntimeError("RegExp.regexpPtr is not a valid handle".to_string())
        })
    }

    fn ensure_buffer_class_layout(&self) -> (usize, usize, LayoutId) {
        let mut classes = self.classes.write();
        if let Some(id) = classes.get_class_by_name("Buffer").map(|class| class.id) {
            let (layout_id, field_count) = self
                .nominal_allocation(id)
                .expect("registered Buffer allocation");
            (id, field_count.max(2), layout_id)
        } else {
            drop(classes);
            let id = self.register_runtime_class_with_layout_names(
                Class::new(0, "Buffer".to_string(), 2),
                Some(crate::vm::object::BUFFER_LAYOUT_FIELDS),
            );
            let (layout_id, field_count) = self
                .nominal_allocation(id)
                .expect("registered Buffer allocation");
            (id, field_count.max(2), layout_id)
        }
    }

    fn ensure_object_class_layout(&self) -> (usize, usize, LayoutId) {
        let mut classes = self.classes.write();
        if let Some(id) = classes.get_class_by_name("Object").map(|class| class.id) {
            let (_, mut field_count) = self
                .nominal_allocation(id)
                .expect("registered Object allocation");
            if field_count < 6 {
                drop(classes);
                self.set_nominal_field_count(id, 6);
                field_count = 6;
                classes = self.classes.write();
            }
            let (layout_id, field_count) = self
                .nominal_allocation(id)
                .expect("registered Object allocation");
            (id, field_count.max(6), layout_id)
        } else {
            drop(classes);
            let id = self.register_runtime_class_with_layout_names(
                Class::new(0, "Object".to_string(), 6),
                Some(crate::vm::object::OBJECT_DESCRIPTOR_LAYOUT_FIELDS),
            );
            let (layout_id, field_count) = self
                .nominal_allocation(id)
                .expect("registered Object allocation");
            (id, field_count.max(6), layout_id)
        }
    }

    fn ensure_symbol_class_layout(&self) -> (usize, usize, LayoutId) {
        let mut classes = self.classes.write();
        if let Some(id) = classes.get_class_by_name("Symbol").map(|class| class.id) {
            let (_, mut field_count) = self
                .nominal_allocation(id)
                .expect("registered Symbol allocation");
            if field_count < 1 {
                drop(classes);
                self.set_nominal_field_count(id, 1);
                field_count = 1;
                classes = self.classes.write();
            }
            let (layout_id, field_count) = self
                .nominal_allocation(id)
                .expect("registered Symbol allocation");
            (id, field_count.max(1), layout_id)
        } else {
            drop(classes);
            const SYMBOL_LAYOUT_FIELDS: &[&str] = &["key"];
            let id = self.register_runtime_class_with_layout_names(
                Class::new(0, "Symbol".to_string(), 1),
                Some(SYMBOL_LAYOUT_FIELDS),
            );
            let (layout_id, field_count) = self
                .nominal_allocation(id)
                .expect("registered Symbol allocation");
            (id, field_count.max(1), layout_id)
        }
    }

    fn alloc_buffer_object(&self, handle: u64, len: usize) -> Result<Value, VmError> {
        let (buffer_nominal_type_id, buffer_field_count, buffer_layout_id) =
            self.ensure_buffer_class_layout();
        let mut obj = Object::new_nominal(
            buffer_layout_id,
            buffer_nominal_type_id as u32,
            buffer_field_count,
        );
        obj.set_field(0, Value::u64(handle))
            .map_err(VmError::RuntimeError)?;
        if buffer_field_count > 1 {
            obj.set_field(1, Value::i32(len as i32))
                .map_err(VmError::RuntimeError)?;
        }
        let obj_ptr = self.gc.lock().allocate(obj);
        Ok(unsafe { Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap()) })
    }

    pub(in crate::vm::interpreter) fn alloc_nominal_instance_value(
        &self,
        nominal_type_id: usize,
    ) -> Result<Value, VmError> {
        let (layout_id, field_count) = self
            .nominal_allocation(nominal_type_id)
            .ok_or_else(|| VmError::RuntimeError(format!("Class {} not found", nominal_type_id)))?;

        let mut obj = Object::new_nominal(layout_id, nominal_type_id as u32, field_count);
        // Set [[Prototype]] from the class's registered prototype
        {
            let classes = self.classes.read();
            if let Some(class) = classes.get_class(nominal_type_id) {
                if let Some(proto_val) = class.prototype_value {
                    obj.prototype = proto_val;
                }
            }
        }
        let gc_ptr = self.gc.lock().allocate(obj);
        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };
        let field_names = {
            let class_metadata = self.class_metadata.read();
            class_metadata
                .get(nominal_type_id)
                .map(|meta| meta.field_names.clone())
                .unwrap_or_default()
        };
        for field_name in field_names {
            if !field_name.is_empty() {
                self.set_fixed_property_deleted(value, &field_name, true);
            }
        }
        Ok(value)
    }

    fn alloc_object_descriptor(&self) -> Result<Value, VmError> {
        let field_names = crate::vm::object::OBJECT_DESCRIPTOR_LAYOUT_FIELDS
            .iter()
            .map(|name| (*name).to_string())
            .collect::<Vec<_>>();
        let object_layout_id = layout_id_from_ordered_names(&field_names);
        self.register_structural_layout_shape(object_layout_id, &field_names);
        let object_field_count = field_names.len();
        let mut obj = Object::new_structural(object_layout_id, object_field_count);
        if object_field_count > 0 {
            obj.set_field(0, Value::undefined())
                .map_err(VmError::RuntimeError)?;
        }
        if object_field_count > 1 {
            obj.set_field(1, Value::bool(false))
                .map_err(VmError::RuntimeError)?;
        }
        if object_field_count > 2 {
            obj.set_field(2, Value::bool(false))
                .map_err(VmError::RuntimeError)?;
        }
        if object_field_count > 3 {
            obj.set_field(3, Value::bool(false))
                .map_err(VmError::RuntimeError)?;
        }
        if object_field_count > 4 {
            obj.set_field(4, Value::undefined())
                .map_err(VmError::RuntimeError)?;
        }
        if object_field_count > 5 {
            obj.set_field(5, Value::undefined())
                .map_err(VmError::RuntimeError)?;
        }
        let mask_key = self.intern_prop_key(FIELD_PRESENT_MASK_KEY);
        obj.ensure_dyn_props()
            .insert(mask_key, DynProp::data(Value::i32(0)));
        let obj_ptr = self.gc.lock().allocate(obj);
        let descriptor =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap()) };
        Ok(descriptor)
    }

    fn clone_descriptor_object(&self, descriptor: Value) -> Result<Value, VmError> {
        let record = self.descriptor_record_from_descriptor(descriptor);
        let cloned = self.alloc_object_descriptor()?;
        if record.has_value {
            self.set_internal_descriptor_field(cloned, "value", record.value)?;
        }
        if record.has_writable {
            self.set_internal_descriptor_field(cloned, "writable", Value::bool(record.writable))?;
        }
        if record.has_enumerable {
            self.set_internal_descriptor_field(
                cloned,
                "enumerable",
                Value::bool(record.enumerable),
            )?;
        }
        if record.has_configurable {
            self.set_internal_descriptor_field(
                cloned,
                "configurable",
                Value::bool(record.configurable),
            )?;
        }
        if record.has_get {
            self.set_internal_descriptor_field(cloned, "get", record.get)?;
        }
        if record.has_set {
            self.set_internal_descriptor_field(cloned, "set", record.set)?;
        }
        Ok(cloned)
    }

    fn descriptor_record_from_descriptor(&self, descriptor: Value) -> JsPropertyDescriptorRecord {
        let mut record = JsPropertyDescriptorRecord::default();

        for field_name in [
            "enumerable",
            "configurable",
            "value",
            "writable",
            "get",
            "set",
        ] {
            if !self.descriptor_field_present(descriptor, field_name) {
                continue;
            }
            let value = self
                .get_field_value_by_name(descriptor, field_name)
                .unwrap_or(Value::undefined());
            match field_name {
                "enumerable" => {
                    record.has_enumerable = true;
                    record.enumerable = value.is_truthy();
                }
                "configurable" => {
                    record.has_configurable = true;
                    record.configurable = value.is_truthy();
                }
                "value" => {
                    record.has_value = true;
                    record.value = value;
                }
                "writable" => {
                    record.has_writable = true;
                    record.writable = value.is_truthy();
                }
                "get" => {
                    record.has_get = true;
                    record.get = value;
                }
                "set" => {
                    record.has_set = true;
                    record.set = value;
                }
                _ => {}
            }
        }

        record
    }

    fn apply_descriptor_record_to_ordinary_property(
        &self,
        current: Option<OrdinaryOwnProperty>,
        record: JsPropertyDescriptorRecord,
    ) -> OrdinaryOwnProperty {
        let descriptor_is_accessor = record.is_accessor();
        let descriptor_is_data = record.is_data();

        match current {
            Some(OrdinaryOwnProperty::Data {
                value,
                writable,
                enumerable,
                configurable,
            }) => {
                if descriptor_is_accessor {
                    OrdinaryOwnProperty::Accessor {
                        get: if record.has_get {
                            record.get
                        } else {
                            Value::undefined()
                        },
                        set: if record.has_set {
                            record.set
                        } else {
                            Value::undefined()
                        },
                        enumerable: if record.has_enumerable {
                            record.enumerable
                        } else {
                            enumerable
                        },
                        configurable: if record.has_configurable {
                            record.configurable
                        } else {
                            configurable
                        },
                    }
                } else {
                    OrdinaryOwnProperty::Data {
                        value: if record.has_value {
                            record.value
                        } else {
                            value
                        },
                        writable: if record.has_writable {
                            record.writable
                        } else {
                            writable
                        },
                        enumerable: if record.has_enumerable {
                            record.enumerable
                        } else {
                            enumerable
                        },
                        configurable: if record.has_configurable {
                            record.configurable
                        } else {
                            configurable
                        },
                    }
                }
            }
            Some(OrdinaryOwnProperty::Accessor {
                get,
                set,
                enumerable,
                configurable,
            }) => {
                if descriptor_is_data {
                    OrdinaryOwnProperty::Data {
                        value: if record.has_value {
                            record.value
                        } else {
                            Value::undefined()
                        },
                        writable: if record.has_writable {
                            record.writable
                        } else {
                            false
                        },
                        enumerable: if record.has_enumerable {
                            record.enumerable
                        } else {
                            enumerable
                        },
                        configurable: if record.has_configurable {
                            record.configurable
                        } else {
                            configurable
                        },
                    }
                } else {
                    OrdinaryOwnProperty::Accessor {
                        get: if record.has_get { record.get } else { get },
                        set: if record.has_set { record.set } else { set },
                        enumerable: if record.has_enumerable {
                            record.enumerable
                        } else {
                            enumerable
                        },
                        configurable: if record.has_configurable {
                            record.configurable
                        } else {
                            configurable
                        },
                    }
                }
            }
            None => {
                if descriptor_is_accessor {
                    OrdinaryOwnProperty::Accessor {
                        get: if record.has_get {
                            record.get
                        } else {
                            Value::undefined()
                        },
                        set: if record.has_set {
                            record.set
                        } else {
                            Value::undefined()
                        },
                        enumerable: if record.has_enumerable {
                            record.enumerable
                        } else {
                            false
                        },
                        configurable: if record.has_configurable {
                            record.configurable
                        } else {
                            false
                        },
                    }
                } else {
                    OrdinaryOwnProperty::Data {
                        value: if record.has_value {
                            record.value
                        } else {
                            Value::undefined()
                        },
                        writable: if record.has_writable {
                            record.writable
                        } else {
                            false
                        },
                        enumerable: if record.has_enumerable {
                            record.enumerable
                        } else {
                            false
                        },
                        configurable: if record.has_configurable {
                            record.configurable
                        } else {
                            false
                        },
                    }
                }
            }
        }
    }

    fn write_ordinary_own_property(
        &self,
        target: Value,
        key: &str,
        property: OrdinaryOwnProperty,
    ) -> Result<bool, VmError> {
        let Some(obj_ptr) = checked_object_ptr(target) else {
            return Ok(false);
        };
        let obj = unsafe { &mut *obj_ptr.as_ptr() };

        if let Some(field_index) = self.get_field_index_for_value(target, key) {
            match property {
                OrdinaryOwnProperty::Data {
                    value,
                    writable,
                    enumerable,
                    configurable,
                } => {
                    obj.set_field(field_index, value)
                        .map_err(VmError::RuntimeError)?;
                    if let Some(meta) = obj.slot_meta.get_mut(field_index) {
                        meta.writable = writable;
                        meta.enumerable = enumerable;
                        meta.configurable = configurable;
                        meta.accessor = None;
                    }
                }
                OrdinaryOwnProperty::Accessor {
                    get,
                    set,
                    enumerable,
                    configurable,
                } => {
                    obj.set_field(field_index, Value::undefined())
                        .map_err(VmError::RuntimeError)?;
                    if let Some(meta) = obj.slot_meta.get_mut(field_index) {
                        meta.writable = false;
                        meta.enumerable = enumerable;
                        meta.configurable = configurable;
                        meta.accessor =
                            Some(Box::new(crate::vm::object::AccessorPair { get, set }));
                    }
                }
            }
            self.set_fixed_property_deleted(target, key, false);
            if self.is_descriptor_object(target)
                && matches!(
                    key,
                    "value" | "writable" | "configurable" | "enumerable" | "get" | "set"
                )
            {
                self.set_descriptor_field_present(target, key, true);
            }
            return Ok(true);
        }

        let key_id = self.intern_prop_key(key);
        let dyn_prop = match property {
            OrdinaryOwnProperty::Data {
                value,
                writable,
                enumerable,
                configurable,
            } => DynProp::data_with_attrs(value, writable, enumerable, configurable),
            OrdinaryOwnProperty::Accessor {
                get,
                set,
                enumerable,
                configurable,
            } => DynProp::accessor(get, set, enumerable, configurable),
        };
        obj.ensure_dyn_props().insert(key_id, dyn_prop);
        self.set_fixed_property_deleted(target, key, false);
        Ok(true)
    }

    fn apply_ordinary_descriptor_record(
        &self,
        target: Value,
        key: &str,
        record: JsPropertyDescriptorRecord,
    ) -> Result<bool, VmError> {
        if checked_object_ptr(target).is_none() {
            return Ok(false);
        }
        let current = self.ordinary_own_property(target, key);
        let next = self.apply_descriptor_record_to_ordinary_property(current, record);
        self.write_ordinary_own_property(target, key, next)
    }

    fn ordinary_layout_backed_property_names(
        &self,
        target: Value,
        collector: &mut OrderedOwnKeyCollector,
    ) {
        let Some(obj_ptr) = checked_object_ptr(target) else {
            return;
        };
        let obj = unsafe { &*obj_ptr.as_ptr() };

        let layout_names = if let Some(nominal_type_id) = obj.nominal_type_id_usize() {
            let class_metadata = self.class_metadata.read();
            class_metadata
                .get(nominal_type_id)
                .map(|meta| meta.field_names.clone())
                .filter(|field_names| !field_names.is_empty())
                .or_else(|| self.layout_field_names_for_object(obj))
        } else {
            self.layout_field_names_for_object(obj)
        };

        if let Some(layout_names) = layout_names {
            for name in layout_names {
                if self.ordinary_own_property(target, &name).is_some() {
                    collector.push(name);
                }
            }
        } else {
            for index in 0..obj.field_count() {
                collector.push(format!("field_{}", index));
            }
        }
    }

    fn ordinary_dynamic_property_names(
        &self,
        target: Value,
        collector: &mut OrderedOwnKeyCollector,
    ) {
        let Some(obj_ptr) = checked_object_ptr(target) else {
            return;
        };
        let obj = unsafe { &*obj_ptr.as_ptr() };
        if let Some(dp) = obj.dyn_props() {
            for key_id in dp.keys_in_order() {
                let Some(name) = self.prop_key_name(key_id) else {
                    continue;
                };
                if self
                    .callable_virtual_property_descriptor(target, &name)
                    .is_some()
                {
                    continue;
                }
                if self.ordinary_own_property(target, &name).is_some() {
                    collector.push(name);
                }
            }
        }
    }

    fn ordinary_named_own_property_keys(&self, target: Value) -> Vec<String> {
        let mut collector = OrderedOwnKeyCollector::default();
        self.ordinary_layout_backed_property_names(target, &mut collector);
        self.ordinary_dynamic_property_names(target, &mut collector);
        collector.finish()
    }

    fn ordinary_dynamic_own_property_keys(&self, target: Value) -> Vec<String> {
        let mut collector = OrderedOwnKeyCollector::default();
        self.ordinary_dynamic_property_names(target, &mut collector);
        collector.finish()
    }

    fn callable_virtual_own_property_names(&self, target: Value) -> Vec<String> {
        let mut collector = OrderedOwnKeyCollector::default();
        for key in ["length", "name", "prototype"] {
            if self
                .callable_virtual_property_descriptor(target, key)
                .is_some()
            {
                collector.push(key.to_string());
            }
        }
        collector.finish()
    }

    fn metadata_backed_property_names(&self, target: Value) -> Vec<String> {
        self.metadata
            .lock()
            .get_property_keys_for_metadata(target, NON_OBJECT_DYNAMIC_VALUE_METADATA_KEY)
    }

    fn append_exotic_own_property_names(
        &self,
        kind: JsExoticAdapterKind,
        target: Value,
        collector: &mut OrderedOwnKeyCollector,
    ) {
        match kind {
            JsExoticAdapterKind::Arguments => {
                let Some(obj_ptr) = checked_object_ptr(target) else {
                    return;
                };
                let obj = unsafe { &*obj_ptr.as_ptr() };
                let Some(arguments) = obj.arguments.as_deref() else {
                    return;
                };

                for (index, property) in arguments.indexed.iter().enumerate() {
                    if !property.deleted {
                        collector.push(index.to_string());
                    }
                }
                if arguments.length.is_some() {
                    collector.push("length".to_string());
                }
                if arguments.strict_poison || arguments.callee.is_some() {
                    collector.push("callee".to_string());
                }
                collector.extend(self.ordinary_dynamic_own_property_keys(target));
                collector.extend(self.metadata_backed_property_names(target));
            }
            JsExoticAdapterKind::Array => {
                let Some(array_ptr) = checked_array_ptr(target) else {
                    return;
                };
                let array = unsafe { &*array_ptr.as_ptr() };

                for index in 0..array.len() {
                    if array.get(index).is_some() {
                        collector.push(index.to_string());
                    }
                }

                collector.push("length".to_string());
                collector.extend(self.metadata_backed_property_names(target));
                collector.extend(self.metadata_descriptor_property_names(target));
            }
            JsExoticAdapterKind::String => {
                let Some(length) = self.string_exotic_length(target) else {
                    return;
                };

                for index in 0..length {
                    collector.push(index.to_string());
                }

                collector.push("length".to_string());
                collector.extend(self.metadata_backed_property_names(target));
                collector.extend(self.metadata_descriptor_property_names(target));
            }
            JsExoticAdapterKind::TypedArray => {
                let Some(length) = self.typed_array_live_length_direct(target) else {
                    return;
                };

                for index in 0..length {
                    collector.push(index.to_string());
                }

                collector.extend(self.ordinary_dynamic_own_property_keys(target));
                collector.extend(self.metadata_backed_property_names(target));
            }
        }
    }

    pub(in crate::vm::interpreter) fn js_own_property_names(&self, target: Value) -> Vec<String> {
        let target = self.public_property_target(target);
        let debug_class_publication = std::env::var("RAYA_DEBUG_CLASS_PUBLICATION").is_ok();
        let mut collector = OrderedOwnKeyCollector::default();
        if let Some(kind) = self.exotic_adapter_kind(target) {
            self.append_exotic_own_property_names(kind, target, &mut collector);
        } else {
            collector.extend(self.callable_virtual_own_property_names(target));
            collector.extend(self.ordinary_named_own_property_keys(target));
            collector.extend(self.metadata_backed_property_names(target));
            collector.extend(self.metadata_descriptor_property_names(target));
        }

        if let Some(global_obj) = self.builtin_global_value("globalThis") {
            if global_obj.raw() == target.raw() {
                for name in ["Infinity", "NaN", "undefined"] {
                    if !self.fixed_property_deleted(target, name) {
                        collector.push(name.to_string());
                    }
                }
                for name in self.builtin_global_slots.read().keys() {
                    if self.fixed_property_deleted(target, name) {
                        continue;
                    }
                    collector.push(name.clone());
                }
            }
        }

        let names = collector
            .finish()
            .into_iter()
            .filter(|name| Self::is_public_string_property_name(name))
            .collect::<Vec<_>>();
        if debug_class_publication {
            if let Some(obj_ptr) = checked_object_ptr(target) {
                let obj = unsafe { obj_ptr.as_ref() };
                let handle_key = self.intern_prop_key("__raya_type_handle__");
                let has_handle = obj
                    .dyn_props()
                    .is_some_and(|dp| dp.contains_key(handle_key));
                eprintln!(
                    "[own-names] target={:#x} is_object=true is_callable={} nominal={:?} layout={} fields={} has_handle={} names={:?}",
                    target.raw(),
                    checked_callable_ptr(target).is_some(),
                    obj.nominal_type_id_usize(),
                    obj.layout_id(),
                    obj.field_count(),
                    has_handle,
                    names
                );
            } else {
                eprintln!(
                    "[own-names] target={:#x} is_object=false is_callable={} names={:?}",
                    target.raw(),
                    checked_callable_ptr(target).is_some(),
                    names
                );
            }
        }
        names
    }

    fn js_own_property_symbols(&self, target: Value) -> Vec<Value> {
        let target = self.public_property_target(target);
        let mut collector = OrderedOwnKeyCollector::default();
        if let Some(kind) = self.exotic_adapter_kind(target) {
            self.append_exotic_own_property_names(kind, target, &mut collector);
        } else {
            collector.extend(self.callable_virtual_own_property_names(target));
            collector.extend(self.ordinary_named_own_property_keys(target));
            collector.extend(self.metadata_backed_property_names(target));
            collector.extend(self.metadata_descriptor_property_names(target));
        }

        collector
            .finish()
            .into_iter()
            .filter_map(|name| self.symbol_value_from_property_key_name(&name))
            .collect()
    }

    fn copy_data_properties_with_context(
        &mut self,
        target: Value,
        source: Value,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<(), VmError> {
        if source.is_null() || source.is_undefined() {
            return Ok(());
        }

        let mut property_keys = self.js_own_property_names(source);
        for symbol in self.js_own_property_symbols(source) {
            let (Some(symbol_key), _) = self.property_key_parts_with_context(
                symbol,
                "Object.copyDataProperties",
                caller_task,
                caller_module,
            )?
            else {
                continue;
            };
            property_keys.push(symbol_key);
        }

        for key in property_keys {
            let Some(shape) = self.resolve_own_property_shape(source, &key) else {
                continue;
            };
            if !shape.enumerable {
                continue;
            }
            let value = self
                .get_own_property_value_via_js_semantics_with_context(
                    source,
                    &key,
                    caller_task,
                    caller_module,
                )?
                .unwrap_or(Value::undefined());
            self.define_data_property_on_target_with_context(
                target,
                &key,
                value,
                true,
                true,
                true,
                caller_task,
                caller_module,
            )?;
        }

        Ok(())
    }

    fn copy_data_properties_excluding_with_context(
        &mut self,
        target: Value,
        source: Value,
        excluded_keys: &FxHashSet<String>,
        caller_task: &Arc<Task>,
        caller_module: &Module,
    ) -> Result<(), VmError> {
        if source.is_null() || source.is_undefined() {
            return Ok(());
        }

        let mut property_keys = self.js_own_property_names(source);
        for symbol in self.js_own_property_symbols(source) {
            let (Some(symbol_key), _) = self.property_key_parts_with_context(
                symbol,
                "Object.copyDataPropertiesExcluding",
                caller_task,
                caller_module,
            )?
            else {
                continue;
            };
            property_keys.push(symbol_key);
        }

        for key in property_keys {
            if excluded_keys.contains(&key) {
                continue;
            }
            let Some(shape) = self.resolve_own_property_shape(source, &key) else {
                continue;
            };
            if !shape.enumerable {
                continue;
            }
            let value = self
                .get_own_property_value_via_js_semantics_with_context(
                    source,
                    &key,
                    caller_task,
                    caller_module,
                )?
                .unwrap_or(Value::undefined());
            self.define_data_property_on_target_with_context(
                target,
                &key,
                value,
                true,
                true,
                true,
                caller_task,
                caller_module,
            )?;
        }

        Ok(())
    }

    fn alloc_plain_object(&self) -> Result<Value, VmError> {
        let field_names: Vec<String> = Vec::new();
        let layout_id = layout_id_from_ordered_names(&field_names);
        self.register_structural_layout_shape(layout_id, &field_names);
        let obj = Object::new_dynamic(layout_id, 0);
        let obj_ptr = self.gc.lock().allocate(obj);
        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap()) };
        if let Some(prototype) = self.ordinary_object_prototype_value() {
            self.set_constructed_object_prototype_from_value(value, prototype);
        }
        Ok(value)
    }

    fn alloc_symbol_object(&self, key: &str) -> Result<Value, VmError> {
        let (symbol_nominal_type_id, symbol_field_count, symbol_layout_id) =
            self.ensure_symbol_class_layout();
        let mut obj = Object::new_nominal(
            symbol_layout_id,
            symbol_nominal_type_id as u32,
            symbol_field_count,
        );
        let key_ptr = self.gc.lock().allocate(RayaString::new(key.to_string()));
        let key_value =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(key_ptr.as_ptr()).unwrap()) };
        obj.set_field(0, key_value).map_err(VmError::RuntimeError)?;
        let obj_ptr = self.gc.lock().allocate(obj);
        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(obj_ptr.as_ptr()).unwrap()) };
        if let Some(symbol_ctor) = self.builtin_global_value("Symbol") {
            if let Some(prototype) = self
                .create_prototype_for_class_by_name("Symbol", symbol_ctor)
                .or_else(|| self.constructor_prototype_value(symbol_ctor))
            {
                self.set_constructed_object_prototype_from_value(value, prototype);
            }
        }
        Ok(value)
    }

    fn synthesize_data_property_descriptor(
        &self,
        target: Value,
        key: &str,
    ) -> Result<Option<Value>, VmError> {
        if self.is_descriptor_object(target)
            && matches!(
                key,
                "value" | "writable" | "configurable" | "enumerable" | "get" | "set"
            )
            && !self.descriptor_field_present(target, key)
        {
            return Ok(None);
        }
        if self.fixed_property_deleted(target, key) {
            return Ok(None);
        }
        let exotic_value = self.get_own_js_property_value_by_name(target, key);
        let callable_value = self
            .callable_virtual_property_value(target, key)
            .or_else(|| self.materialize_constructor_static_method(target, key));
        let builtin_native_value = self.own_builtin_native_method_value(target, key);
        let builtin_global_value = self.builtin_global_property_value(target, key);
        let object_value = checked_object_ptr(target).map(|obj_ptr| {
            let obj = unsafe { &*obj_ptr.as_ptr() };
            let fixed_value = self
                .get_field_index_for_value(target, key)
                .and_then(|index| obj.get_field(index));
            let fixed_value =
                if fixed_value.is_some_and(|value| value.is_null()) && callable_value.is_some() {
                    None
                } else {
                    fixed_value
                };
            let dynamic_value = obj
                .dyn_props()
                .and_then(|dp| dp.get(self.intern_prop_key(key)).map(|p| p.value));
            fixed_value.or(dynamic_value)
        });
        let metadata_value = self.metadata_data_property_value(target, key);
        let Some(value) = exotic_value
            .or(object_value.flatten())
            .or(metadata_value)
            .or(builtin_native_value)
            .or(builtin_global_value)
            .or(callable_value)
        else {
            return Ok(None);
        };
        let own_flags = self.own_js_property_flags(target, key);
        let object_backed_value = object_value
            .flatten()
            .or(metadata_value)
            .or(builtin_global_value);

        let descriptor = self.alloc_object_descriptor()?;
        let legacy_error_descriptor = self.legacy_error_field_descriptor(target, key);
        let callable_virtual_descriptor = self.callable_virtual_property_descriptor(target, key);
        let resolved_data_shape = self
            .resolve_own_property_shape(target, key)
            .filter(|shape| matches!(shape.kind, JsOwnPropertyKind::Data));
        let writable_flag = resolved_data_shape
            .map(|shape| shape.writable)
            .or(callable_virtual_descriptor
                .or(legacy_error_descriptor)
                .map(|(writable, _, _)| writable)
                .or(own_flags.map(|(writable, _, _)| writable)))
            .unwrap_or(object_backed_value.is_some());
        let configurable_flag = resolved_data_shape
            .map(|shape| shape.configurable)
            .or(callable_virtual_descriptor
                .or(legacy_error_descriptor)
                .map(|(_, configurable, _)| configurable)
                .or(own_flags.map(|(_, configurable, _)| configurable)))
            .unwrap_or(true);
        let callable_data_property = callable_virtual_descriptor.is_none()
            && object_backed_value.is_some()
            && self.callable_function_info(value).is_some()
            && self.callable_function_info(target).is_some();
        let enumerable_flag = resolved_data_shape
            .map(|shape| shape.enumerable)
            .or(callable_virtual_descriptor
                .or(legacy_error_descriptor)
                .map(|(_, _, enumerable)| enumerable)
                .or(own_flags.map(|(_, _, enumerable)| enumerable)))
            .unwrap_or_else(|| {
                if callable_data_property {
                    false
                } else {
                    object_backed_value.is_some()
                }
            });
        self.set_internal_descriptor_field(descriptor, "value", value)?;
        self.set_internal_descriptor_field(descriptor, "writable", Value::bool(writable_flag))?;
        self.set_internal_descriptor_field(
            descriptor,
            "configurable",
            Value::bool(configurable_flag),
        )?;
        self.set_internal_descriptor_field(descriptor, "enumerable", Value::bool(enumerable_flag))?;

        Ok(Some(descriptor))
    }

    fn synthesize_accessor_property_descriptor(
        &self,
        target: Value,
        key: &str,
    ) -> Result<Option<Value>, VmError> {
        let getter = self.callable_virtual_accessor_value(target, key, "get");
        let setter = self.callable_virtual_accessor_value(target, key, "set");
        if getter.is_none() && setter.is_none() {
            return Ok(None);
        }

        let descriptor = self.alloc_object_descriptor()?;
        self.set_internal_descriptor_field(descriptor, "configurable", Value::bool(true))?;
        self.set_internal_descriptor_field(descriptor, "enumerable", Value::bool(false))?;
        self.set_internal_descriptor_field(
            descriptor,
            "get",
            getter.unwrap_or(Value::undefined()),
        )?;
        self.set_internal_descriptor_field(
            descriptor,
            "set",
            setter.unwrap_or(Value::undefined()),
        )?;

        Ok(Some(descriptor))
    }

    /// Synthesize a descriptor object from a `DynProp` stored in the property kernel.
    fn synthesize_descriptor_from_dyn_prop(&self, prop: &DynProp) -> Result<Value, VmError> {
        let descriptor = self.alloc_object_descriptor()?;
        let Some(descriptor_ptr) = (unsafe { descriptor.as_ptr::<Object>() }) else {
            return Err(VmError::RuntimeError(
                "Failed to allocate property descriptor object".to_string(),
            ));
        };
        let descriptor_obj = unsafe { &mut *descriptor_ptr.as_ptr() };

        if prop.is_accessor {
            // Accessor descriptor: get, set, enumerable, configurable
            self.set_internal_descriptor_field(descriptor, "get", prop.get)?;
            self.set_internal_descriptor_field(descriptor, "set", prop.set)?;
        } else {
            // Data descriptor: value, writable, enumerable, configurable
            self.set_internal_descriptor_field(descriptor, "value", prop.value)?;
            self.set_internal_descriptor_field(descriptor, "writable", Value::bool(prop.writable))?;
        }

        self.set_internal_descriptor_field(descriptor, "enumerable", Value::bool(prop.enumerable))?;
        self.set_internal_descriptor_field(
            descriptor,
            "configurable",
            Value::bool(prop.configurable),
        )?;

        Ok(descriptor)
    }

    /// Synthesize a descriptor object from `SlotMeta` and its corresponding value.
    fn synthesize_descriptor_from_slot_meta(
        &self,
        meta: &SlotMeta,
        value: Value,
    ) -> Result<Value, VmError> {
        let descriptor = self.alloc_object_descriptor()?;
        let Some(descriptor_ptr) = (unsafe { descriptor.as_ptr::<Object>() }) else {
            return Err(VmError::RuntimeError(
                "Failed to allocate property descriptor object".to_string(),
            ));
        };
        let descriptor_obj = unsafe { &mut *descriptor_ptr.as_ptr() };

        if let Some(ref accessor) = meta.accessor {
            // Accessor descriptor
            if let Some(idx) = self.get_field_index_for_value(descriptor, "get") {
                descriptor_obj
                    .set_field(idx, accessor.get)
                    .map_err(VmError::RuntimeError)?;
            }
            self.set_descriptor_field_present(descriptor, "get", true);
            if let Some(idx) = self.get_field_index_for_value(descriptor, "set") {
                descriptor_obj
                    .set_field(idx, accessor.set)
                    .map_err(VmError::RuntimeError)?;
            }
            self.set_descriptor_field_present(descriptor, "set", true);
            if let Some(idx) = self.get_field_index_for_value(descriptor, "value") {
                descriptor_obj
                    .set_field(idx, Value::undefined())
                    .map_err(VmError::RuntimeError)?;
            }
            if let Some(idx) = self.get_field_index_for_value(descriptor, "writable") {
                descriptor_obj
                    .set_field(idx, Value::undefined())
                    .map_err(VmError::RuntimeError)?;
            }
        } else {
            // Data descriptor
            if let Some(idx) = self.get_field_index_for_value(descriptor, "value") {
                descriptor_obj
                    .set_field(idx, value)
                    .map_err(VmError::RuntimeError)?;
            }
            self.set_descriptor_field_present(descriptor, "value", true);
            if let Some(idx) = self.get_field_index_for_value(descriptor, "writable") {
                descriptor_obj
                    .set_field(idx, Value::bool(meta.writable))
                    .map_err(VmError::RuntimeError)?;
            }
            self.set_descriptor_field_present(descriptor, "writable", true);
        }

        if let Some(idx) = self.get_field_index_for_value(descriptor, "enumerable") {
            descriptor_obj
                .set_field(idx, Value::bool(meta.enumerable))
                .map_err(VmError::RuntimeError)?;
        }
        self.set_descriptor_field_present(descriptor, "enumerable", true);
        if let Some(idx) = self.get_field_index_for_value(descriptor, "configurable") {
            descriptor_obj
                .set_field(idx, Value::bool(meta.configurable))
                .map_err(VmError::RuntimeError)?;
        }
        self.set_descriptor_field_present(descriptor, "configurable", true);

        Ok(descriptor)
    }

    fn legacy_error_field_descriptor(
        &self,
        target: Value,
        key: &str,
    ) -> Option<(bool, bool, bool)> {
        let obj_ptr = checked_object_ptr(target)?;
        let obj = unsafe { &*obj_ptr.as_ptr() };
        let nominal_class_name = obj.nominal_type_id_usize().and_then(|nominal_type_id| {
            let classes = self.classes.read();
            classes
                .get_class(nominal_type_id)
                .map(|class| class.name.clone())
        });
        let field_names = self.layout_field_names_for_object(obj).unwrap_or_default();
        let is_error_like = nominal_class_name.as_deref().is_some_and(|name| {
            matches!(
                name,
                "Error"
                    | "TypeError"
                    | "RangeError"
                    | "ReferenceError"
                    | "SyntaxError"
                    | "URIError"
                    | "EvalError"
                    | "InternalError"
                    | "AggregateError"
                    | "SuppressedError"
                    | "ChannelClosedError"
                    | "AssertionError"
            )
        }) || (field_names.iter().any(|name| name == "message")
            && field_names.iter().any(|name| name == "name"));
        if !is_error_like {
            return None;
        }

        match key {
            "message" | "name" | "stack" | "cause" | "code" | "errno" | "syscall" | "path" => {
                Some((true, true, false))
            }
            "errors"
                if nominal_class_name.as_deref() == Some("AggregateError")
                    || field_names.iter().any(|name| name == "errors") =>
            {
                Some((true, true, false))
            }
            _ => None,
        }
    }

    pub(in crate::vm::interpreter) fn exec_native_ops(
        &mut self,
        stack: &mut Stack,
        ip: &mut usize,
        code: &[u8],
        module: &Module,
        task: &Arc<Task>,
        opcode: Opcode,
    ) -> OpcodeResult {
        match opcode {
            Opcode::NativeCall => {
                let native_id = match Self::read_u16(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arg_count = match Self::read_u8(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let debug_native_stack = std::env::var("RAYA_DEBUG_NATIVE_STACK").is_ok();
                if debug_native_stack {
                    let func_id = task.current_func_id();
                    let func_name = module
                        .functions
                        .get(func_id)
                        .map(|f| f.name.as_str())
                        .unwrap_or("<unknown>");
                    eprintln!(
                        "[native] enter {}#{} native_id={} arg_count={} stack_depth={}",
                        func_name,
                        func_id,
                        native_id,
                        arg_count,
                        stack.depth()
                    );
                }

                // Pop arguments
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    match stack.pop() {
                        Ok(v) => args.push(v),
                        Err(e) => {
                            if debug_native_stack {
                                let func_id = task.current_func_id();
                                let func_name = module
                                    .functions
                                    .get(func_id)
                                    .map(|f| f.name.as_str())
                                    .unwrap_or("<unknown>");
                                eprintln!(
                                    "[native] pop-underflow {}#{} native_id={} arg_count={} stack_depth={}",
                                    func_name,
                                    func_id,
                                    native_id,
                                    arg_count,
                                    stack.depth()
                                );
                            }
                            return OpcodeResult::Error(e);
                        }
                    }
                }
                args.reverse();

                // Route builtin array native IDs through shared array handler.
                // Native array calls use args = [receiver, ...methodArgs].
                if crate::vm::builtin::is_array_method(native_id) {
                    if args.is_empty() {
                        return OpcodeResult::Error(VmError::RuntimeError(
                            "Array native call requires receiver".to_string(),
                        ));
                    }
                    for arg in &args {
                        if let Err(e) = stack.push(*arg) {
                            return OpcodeResult::Error(e);
                        }
                    }
                    let method_arg_count = args.len().saturating_sub(1);
                    return match self.call_array_method(
                        task,
                        stack,
                        native_id,
                        method_arg_count,
                        module,
                    ) {
                        Ok(()) => OpcodeResult::Continue,
                        Err(e) => OpcodeResult::Error(e),
                    };
                }

                // Route builtin string native IDs through shared string handler.
                if crate::vm::builtin::is_string_method(native_id) {
                    if args.is_empty() {
                        return OpcodeResult::Error(VmError::RuntimeError(
                            "String native call requires receiver".to_string(),
                        ));
                    }
                    for arg in &args {
                        if let Err(e) = stack.push(*arg) {
                            return OpcodeResult::Error(e);
                        }
                    }
                    let method_arg_count = args.len().saturating_sub(1);
                    return match self.call_string_method(
                        task,
                        stack,
                        native_id,
                        method_arg_count,
                        module,
                    ) {
                        Ok(()) => OpcodeResult::Continue,
                        Err(e) => OpcodeResult::Error(e),
                    };
                }

                // Execute native call - handle channel operations specially for suspension
                match native_id {
                    id if id == crate::compiler::native_id::OBJECT_NEW => {
                        let value = match self.alloc_plain_object() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_DESCRIPTOR_NEW => {
                        let value = match self.alloc_object_descriptor() {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_WELL_KNOWN_SYMBOL => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.wellKnownSymbol requires 1 argument".to_string(),
                            ));
                        }
                        let Some(name_ptr) = (unsafe { args[0].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.wellKnownSymbol expects a string key".to_string(),
                            ));
                        };
                        let name = unsafe { &*name_ptr.as_ptr() }.data.clone();
                        let value = match self.alloc_symbol_object(&name) {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_GET_AMBIENT_GLOBAL => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "ambient global lookup expects exactly one string argument"
                                    .to_string(),
                            ));
                        }
                        let Some(name_ptr) = (unsafe { args[0].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "ambient global lookup expects a string name".to_string(),
                            ));
                        };
                        let name = unsafe { &*name_ptr.as_ptr() };
                        match self.activation_eval_env_get(task, module, &name.data) {
                            Ok(Some(value)) => {
                                if let Err(e) = stack.push(value) {
                                    return OpcodeResult::Error(e);
                                }
                                return OpcodeResult::Continue;
                            }
                            Ok(None) => {}
                            Err(error) => return OpcodeResult::Error(error),
                        }
                        let value = match self.ensure_builtin_global_value(&name.data, task) {
                            Ok(Some(v)) => Some(v),
                            Ok(None) => match self.shared_js_global_binding_value(&name.data) {
                                Ok(Some(v)) => Some(v),
                                Ok(None) => {
                                    let class_constructor =
                                        self.classes.read().get_class_by_name(&name.data).and_then(
                                            |class| {
                                                self.constructor_value_for_nominal_type(class.id)
                                            },
                                        );
                                    if let Some(v) = class_constructor {
                                        Some(v)
                                    } else {
                                        match self.ensure_builtin_global_value("globalThis", task) {
                                            Ok(Some(gt)) => self
                                                .get_property_value_via_js_semantics_with_context(
                                                    gt, &name.data, task, module,
                                                )
                                                .ok()
                                                .flatten(),
                                            Ok(None) => None,
                                            Err(error) => return OpcodeResult::Error(error),
                                        }
                                    }
                                }
                                Err(error) => return OpcodeResult::Error(error),
                            },
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        let Some(value) = value else {
                            return OpcodeResult::Error(
                                self.raise_unresolved_identifier_error(task, &name.data),
                            );
                        };
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_THROW_REFERENCE_ERROR => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Object.throwReferenceError expects exactly one string argument"
                                    .to_string(),
                            ));
                        }
                        let Some(name_ptr) = (unsafe { args[0].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.throwReferenceError expects a string name".to_string(),
                            ));
                        };
                        let name = unsafe { &*name_ptr.as_ptr() };
                        return OpcodeResult::Error(self.raise_task_builtin_error(
                            task,
                            "ReferenceError",
                            format!("{} is not defined", name.data),
                        ));
                    }

                    id if id == crate::compiler::native_id::OBJECT_CREATE_REFERENCE_ERROR => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Object.createReferenceError expects exactly one string argument"
                                    .to_string(),
                            ));
                        }
                        let Some(name_ptr) = (unsafe { args[0].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.createReferenceError expects a string name".to_string(),
                            ));
                        };
                        let name = unsafe { &*name_ptr.as_ptr() };
                        let exception = self.alloc_builtin_error_value(
                            "ReferenceError",
                            &format!("{} is not defined", name.data),
                        );
                        if let Err(error) = stack.push(exception) {
                            return OpcodeResult::Error(error);
                        }
                        return OpcodeResult::Continue;
                    }

                    id if id == crate::compiler::native_id::OBJECT_THROW_TYPE_ERROR => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Object.throwTypeError expects exactly one string argument"
                                    .to_string(),
                            ));
                        }
                        let Some(message_ptr) = (unsafe { args[0].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.throwTypeError expects a string message".to_string(),
                            ));
                        };
                        let message = unsafe { &*message_ptr.as_ptr() };
                        return OpcodeResult::Error(self.raise_task_builtin_error(
                            task,
                            "TypeError",
                            message.data.clone(),
                        ));
                    }

                    id if id == crate::compiler::native_id::OBJECT_CREATE_TYPE_ERROR => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Object.createTypeError expects exactly one string argument"
                                    .to_string(),
                            ));
                        }
                        let Some(message_ptr) = (unsafe { args[0].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.createTypeError expects a string message".to_string(),
                            ));
                        };
                        let message = unsafe { &*message_ptr.as_ptr() };
                        let exception =
                            self.alloc_builtin_error_value("TypeError", &message.data);
                        if let Err(error) = stack.push(exception) {
                            return OpcodeResult::Error(error);
                        }
                        return OpcodeResult::Continue;
                    }

                    id if id == crate::compiler::native_id::TRY_GET_GLOBAL => {
                        // Non-throwing global lookup: returns value or undefined.
                        // Checks builtin_global_slots, then globalThis properties.
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "tryGetGlobal expects exactly one string argument".to_string(),
                            ));
                        }
                        let Some(name_ptr) = (unsafe { args[0].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "tryGetGlobal expects a string name".to_string(),
                            ));
                        };
                        let name = unsafe { &*name_ptr.as_ptr() };
                        // 1. Check builtin_global_slots (builtins + module exports)
                        let value = match self.activation_eval_env_get(task, module, &name.data) {
                            Ok(Some(v)) => Some(v),
                            Ok(None) => match self.shared_js_global_binding_value(&name.data) {
                                Ok(value) => match value {
                                    Some(v) => Some(v),
                                    None => {
                                        match self.ensure_builtin_global_value(&name.data, task) {
                                            Ok(value) => value,
                                            Err(error) => return OpcodeResult::Error(error),
                                        }
                                    }
                                },
                                Err(error) => return OpcodeResult::Error(error),
                            },
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        let result = if let Some(v) = value {
                            v
                        } else if let Ok(Some(gt)) =
                            self.ensure_builtin_global_value("globalThis", task)
                        {
                            // 2. Check globalThis properties (script-level var bindings)
                            let prop = self.get_property_value_via_js_semantics_with_context(
                                gt, &name.data, task, module,
                            );
                            if std::env::var("RAYA_DEBUG_TRY_GET_GLOBAL").is_ok() {
                                eprintln!(
                                    "[try-get-global] name='{}' globalThis={:#x} prop={:?}",
                                    name.data,
                                    gt.raw(),
                                    prop.as_ref().map(|p| p.is_some()),
                                );
                            }
                            match prop {
                                Ok(Some(v)) => v,
                                _ => Value::undefined(),
                            }
                        } else {
                            Value::undefined()
                        };
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_EVAL_ENV_GET => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "eval env get expects exactly one string argument".to_string(),
                            ));
                        }
                        let Some(name_ptr) = (unsafe { args[0].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "eval env get expects a string name".to_string(),
                            ));
                        };
                        let name = unsafe { &*name_ptr.as_ptr() };
                        let value = match self.activation_eval_env_get(task, module, &name.data) {
                            Ok(Some(v)) => v,
                            Ok(None) => {
                                let name_reg = name.data.clone();
                                if let Some(v) =
                                    match self.shared_js_global_binding_value(&name_reg) {
                                        Ok(value) => value,
                                        Err(error) => return OpcodeResult::Error(error),
                                    }
                                {
                                    v
                                } else if let Ok(Some(v)) =
                                    self.ensure_builtin_global_value(&name_reg, task)
                                {
                                    v
                                } else if let Ok(Some(gt)) =
                                    self.ensure_builtin_global_value("globalThis", task)
                                {
                                    match self.get_property_value_via_js_semantics_with_context(
                                        gt, &name.data, task, module,
                                    ) {
                                        Ok(Some(v)) => v,
                                        Ok(None) => {
                                            return OpcodeResult::Error(
                                                self.raise_unresolved_identifier_error(
                                                    task, &name.data,
                                                ),
                                            )
                                        }
                                        Err(error) => return OpcodeResult::Error(error),
                                    }
                                } else {
                                    return OpcodeResult::Error(
                                        self.raise_unresolved_identifier_error(task, &name.data),
                                    );
                                }
                            }
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if let Err(error) = stack.push(value) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_EVAL_ENV_TRY_GET => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "eval env tryGet expects exactly one string argument".to_string(),
                            ));
                        }
                        let Some(name_ptr) = (unsafe { args[0].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "eval env tryGet expects a string name".to_string(),
                            ));
                        };
                        let name = unsafe { &*name_ptr.as_ptr() };
                        let value = match self.activation_eval_env_get(task, module, &name.data) {
                            Ok(Some(v)) => v,
                            Ok(None) => {
                                let name_reg = name.data.clone();
                                match self.shared_js_global_binding_value(&name_reg) {
                                    Ok(Some(v)) => v,
                                    Ok(None) => self
                                        .ensure_builtin_global_value(&name_reg, task)
                                        .ok()
                                        .flatten()
                                        .or_else(|| {
                                            self.ensure_builtin_global_value("globalThis", task)
                                                .ok()
                                                .flatten()
                                                .and_then(|gt| {
                                                    self.get_property_value_via_js_semantics_with_context(
                                                        gt,
                                                        &name.data,
                                                        task,
                                                        module,
                                                    )
                                                    .ok()
                                                    .flatten()
                                                })
                                        })
                                        .unwrap_or(Value::undefined()),
                                    Err(error) => return OpcodeResult::Error(error),
                                }
                            }
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if let Err(error) = stack.push(value) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_EVAL_ENV_SET => {
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "eval env set expects name and value".to_string(),
                            ));
                        }
                        let Some(name_ptr) = (unsafe { args[0].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "eval env set expects a string name".to_string(),
                            ));
                        };
                        let name = unsafe { &*name_ptr.as_ptr() };
                        match self.activation_eval_env_set(task, module, &name.data, args[1]) {
                            Ok(true) => {}
                            Ok(false) => {
                                if let Ok(true) = self.set_shared_js_global_binding_value(
                                    &name.data, args[1], task, module,
                                ) {
                                    if let Err(error) = stack.push(args[1]) {
                                        return OpcodeResult::Error(error);
                                    }
                                    return OpcodeResult::Continue;
                                }
                                let has_global_binding = self
                                    .ensure_builtin_global_value("globalThis", task)
                                    .ok()
                                    .flatten()
                                    .is_some_and(|global_this| {
                                        self.has_property_via_js_semantics(global_this, &name.data)
                                    });
                                if self.current_function_is_strict_js(task, module)
                                    && !has_global_binding
                                {
                                    return OpcodeResult::Error(self.raise_task_builtin_error(
                                        task,
                                        "ReferenceError",
                                        format!("{} is not defined", name.data),
                                    ));
                                }
                                if let Err(error) = self.bind_script_global_property(
                                    &name.data, args[1], false, task, module,
                                ) {
                                    return OpcodeResult::Error(error);
                                }
                            }
                            Err(error) => return OpcodeResult::Error(error),
                        }
                        if let Err(error) = stack.push(args[1]) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_EVAL_ENV_DECLARE_VAR => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "eval env declareVar expects a string name".to_string(),
                            ));
                        }
                        let Some(name_ptr) = (unsafe { args[0].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "eval env declareVar expects a string name".to_string(),
                            ));
                        };
                        let name = unsafe { &*name_ptr.as_ptr() };
                        if let Err(error) =
                            self.activation_eval_env_declare_var(task, module, &name.data)
                        {
                            return OpcodeResult::Error(error);
                        }
                        if let Err(error) = stack.push(Value::undefined()) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_EVAL_ENV_DECLARE_FUNCTION => {
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "eval env declareFunction expects name and value".to_string(),
                            ));
                        }
                        let Some(name_ptr) = (unsafe { args[0].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "eval env declareFunction expects a string name".to_string(),
                            ));
                        };
                        let name = unsafe { &*name_ptr.as_ptr() };
                        let Some(env) = self.current_activation_eval_env(task) else {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "No active eval environment".to_string(),
                            ));
                        };
                        if !self.current_function_is_strict_js(task, module)
                            && self.direct_eval_chain_has_lexical_binding(env, &name.data)
                        {
                            return OpcodeResult::Error(self.raise_task_builtin_error(
                                task,
                                "SyntaxError",
                                format!(
                                    "direct eval cannot declare function '{}' over an existing lexical binding",
                                    name.data
                                ),
                            ));
                        }
                        let outer_has_binding =
                            self.direct_eval_outer_env(env).is_some_and(|outer_env| {
                                self.resolve_own_property_shape(outer_env, &name.data)
                                    .is_some()
                            });
                        let use_outer_binding = self
                            .active_direct_eval_persist_caller_declarations(task)
                            && outer_has_binding;
                        if !use_outer_binding {
                            if let Err(error) = self.activation_eval_env_create_mutable_binding(
                                task,
                                module,
                                &name.data,
                                args[1],
                            ) {
                                return OpcodeResult::Error(error);
                            }
                        }
                        match self.activation_eval_env_set(task, module, &name.data, args[1]) {
                            Ok(true) => {}
                            Ok(false) => {
                                return OpcodeResult::Error(VmError::RuntimeError(
                                    "No active eval environment".to_string(),
                                ))
                            }
                            Err(error) => return OpcodeResult::Error(error),
                        }
                        if self.active_direct_eval_uses_script_global_bindings(task) {
                            if let Err(error) = self
                                .bind_direct_eval_global_function(&name.data, args[1], task, module)
                            {
                                return OpcodeResult::Error(error);
                            }
                        }
                        if let Err(error) = stack.push(args[1]) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_EVAL_ENV_DECLARE_LEXICAL => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "eval env declareLexical expects a string name".to_string(),
                            ));
                        }
                        let Some(name_ptr) = (unsafe { args[0].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "eval env declareLexical expects a string name".to_string(),
                            ));
                        };
                        let name = unsafe { &*name_ptr.as_ptr() };
                        if let Err(error) =
                            self.activation_eval_env_declare_lexical(task, &name.data)
                        {
                            return OpcodeResult::Error(error);
                        }
                        if let Err(error) = stack.push(Value::undefined()) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_EVAL_SET_COMPLETION => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "eval setCompletion expects exactly one value".to_string(),
                            ));
                        }
                        let Some(env) = self.current_activation_eval_env(task) else {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "No active eval completion context".to_string(),
                            ));
                        };
                        if std::env::var("RAYA_DEBUG_DIRECT_EVAL_COMPLETION").is_ok() {
                            eprintln!(
                                "[eval-completion:set] env={:#x} value={:#x} string={:?}",
                                env.raw(),
                                args[0].raw(),
                                primitive_to_js_string(args[0]),
                            );
                        }
                        if let Err(error) = self.set_direct_eval_completion(env, args[0]) {
                            return OpcodeResult::Error(error);
                        }
                        if let Err(error) = stack.push(args[0]) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_EVAL_GET_COMPLETION => {
                        let env = self.current_activation_eval_env(task);
                        let value = env
                            .and_then(|env| self.direct_eval_completion(env))
                            .unwrap_or(Value::undefined());
                        if std::env::var("RAYA_DEBUG_DIRECT_EVAL_COMPLETION").is_ok() {
                            eprintln!(
                                "[eval-completion:get] env={:?} value={:#x} string={:?}",
                                env.map(|v| format!("{:#x}", v.raw())),
                                value.raw(),
                                primitive_to_js_string(value),
                            );
                        }
                        if let Err(error) = stack.push(value) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_CURRENT_NEW_TARGET => {
                        let value = self
                            .current_js_new_target(task)
                            .unwrap_or(Value::undefined());
                        if let Err(error) = stack.push(value) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_SET_CALLABLE_HOME_OBJECT => {
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "setCallableHomeObject expects callable and homeObject".to_string(),
                            ));
                        }
                        let Some(callable_ptr) = (unsafe { args[0].as_ptr::<Object>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "setCallableHomeObject expects a callable object".to_string(),
                            ));
                        };
                        let callable = unsafe { &mut *callable_ptr.as_ptr() };
                        if let Err(error) = callable.set_callable_home_object(args[1]) {
                            return OpcodeResult::Error(VmError::RuntimeError(error));
                        }
                        if let Err(error) = stack.push(args[0]) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_GET_SUPER_PROPERTY => {
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "getSuperProperty expects receiver and property name".to_string(),
                            ));
                        }
                        let Some(name_ptr) = (unsafe { args[1].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "getSuperProperty expects a string property name".to_string(),
                            ));
                        };
                        let name = unsafe { &*name_ptr.as_ptr() };
                        let Some(home_object) = self.current_js_home_object(task) else {
                            return OpcodeResult::Error(self.raise_task_builtin_error(
                                task,
                                "ReferenceError",
                                "`super` is not available in this context".to_string(),
                            ));
                        };
                        let Some(base) = self.prototype_of_value(home_object) else {
                            if let Err(error) = stack.push(Value::undefined()) {
                                return OpcodeResult::Error(error);
                            }
                            return OpcodeResult::Continue;
                        };
                        let value = match self
                            .get_property_value_on_receiver_via_js_semantics_with_context(
                                base, &name.data, args[0], task, module,
                            ) {
                            Ok(Some(value)) => value,
                            Ok(None) => Value::undefined(),
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if let Err(error) = stack.push(value) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_PUSH_WITH_ENV => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "pushWithEnv expects exactly one object".to_string(),
                            ));
                        }
                        let outer_env = self.current_activation_eval_env(task);
                        let env = match self.alloc_with_runtime_env(args[0], outer_env) {
                            Ok(env) => env,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        task.push_active_direct_eval_env(env, false, false, false);
                        if let Err(error) = stack.push(args[0]) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_POP_WITH_ENV => {
                        let _ = task.pop_active_direct_eval_env();
                        if let Err(error) = stack.push(Value::undefined()) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_JS_DELETE_IDENTIFIER => {
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "jsDeleteIdentifier expects name and resolvedLocally".to_string(),
                            ));
                        }
                        let Some(name_ptr) = (unsafe { args[0].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "jsDeleteIdentifier expects a string name".to_string(),
                            ));
                        };
                        let name = unsafe { &*name_ptr.as_ptr() };
                        let deleted = match self.delete_js_identifier_reference(
                            task,
                            module,
                            &name.data,
                            args[1].is_truthy(),
                        ) {
                            Ok(deleted) => deleted,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if let Err(error) = stack.push(Value::bool(deleted)) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_EVAL_ENV_HAS => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "eval env has expects a string name".to_string(),
                            ));
                        }
                        let Some(name_ptr) = (unsafe { args[0].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "eval env has expects a string name".to_string(),
                            ));
                        };
                        let name = unsafe { &*name_ptr.as_ptr() };
                        if let Err(error) =
                            stack.push(Value::bool(self.activation_eval_env_has(task, &name.data)))
                        {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id
                        == crate::compiler::native_id::OBJECT_ENSURE_ACTIVATION_EVAL_ENV =>
                    {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "ensureActivationEvalEnv expects exactly one env object"
                                    .to_string(),
                            ));
                        }
                        let env = task
                            .current_activation_direct_eval_env()
                            .unwrap_or_else(|| {
                                task.set_activation_direct_eval_env(
                                    task.current_func_id(),
                                    task.current_locals_base(),
                                    args[0],
                                );
                                args[0]
                            });
                        if let Err(error) = stack.push(env) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_SET_AMBIENT_GLOBAL => {
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "setAmbientGlobal expects name and value".to_string(),
                            ));
                        }
                        let Some(name_ptr) = (unsafe { args[0].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "setAmbientGlobal expects a string name".to_string(),
                            ));
                        };
                        let name = unsafe { &*name_ptr.as_ptr() };
                        if let Err(error) =
                            self.assign_ambient_identifier_value(task, module, &name.data, args[1])
                        {
                            return OpcodeResult::Error(error);
                        }
                        if let Err(error) = stack.push(args[1]) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_SET_PROPERTY
                        || id == crate::compiler::native_id::OBJECT_SET_PROPERTY_STRICT =>
                    {
                        if args.len() != 3 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "setProperty expects target, key, and value".to_string(),
                            ));
                        }
                        let (Some(key_str), _) = (match self.property_key_parts_with_context(
                            args[1],
                            "Object.setProperty",
                            task,
                            module,
                        ) {
                            Ok(parts) => parts,
                            Err(error) => return OpcodeResult::Error(error),
                        }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Cannot convert property key to string".to_string(),
                            ));
                        };
                        let target = self.proxy_wrapper_proxy_value(args[0]).unwrap_or(args[0]);
                        if target.is_null() || target.is_undefined() {
                            return OpcodeResult::Error(VmError::TypeError(format!(
                                "Cannot set property '{}' of null or undefined",
                                key_str
                            )));
                        }
                        let written = match self.set_property_value_via_js_semantics(
                            target, &key_str, args[2], target, task, module,
                        ) {
                            Ok(written) => written,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if !written
                            && id == crate::compiler::native_id::OBJECT_SET_PROPERTY_STRICT
                        {
                            return OpcodeResult::Error(VmError::TypeError(format!(
                                "Cannot assign to non-writable property '{}'",
                                key_str
                            )));
                        }
                        if let Err(error) = stack.push(args[2]) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_SET_SUPER_PROPERTY
                        || id == crate::compiler::native_id::OBJECT_SET_SUPER_PROPERTY_STRICT =>
                    {
                        if args.len() != 3 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "setSuperProperty expects receiver, key, and value".to_string(),
                            ));
                        }
                        let (Some(key_str), _) = (match self.property_key_parts_with_context(
                            args[1],
                            "Object.setSuperProperty",
                            task,
                            module,
                        ) {
                            Ok(parts) => parts,
                            Err(error) => return OpcodeResult::Error(error),
                        }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Cannot convert property key to string".to_string(),
                            ));
                        };
                        let Some(home_object) = self.current_js_home_object(task) else {
                            return OpcodeResult::Error(self.replace_task_builtin_error(
                                task,
                                "ReferenceError",
                                "`super` is not available in this context".to_string(),
                            ));
                        };
                        let Some(base) = self.prototype_of_value(home_object) else {
                            return OpcodeResult::Error(self.replace_task_builtin_error(
                                task,
                                "TypeError",
                                "Cannot assign to property on null prototype".to_string(),
                            ));
                        };
                        if base.is_null() || base.is_undefined() {
                            return OpcodeResult::Error(self.replace_task_builtin_error(
                                task,
                                "TypeError",
                                "Cannot assign to property on null prototype".to_string(),
                            ));
                        }
                        let receiver = self.proxy_wrapper_proxy_value(args[0]).unwrap_or(args[0]);
                        let written = match self.set_property_value_via_js_semantics(
                            base, &key_str, args[2], receiver, task, module,
                        ) {
                            Ok(written) => written,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if !written
                            && id == crate::compiler::native_id::OBJECT_SET_SUPER_PROPERTY_STRICT
                        {
                            return OpcodeResult::Error(self.replace_task_builtin_error(
                                task,
                                "TypeError",
                                format!("Cannot assign to non-writable property '{}'", key_str),
                            ));
                        }
                        if let Err(error) = stack.push(args[2]) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id
                        == crate::compiler::native_id::OBJECT_CAPTURE_IDENTIFIER_ASSIGNMENT_TARGET =>
                    {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "captureIdentifierAssignmentTarget expects a string name"
                                    .to_string(),
                            ));
                        }
                        let Some(name_ptr) = (unsafe { args[0].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "captureIdentifierAssignmentTarget expects a string name"
                                    .to_string(),
                            ));
                        };
                        let name = unsafe { &*name_ptr.as_ptr() };
                        let target = match self.capture_activation_identifier_assignment_target(
                            task,
                            module,
                            &name.data,
                        ) {
                            Ok(target) => target.unwrap_or(Value::null()),
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if let Err(error) = stack.push(target) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id
                        == crate::compiler::native_id::OBJECT_STORE_IDENTIFIER_ASSIGNMENT_TARGET =>
                    {
                        if args.len() != 3 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "storeIdentifierAssignmentTarget expects target, name, and value"
                                    .to_string(),
                            ));
                        }
                        let Some(name_ptr) = (unsafe { args[1].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "storeIdentifierAssignmentTarget expects a string name"
                                    .to_string(),
                            ));
                        };
                        let name = unsafe { &*name_ptr.as_ptr() };
                        let target = if args[0].is_null() || args[0].is_undefined() {
                            None
                        } else {
                            Some(args[0])
                        };
                        if let Err(error) = self.store_identifier_assignment_target(
                            task,
                            module,
                            target,
                            &name.data,
                            args[2],
                        ) {
                            return OpcodeResult::Error(error);
                        }
                        if let Err(error) = stack.push(args[2]) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_HAS_PROPERTY => {
                        // ES `key in obj` — HasProperty on prototype chain
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "in operator requires 2 arguments".to_string(),
                            ));
                        }
                        let obj = args[1];
                        if !self.is_js_object_value(obj) {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Cannot use 'in' operator to search for a property in a non-object"
                                    .to_string(),
                            ));
                        }
                        let (Some(key_str), _) = (match self.property_key_parts_with_context(
                            args[0],
                            "Object.hasProperty",
                            task,
                            module,
                        ) {
                            Ok(parts) => parts,
                            Err(error) => return OpcodeResult::Error(error),
                        }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Cannot convert property key to string".to_string(),
                            ));
                        };
                        let result = match self.has_property_via_js_semantics_with_context(
                            obj, &key_str, task, module,
                        ) {
                            Ok(result) => result,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if let Err(e) = stack.push(Value::bool(result)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_BIND_SCRIPT_GLOBAL => {
                        if args.len() != 2 && args.len() != 3 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "script global binding expects name, value, and optional configurability"
                                    .to_string(),
                            ));
                        }
                        let Some(name_ptr) = (unsafe { args[0].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "script global binding expects a string name".to_string(),
                            ));
                        };
                        let name = unsafe { &*name_ptr.as_ptr() };
                        let configurable =
                            args.get(2).copied().is_some_and(|value| value.is_truthy());
                        if let Err(error) = self.bind_script_global_property(
                            &name.data,
                            args[1],
                            configurable,
                            task,
                            module,
                        ) {
                            return OpcodeResult::Error(error);
                        }
                        if let Err(e) = stack.push(args[1]) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_CALL_CONSTRUCTOR_BY_NAME => {
                        if args.len() < 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "parent constructor helper expects `this`, class name, and optional args"
                                    .to_string(),
                            ));
                        }
                        let this_arg = args[0];
                        let Some(name_ptr) = (unsafe { args[1].as_ptr::<RayaString>() }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "parent constructor helper expects a string class name".to_string(),
                            ));
                        };
                        let class_name = unsafe { &*name_ptr.as_ptr() }.data.clone();
                        if std::env::var("RAYA_DEBUG_SUPER_BY_NAME").is_ok() {
                            let preview = args[2..]
                                .iter()
                                .take(4)
                                .map(|value| format!("{:#x}", value.raw()))
                                .collect::<Vec<_>>()
                                .join(", ");
                            eprintln!(
                                "[super.by-name] class={} argc={} this={:#x} args=[{}]",
                                class_name,
                                args.len().saturating_sub(2),
                                this_arg.raw(),
                                preview
                            );
                        }
                        let (constructor_id, constructor_module) = {
                            let classes = self.classes.read();
                            let Some(class) = classes.get_class_by_name(&class_name) else {
                                return OpcodeResult::Error(VmError::RuntimeError(format!(
                                    "Parent class '{}' not found",
                                    class_name
                                )));
                            };
                            (class.get_constructor(), class.module.clone())
                        };
                        if let Some(constructor_id) = constructor_id {
                            let closure = if let Some(module) = constructor_module {
                                Object::new_closure_with_module(constructor_id, Vec::new(), module)
                            } else {
                                Object::new_closure(constructor_id, Vec::new())
                            };
                            let closure_ptr = self.gc.lock().allocate(closure);
                            let closure_val = unsafe {
                                Value::from_ptr(
                                    std::ptr::NonNull::new(closure_ptr.as_ptr())
                                        .expect("parent constructor closure ptr"),
                                )
                            };
                            self.ephemeral_gc_roots.write().push(closure_val);
                            let invoke_args = args[2..].to_vec();
                            let invoke_result = self.invoke_callable_sync_with_this(
                                closure_val,
                                Some(this_arg),
                                &invoke_args,
                                task,
                                module,
                            );
                            {
                                let mut ephemeral = self.ephemeral_gc_roots.write();
                                if let Some(index) = ephemeral
                                    .iter()
                                    .rposition(|candidate| *candidate == closure_val)
                                {
                                    ephemeral.swap_remove(index);
                                }
                            }
                            if let Err(error) = invoke_result {
                                return OpcodeResult::Error(error);
                            }
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_SUPER_CONSTRUCT => {
                        if args.len() < 3 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "super construct expects receiver, parent constructor, newTarget, and optional args"
                                    .to_string(),
                            ));
                        }
                        let value = match self
                            .construct_value_with_existing_receiver_and_new_target(
                                args[0],
                                args[1],
                                args[2],
                                &args[3..],
                                task,
                                module,
                            ) {
                            Ok(value) => value,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if let Err(error) = stack.push(value) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_JS_ADD => {
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Object.jsAdd expects exactly two arguments".to_string(),
                            ));
                        }
                        let value = match self.js_add_with_context(args[0], args[1], task, module) {
                            Ok(value) => value,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if let Err(error) = stack.push(value) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::FUNCTION_CONSTRUCTOR_HELPER => {
                        let constructor_args = if args.len() == 1 {
                            match self.collect_apply_arguments(args[0], task, module) {
                                Ok(values) => values,
                                Err(_) => args.clone(),
                            }
                        } else {
                            args.clone()
                        };
                        let value =
                            match self.alloc_dynamic_js_function(&constructor_args, task, module) {
                                Ok(value) => value,
                                Err(error) => return OpcodeResult::Error(error),
                            };
                        if let Err(error) = stack.push(value) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::FUNCTION_EVAL_HELPER => {
                        let source = if let Some(source) = args.first().copied() {
                            if checked_string_ptr(source).is_none() {
                                if let Err(error) = stack.push(source) {
                                    return OpcodeResult::Error(error);
                                }
                                return OpcodeResult::Continue;
                            }
                            match self.js_function_argument_to_string(source, task, module) {
                                Ok(source) => source,
                                Err(error) => return OpcodeResult::Error(error),
                            }
                        } else {
                            String::new()
                        };
                        let direct_env = args.get(1).copied();
                        let has_parameter_named_arguments =
                            args.get(2).copied().is_some_and(|value| value.is_truthy());
                        let in_parameter_initializer =
                            args.get(3).copied().is_some_and(|value| value.is_truthy());
                        let uses_script_global_bindings =
                            args.get(4).copied().is_some_and(|value| value.is_truthy());
                        let value = match self.eval_dynamic_js_source(
                            &source,
                            DynamicJsCompileOptions {
                                has_parameter_named_arguments,
                                in_parameter_initializer,
                                uses_script_global_bindings,
                                ..DynamicJsCompileOptions::default()
                            },
                            direct_env,
                            stack,
                            task,
                            module,
                        ) {
                            Ok(value) => value,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if let Err(error) = stack.push(value) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_JS_TO_NUMBER => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Object.jsToNumber expects exactly one argument".to_string(),
                            ));
                        }
                        let number = match self.js_to_number_with_context(args[0], task, module) {
                            Ok(number) => number,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        let value = Value::f64(number);
                        if let Err(error) = stack.push(value) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_JS_UNARY_MINUS => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Object.jsUnaryMinus expects exactly one argument".to_string(),
                            ));
                        }
                        let value = match self.js_unary_minus_with_context(args[0], task, module) {
                            Ok(value) => value,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if let Err(error) = stack.push(value) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_PARSE_BIGINT_LITERAL => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Object.parseBigIntLiteral expects exactly one argument"
                                    .to_string(),
                            ));
                        }
                        let source =
                            match self.js_function_argument_to_string(args[0], task, module) {
                                Ok(source) => source,
                                Err(error) => return OpcodeResult::Error(error),
                            };
                        let bigint = match self.parse_js_bigint_literal_value(&source) {
                            Ok(bigint) => bigint,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if let Err(error) = stack.push(self.alloc_bigint_value(bigint)) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_JS_TO_INTEGER_OR_INFINITY => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Object.jsToIntegerOrInfinity expects exactly one argument"
                                    .to_string(),
                            ));
                        }
                        let number = match self
                            .js_to_integer_or_infinity_with_context(args[0], task, module)
                        {
                            Ok(number) => number,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        let value = if number.fract() == 0.0
                            && number.is_finite()
                            && number >= i32::MIN as f64
                            && number <= i32::MAX as f64
                        {
                            Value::i32(number as i32)
                        } else {
                            Value::f64(number)
                        };
                        if let Err(error) = stack.push(value) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::FUNCTION_CALL_HELPER => {
                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Function.prototype.call requires a target function".to_string(),
                            ));
                        }
                        let target_callable = args[0];
                        if !self.js_call_target_supported(target_callable) {
                            if std::env::var("RAYA_DEBUG_CALL_HELPER").is_ok() {
                                let type_info = if target_callable.is_null() {
                                    "null"
                                } else if target_callable.is_undefined() {
                                    "undefined"
                                } else if target_callable.is_i32() {
                                    "i32"
                                } else if target_callable.is_f64() {
                                    "f64"
                                } else if target_callable.is_bool() {
                                    "bool"
                                } else if target_callable.is_ptr() {
                                    let ptr = unsafe { target_callable.as_ptr::<u8>().unwrap() };
                                    let hdr = unsafe { &*header_ptr_from_value_ptr(ptr.as_ptr()) };
                                    if hdr.type_id() == std::any::TypeId::of::<Object>() {
                                        let obj = unsafe {
                                            &*target_callable.as_ptr::<Object>().unwrap().as_ptr()
                                        };
                                        if obj.is_callable() {
                                            "Object(callable)"
                                        } else {
                                            "Object"
                                        }
                                    } else if hdr.type_id() == std::any::TypeId::of::<RayaString>()
                                    {
                                        "String"
                                    } else if hdr.type_id() == std::any::TypeId::of::<Array>() {
                                        "Array"
                                    } else {
                                        "ptr(other)"
                                    }
                                } else {
                                    "unknown"
                                };
                                eprintln!(
                                    "[CALL_HELPER] target not callable: raw={:#x} type={} nargs={}",
                                    target_callable.raw(),
                                    type_info,
                                    args.len()
                                );
                                let current_func_id = task.current_func_id();
                                let current_func_name = module
                                    .functions
                                    .get(current_func_id)
                                    .map(|function| function.name.as_str())
                                    .unwrap_or("<unknown>");
                                eprintln!(
                                    "[CALL_HELPER] current={}::{}#{}",
                                    module.metadata.name,
                                    current_func_name,
                                    current_func_id
                                );
                                for (index, frame) in
                                    task.get_execution_frames().iter().rev().take(6).enumerate()
                                {
                                    let frame_func_name = frame
                                        .module
                                        .functions
                                        .get(frame.func_id)
                                        .map(|function| function.name.as_str())
                                        .unwrap_or("<unknown>");
                                    eprintln!(
                                        "[CALL_HELPER] frame[{index}]={}::{}#{}",
                                        frame.module.metadata.name,
                                        frame_func_name,
                                        frame.func_id
                                    );
                                }
                            }
                            return OpcodeResult::Error(VmError::TypeError(
                                "Function.prototype.call target is not callable".to_string(),
                            ));
                        }
                        let this_arg = args.get(1).copied().unwrap_or(Value::undefined());
                        let rest_args = if args.len() >= 3 {
                            match self.collect_apply_arguments(args[2], task, module) {
                                Ok(values) => values,
                                Err(_) => args[2..].to_vec(),
                            }
                        } else {
                            Vec::new()
                        };

                        if let Some(target_ptr) = unsafe { target_callable.as_ptr::<u8>() } {
                            let header =
                                unsafe { &*header_ptr_from_value_ptr(target_ptr.as_ptr()) };
                            if header.type_id() == std::any::TypeId::of::<Object>() {
                                let co = unsafe {
                                    &*target_callable
                                        .as_ptr::<Object>()
                                        .expect("callable target")
                                        .as_ptr()
                                };
                                if let Some(ref cd) = co.callable {
                                    match &cd.kind {
                                        CallableKind::BoundNative {
                                            native_id,
                                            receiver,
                                        } => {
                                            return self.exec_bound_native_method_call(
                                                stack,
                                                *receiver,
                                                *native_id,
                                                rest_args,
                                                module,
                                                task,
                                            );
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        self.dispatch_call_with_explicit_this(
                            stack,
                            target_callable,
                            this_arg,
                            rest_args,
                            module,
                            task,
                            "Function.prototype.call target is not callable",
                        )
                    }

                    id if id == crate::compiler::native_id::FUNCTION_APPLY_HELPER => {
                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Function.prototype.apply requires a target function".to_string(),
                            ));
                        }
                        let target_callable = args[0];
                        if !self.js_call_target_supported(target_callable) {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Function.prototype.apply target is not callable".to_string(),
                            ));
                        }
                        let this_arg = args.get(1).copied().unwrap_or(Value::undefined());
                        let apply_args = if args.len() >= 3 {
                            match self.collect_apply_arguments(args[2], task, module) {
                                Ok(values) => values,
                                Err(error) => return OpcodeResult::Error(error),
                            }
                        } else {
                            Vec::new()
                        };

                        if let Some(target_ptr) = unsafe { target_callable.as_ptr::<u8>() } {
                            let header =
                                unsafe { &*header_ptr_from_value_ptr(target_ptr.as_ptr()) };
                            if header.type_id() == std::any::TypeId::of::<Object>() {
                                let co = unsafe {
                                    &*target_callable
                                        .as_ptr::<Object>()
                                        .expect("callable target")
                                        .as_ptr()
                                };
                                if let Some(ref cd) = co.callable {
                                    match &cd.kind {
                                        CallableKind::BoundNative {
                                            native_id,
                                            receiver,
                                        } => {
                                            return self.exec_bound_native_method_call(
                                                stack,
                                                *receiver,
                                                *native_id,
                                                apply_args,
                                                module,
                                                task,
                                            );
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }

                        match self.callable_frame_for_value(
                            target_callable,
                            stack,
                            &apply_args,
                            Some(this_arg),
                            ReturnAction::PushReturnValue,
                            module,
                            task,
                        ) {
                            Ok(Some(frame)) => frame,
                            Ok(None) => OpcodeResult::Error(VmError::TypeError(
                                "Function.prototype.apply target is not callable".to_string(),
                            )),
                            Err(error) => OpcodeResult::Error(error),
                        }
                    }

                    id if id == crate::compiler::native_id::FUNCTION_BIND_HELPER => {
                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Function.prototype.bind requires a target function".to_string(),
                            ));
                        }
                        let target_callable = args[0];
                        if !self.js_call_target_supported(target_callable) {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Function.prototype.bind target is not callable".to_string(),
                            ));
                        }
                        let this_arg = args.get(1).copied().unwrap_or(Value::undefined());
                        let bound_args = if args.len() >= 3 {
                            match self.collect_apply_arguments(args[2], task, module) {
                                Ok(values) => values,
                                Err(error) => return OpcodeResult::Error(error),
                            }
                        } else {
                            Vec::new()
                        };
                        let bound = match self.alloc_bound_function(
                            target_callable,
                            this_arg,
                            bound_args,
                            task,
                            module,
                        ) {
                            Ok(value) => value,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if let Err(error) = stack.push(bound) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_CONSTRUCT_DYNAMIC_CLASS => {
                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "dynamic class construction requires type handle as first argument"
                                    .to_string(),
                            ));
                        }

                        if self.callable_native_alias_id(args[0])
                            == Some(crate::compiler::native_id::FUNCTION_CONSTRUCTOR_HELPER)
                        {
                            let value =
                                match self.alloc_dynamic_js_function(&args[1..], task, module) {
                                    Ok(value) => value,
                                    Err(error) => return OpcodeResult::Error(error),
                                };
                            if let Err(error) = stack.push(value) {
                                return OpcodeResult::Error(error);
                            }
                            return OpcodeResult::Continue;
                        }

                        let value = match self.construct_value_with_new_target(
                            args[0],
                            args[0],
                            &args[1..],
                            task,
                            module,
                        ) {
                            Ok(value) => value,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if let Err(error) = stack.push(value) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_CONSTRUCT_APPLY_HELPER => {
                        if args.len() < 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "spread construction requires (type handle, args array)"
                                    .to_string(),
                            ));
                        }

                        let apply_args = match self.collect_apply_arguments(args[1], task, module) {
                            Ok(values) => values,
                            Err(error) => return OpcodeResult::Error(error),
                        };

                        if self.callable_native_alias_id(args[0])
                            == Some(crate::compiler::native_id::FUNCTION_CONSTRUCTOR_HELPER)
                        {
                            let value =
                                match self.alloc_dynamic_js_function(&apply_args, task, module) {
                                    Ok(value) => value,
                                    Err(error) => return OpcodeResult::Error(error),
                                };
                            if let Err(error) = stack.push(value) {
                                return OpcodeResult::Error(error);
                            }
                            return OpcodeResult::Continue;
                        }

                        let value = match self.construct_value_with_new_target(
                            args[0],
                            args[0],
                            &apply_args,
                            task,
                            module,
                        ) {
                            Ok(value) => value,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if let Err(error) = stack.push(value) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    id if id == crate::compiler::native_id::OBJECT_INSTANCE_OF_DYNAMIC_CLASS => {
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "dynamic instanceof requires (object, classValue)".to_string(),
                            ));
                        }

                        let mut result = false;
                        let is_task_backed_promise =
                            self.promise_handle_from_value(args[0]).is_some();
                        if !self.is_js_object_value(args[0]) && !is_task_backed_promise {
                            if let Err(error) = stack.push(Value::bool(false)) {
                                return OpcodeResult::Error(error);
                            }
                            return OpcodeResult::Continue;
                        }

                        if let Some(constructor_prototype) =
                            self.constructor_prototype_value(args[1])
                        {
                            let mut current = self.prototype_of_value(args[0]);
                            let mut seen = vec![args[0].raw()];
                            while let Some(prototype) = current {
                                if seen.contains(&prototype.raw()) {
                                    break;
                                }
                                seen.push(prototype.raw());
                                if prototype == constructor_prototype {
                                    result = true;
                                    break;
                                }
                                let next = self.prototype_of_value(prototype);
                                if next == current {
                                    break;
                                }
                                current = next;
                            }
                        }

                        if !result {
                            let Some(nominal_type_id) =
                                self.nominal_type_id_from_imported_class_value(module, args[1])
                            else {
                                return OpcodeResult::Error(VmError::TypeError(
                                    "dynamic instanceof expects imported or ambient class value"
                                        .to_string(),
                                ));
                            };

                            let classes = self.classes.read();
                            result = crate::vm::reflect::is_instance_of(
                                &classes,
                                args[0],
                                nominal_type_id,
                            );
                            if std::env::var("RAYA_DEBUG_INSTANCEOF").is_ok() {
                                eprintln!(
                                    "[instanceof-dynamic] object={:#x} class_value={:#x} nominal_type_id={} result={}",
                                    args[0].raw(),
                                    args[1].raw(),
                                    nominal_type_id,
                                    result
                                );
                            }
                        }
                        if let Err(error) = stack.push(Value::bool(result)) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_NEW => {
                        // Create a new channel with given capacity
                        let capacity = args[0].as_i32().unwrap_or(0) as usize;
                        let ch = ChannelObject::new(capacity);
                        let handle = self.allocate_pinned_handle(ch);
                        if let Err(e) = stack.push(Value::u64(handle)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_SEND => {
                        // args: [channel_handle, value]
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_SEND requires 2 arguments".to_string(),
                            ));
                        }
                        let value = args[1];
                        let (handle, channel) = match self.channel_from_handle_arg(args[0]) {
                            Ok(tuple) => tuple,
                            Err(err) => return OpcodeResult::Error(err),
                        };

                        if channel.is_closed() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Channel closed".to_string(),
                            ));
                        }
                        if channel.try_send(value) {
                            if let Err(e) = stack.push(Value::null()) {
                                return OpcodeResult::Error(e);
                            }
                            OpcodeResult::Continue
                        } else {
                            use crate::vm::scheduler::SuspendReason;
                            OpcodeResult::Suspend(SuspendReason::ChannelSend {
                                channel_id: handle,
                                value,
                            })
                        }
                    }

                    CHANNEL_RECEIVE => {
                        // args: [channel_handle]
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_RECEIVE requires 1 argument".to_string(),
                            ));
                        }
                        let (handle, channel) = match self.channel_from_handle_arg(args[0]) {
                            Ok(tuple) => tuple,
                            Err(err) => return OpcodeResult::Error(err),
                        };

                        if let Some(val) = channel.try_receive() {
                            if debug_native_stack {
                                eprintln!("[native] CHANNEL_RECEIVE immediate value");
                            }
                            if let Err(e) = stack.push(val) {
                                return OpcodeResult::Error(e);
                            }
                            OpcodeResult::Continue
                        } else if channel.is_closed() {
                            if debug_native_stack {
                                eprintln!("[native] CHANNEL_RECEIVE closed->null");
                            }
                            if let Err(e) = stack.push(Value::null()) {
                                return OpcodeResult::Error(e);
                            }
                            OpcodeResult::Continue
                        } else {
                            if debug_native_stack {
                                eprintln!("[native] CHANNEL_RECEIVE suspend");
                            }
                            use crate::vm::scheduler::SuspendReason;
                            OpcodeResult::Suspend(SuspendReason::ChannelReceive {
                                channel_id: handle,
                            })
                        }
                    }

                    CHANNEL_TRY_SEND => {
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_TRY_SEND requires 2 arguments".to_string(),
                            ));
                        }
                        let value = args[1];
                        let (_, channel) = match self.channel_from_handle_arg(args[0]) {
                            Ok(tuple) => tuple,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let result = channel.try_send(value);
                        if let Err(e) = stack.push(Value::bool(result)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_TRY_RECEIVE => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_TRY_RECEIVE requires 1 argument".to_string(),
                            ));
                        }
                        let (_, channel) = match self.channel_from_handle_arg(args[0]) {
                            Ok(tuple) => tuple,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let result = channel.try_receive().unwrap_or(Value::null());
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_CLOSE => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_CLOSE requires 1 argument".to_string(),
                            ));
                        }
                        let (_, channel) = match self.channel_from_handle_arg(args[0]) {
                            Ok(tuple) => tuple,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        channel.close();
                        // Reactor will wake any waiting tasks on next iteration
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_IS_CLOSED => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_IS_CLOSED requires 1 argument".to_string(),
                            ));
                        }
                        let (_, channel) = match self.channel_from_handle_arg(args[0]) {
                            Ok(tuple) => tuple,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let closed = channel.is_closed();
                        if debug_native_stack {
                            eprintln!("[native] CHANNEL_IS_CLOSED -> {}", closed);
                        }
                        if let Err(e) = stack.push(Value::bool(closed)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_LENGTH => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_LENGTH requires 1 argument".to_string(),
                            ));
                        }
                        let (_, channel) = match self.channel_from_handle_arg(args[0]) {
                            Ok(tuple) => tuple,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        if let Err(e) = stack.push(Value::i32(channel.length() as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    CHANNEL_CAPACITY => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "CHANNEL_CAPACITY requires 1 argument".to_string(),
                            ));
                        }
                        let (_, channel) = match self.channel_from_handle_arg(args[0]) {
                            Ok(tuple) => tuple,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        if let Err(e) = stack.push(Value::i32(channel.capacity() as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    // Buffer native calls
                    id if id == buffer::NEW => {
                        let size = match self.js_usize_arg_with_context(args[0], task, module) {
                            Ok(size) => size,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        let buf = Buffer::new(size);
                        let handle = self.allocate_pinned_handle(buf);
                        let wrapped = match self.alloc_buffer_object(handle, size) {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        if let Err(e) = stack.push(wrapped) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::LENGTH => {
                        let handle = match self.buffer_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let buf_ptr = handle as *const Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid buffer handle".to_string(),
                            ));
                        }
                        let buf = unsafe { &*buf_ptr };
                        if let Err(e) = stack.push(Value::i32(buf.length() as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::GET_BYTE => {
                        let handle = match self.buffer_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let index = match self.js_usize_arg_with_context(args[1], task, module) {
                            Ok(index) => index,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        let buf_ptr = handle as *const Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid buffer handle".to_string(),
                            ));
                        }
                        let buf = unsafe { &*buf_ptr };
                        let value = buf.get_byte(index).unwrap_or(0);
                        if let Err(e) = stack.push(Value::i32(value as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::SET_BYTE => {
                        let handle = match self.buffer_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let index = match self.js_usize_arg_with_context(args[1], task, module) {
                            Ok(index) => index,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        let value = match self.js_i32_arg_with_context(args[2], task, module) {
                            Ok(value) => value as u8,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        let buf_ptr = handle as *mut Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid buffer handle".to_string(),
                            ));
                        }
                        let buf = unsafe { &mut *buf_ptr };
                        if let Err(msg) = buf.set_byte(index, value) {
                            return OpcodeResult::Error(VmError::RuntimeError(msg));
                        }
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::GET_INT32 => {
                        let handle = match self.buffer_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let index = match self.js_usize_arg_with_context(args[1], task, module) {
                            Ok(index) => index,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        let buf_ptr = handle as *const Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid buffer handle".to_string(),
                            ));
                        }
                        let buf = unsafe { &*buf_ptr };
                        let value = buf.get_int32(index).unwrap_or(0);
                        if let Err(e) = stack.push(Value::i32(value)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::SET_INT32 => {
                        let handle = match self.buffer_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let index = match self.js_usize_arg_with_context(args[1], task, module) {
                            Ok(index) => index,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        let value = match self.js_i32_arg_with_context(args[2], task, module) {
                            Ok(value) => value,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        let buf_ptr = handle as *mut Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid buffer handle".to_string(),
                            ));
                        }
                        let buf = unsafe { &mut *buf_ptr };
                        if let Err(msg) = buf.set_int32(index, value) {
                            return OpcodeResult::Error(VmError::RuntimeError(msg));
                        }
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::GET_FLOAT64 => {
                        let handle = match self.buffer_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let index = match self.js_usize_arg_with_context(args[1], task, module) {
                            Ok(index) => index,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        let buf_ptr = handle as *const Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid buffer handle".to_string(),
                            ));
                        }
                        let buf = unsafe { &*buf_ptr };
                        let value = buf.get_float64(index).unwrap_or(0.0);
                        if let Err(e) = stack.push(Value::f64(value)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::SET_FLOAT64 => {
                        let handle = match self.buffer_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let index = match self.js_usize_arg_with_context(args[1], task, module) {
                            Ok(index) => index,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        let value = match self.js_to_number_with_context(args[2], task, module) {
                            Ok(value) => value,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        let buf_ptr = handle as *mut Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid buffer handle".to_string(),
                            ));
                        }
                        let buf = unsafe { &mut *buf_ptr };
                        if let Err(msg) = buf.set_float64(index, value) {
                            return OpcodeResult::Error(VmError::RuntimeError(msg));
                        }
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::SLICE => {
                        let handle = match self.buffer_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let start = match self.js_usize_arg_with_context(args[1], task, module) {
                            Ok(start) => start,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        let buf_ptr = handle as *const Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid buffer handle".to_string(),
                            ));
                        }
                        let buf = unsafe { &*buf_ptr };
                        // end is optional - if not provided, use buffer length
                        let end = if arg_count >= 3 {
                            match self.js_usize_arg_with_context(args[2], task, module) {
                                Ok(end) => end,
                                Err(error) => return OpcodeResult::Error(error),
                            }
                        } else {
                            buf.length()
                        };
                        let sliced = buf.slice(start, end);
                        let sliced_len = sliced.length() as i32;
                        let new_handle = self.allocate_pinned_handle(sliced);

                        let value = match self.alloc_buffer_object(new_handle, sliced_len as usize)
                        {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };

                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::COPY => {
                        // copy(srcHandle, targetHandle, targetStart?, sourceStart?, sourceEnd?)
                        let src_handle = match self.buffer_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let tgt_handle = match self.buffer_handle_from_value(args[1]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let src_ptr = src_handle as *const Buffer;
                        let tgt_ptr = tgt_handle as *mut Buffer;
                        if src_ptr.is_null() || tgt_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid buffer handle".to_string(),
                            ));
                        }
                        let src = unsafe { &*src_ptr };
                        let tgt = unsafe { &mut *tgt_ptr };

                        // Optional parameters with defaults
                        let tgt_start = if arg_count >= 3 {
                            match self.js_usize_arg_with_context(args[2], task, module) {
                                Ok(value) => value,
                                Err(error) => return OpcodeResult::Error(error),
                            }
                        } else {
                            0
                        };
                        let src_start = if arg_count >= 4 {
                            match self.js_usize_arg_with_context(args[3], task, module) {
                                Ok(value) => value,
                                Err(error) => return OpcodeResult::Error(error),
                            }
                        } else {
                            0
                        };
                        let src_end = if arg_count >= 5 {
                            match self.js_usize_arg_with_context(args[4], task, module) {
                                Ok(value) => value,
                                Err(error) => return OpcodeResult::Error(error),
                            }
                        } else {
                            src.data.len()
                        };

                        let src_end = src_end.min(src.data.len());
                        let src_start = src_start.min(src_end);
                        let bytes = &src.data[src_start..src_end];
                        let copy_len = bytes.len().min(tgt.data.len().saturating_sub(tgt_start));
                        tgt.data[tgt_start..tgt_start + copy_len]
                            .copy_from_slice(&bytes[..copy_len]);
                        if let Err(e) = stack.push(Value::i32(copy_len as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::TO_STRING => {
                        let handle = match self.buffer_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let buf_ptr = handle as *const Buffer;
                        if buf_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid buffer handle".to_string(),
                            ));
                        }
                        let buf = unsafe { &*buf_ptr };
                        // encoding argument (args[1]) — currently only utf8/ascii supported
                        let text = String::from_utf8_lossy(&buf.data).into_owned();
                        let s = RayaString::new(text);
                        let gc_ptr = self.gc.lock().allocate(s);
                        let val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == buffer::FROM_STRING => {
                        // args[0] = string pointer, args[1] = encoding (ignored, utf8)
                        if !args[0].is_ptr() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Expected string".to_string(),
                            ));
                        }
                        let str_ptr = unsafe { args[0].as_ptr::<RayaString>() };
                        let s = match str_ptr {
                            Some(p) => unsafe { &*p.as_ptr() },
                            None => {
                                return OpcodeResult::Error(VmError::TypeError(
                                    "Expected string".to_string(),
                                ))
                            }
                        };
                        let bytes = s.data.as_bytes();
                        let mut buf = Buffer::new(bytes.len());
                        buf.data.copy_from_slice(bytes);
                        let new_handle = self.allocate_pinned_handle(buf);
                        let value = match self.alloc_buffer_object(new_handle, bytes.len()) {
                            Ok(v) => v,
                            Err(e) => return OpcodeResult::Error(e),
                        };

                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // Mutex native calls
                    id if id == mutex::TRY_LOCK => {
                        let mutex_id = MutexId::from_u64(args[0].as_i64().unwrap_or(0) as u64);
                        if let Some(mutex) = self.mutex_registry.get(mutex_id) {
                            match mutex.try_lock(task.id()) {
                                Ok(()) => {
                                    task.add_held_mutex(mutex_id);
                                    if let Err(e) = stack.push(Value::bool(true)) {
                                        return OpcodeResult::Error(e);
                                    }
                                }
                                Err(_) => {
                                    if let Err(e) = stack.push(Value::bool(false)) {
                                        return OpcodeResult::Error(e);
                                    }
                                }
                            }
                        } else {
                            return OpcodeResult::Error(VmError::RuntimeError(format!(
                                "Mutex {:?} not found",
                                mutex_id
                            )));
                        }
                        OpcodeResult::Continue
                    }
                    id if id == mutex::IS_LOCKED => {
                        let mutex_id = MutexId::from_u64(args[0].as_i64().unwrap_or(0) as u64);
                        if let Some(mutex) = self.mutex_registry.get(mutex_id) {
                            let is_locked = mutex.is_locked();
                            if let Err(e) = stack.push(Value::bool(is_locked)) {
                                return OpcodeResult::Error(e);
                            }
                        } else {
                            return OpcodeResult::Error(VmError::RuntimeError(format!(
                                "Mutex {:?} not found",
                                mutex_id
                            )));
                        }
                        OpcodeResult::Continue
                    }
                    id if id == url::ENCODE => {
                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "encodeURI requires 1 argument".to_string(),
                            ));
                        }
                        let encoded = match value_as_string(args[0]) {
                            Ok(input) => percent_encode_uri_component(&input),
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let s = RayaString::new(encoded);
                        let gc_ptr = self.gc.lock().allocate(s);
                        let result = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == url::DECODE => {
                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "decodeURI requires 1 argument".to_string(),
                            ));
                        }
                        let decoded = match value_as_string(args[0])
                            .and_then(|input| percent_decode_uri_component(&input))
                        {
                            Ok(decoded) => decoded,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let s = RayaString::new(decoded);
                        let gc_ptr = self.gc.lock().allocate(s);
                        let result = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // Map native calls
                    id if id == map::NEW => {
                        let map = MapObject::new();
                        let handle = self.allocate_pinned_handle(map);
                        if let Err(e) = stack.push(Value::u64(handle)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::SIZE => {
                        let handle = match self.map_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let map_ptr = handle as *const MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid map handle".to_string(),
                            ));
                        }
                        let map = unsafe { &*map_ptr };
                        if let Err(e) = stack.push(Value::i32(map.size() as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::GET => {
                        let handle = match self.map_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let key = args[1];
                        let map_ptr = handle as *const MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid map handle".to_string(),
                            ));
                        }
                        let map = unsafe { &*map_ptr };
                        let value = map.get(key).unwrap_or(Value::null());
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::SET => {
                        let handle = match self.map_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let key = args[1];
                        let value = args[2];
                        let map_ptr = handle as *mut MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid map handle".to_string(),
                            ));
                        }
                        let map = unsafe { &mut *map_ptr };
                        map.set(key, value);
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::HAS => {
                        let handle = match self.map_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let key = args[1];
                        let map_ptr = handle as *const MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid map handle".to_string(),
                            ));
                        }
                        let map = unsafe { &*map_ptr };
                        if let Err(e) = stack.push(Value::bool(map.has(key))) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::DELETE => {
                        let handle = match self.map_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let key = args[1];
                        let map_ptr = handle as *mut MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid map handle".to_string(),
                            ));
                        }
                        let map = unsafe { &mut *map_ptr };
                        let result = map.delete(key);
                        if let Err(e) = stack.push(Value::bool(result)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::CLEAR => {
                        let handle = match self.map_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let map_ptr = handle as *mut MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid map handle".to_string(),
                            ));
                        }
                        let map = unsafe { &mut *map_ptr };
                        map.clear();
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::KEYS => {
                        let handle = match self.map_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let map_ptr = handle as *const MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid map handle".to_string(),
                            ));
                        }
                        let map = unsafe { &*map_ptr };
                        let keys = map.keys();
                        let mut arr = Array::new(0, 0);
                        for key in keys {
                            arr.push(key);
                        }
                        let arr_gc = self.gc.lock().allocate(arr);
                        let arr_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(arr_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::VALUES => {
                        let handle = match self.map_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let map_ptr = handle as *const MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid map handle".to_string(),
                            ));
                        }
                        let map = unsafe { &*map_ptr };
                        let values = map.values();
                        let mut arr = Array::new(0, 0);
                        for val in values {
                            arr.push(val);
                        }
                        let arr_gc = self.gc.lock().allocate(arr);
                        let arr_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(arr_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == map::ENTRIES => {
                        let handle = match self.map_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let map_ptr = handle as *const MapObject;
                        if map_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid map handle".to_string(),
                            ));
                        }
                        let map = unsafe { &*map_ptr };
                        let entries = map.entries();
                        let mut arr = Array::new(0, 0);
                        for (key, val) in entries {
                            let mut entry = Array::new(0, 0);
                            entry.push(key);
                            entry.push(val);
                            let entry_gc = self.gc.lock().allocate(entry);
                            let entry_val = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(entry_gc.as_ptr()).unwrap())
                            };
                            arr.push(entry_val);
                        }
                        let arr_gc = self.gc.lock().allocate(arr);
                        let arr_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(arr_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // Set native calls
                    id if id == set::NEW => {
                        let set_obj = SetObject::new();
                        let handle = self.allocate_pinned_handle(set_obj);
                        if let Err(e) = stack.push(Value::u64(handle)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::SIZE => {
                        let handle = match self.set_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let set_ptr = handle as *const SetObject;
                        if set_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid set handle".to_string(),
                            ));
                        }
                        let set_obj = unsafe { &*set_ptr };
                        if let Err(e) = stack.push(Value::i32(set_obj.size() as i32)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::ADD => {
                        let handle = match self.set_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let value = args[1];
                        let set_ptr = handle as *mut SetObject;
                        if set_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid set handle".to_string(),
                            ));
                        }
                        let set_obj = unsafe { &mut *set_ptr };
                        set_obj.add(value);
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::HAS => {
                        let handle = match self.set_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let value = args[1];
                        let set_ptr = handle as *const SetObject;
                        if set_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid set handle".to_string(),
                            ));
                        }
                        let set_obj = unsafe { &*set_ptr };
                        if let Err(e) = stack.push(Value::bool(set_obj.has(value))) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::DELETE => {
                        let handle = match self.set_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let value = args[1];
                        let set_ptr = handle as *mut SetObject;
                        if set_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid set handle".to_string(),
                            ));
                        }
                        let set_obj = unsafe { &mut *set_ptr };
                        let result = set_obj.delete(value);
                        if let Err(e) = stack.push(Value::bool(result)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::CLEAR => {
                        let handle = match self.set_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let set_ptr = handle as *mut SetObject;
                        if set_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid set handle".to_string(),
                            ));
                        }
                        let set_obj = unsafe { &mut *set_ptr };
                        set_obj.clear();
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::VALUES => {
                        let handle = match self.set_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let set_ptr = handle as *const SetObject;
                        if set_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid set handle".to_string(),
                            ));
                        }
                        let set_obj = unsafe { &*set_ptr };
                        let values = set_obj.values();
                        let mut arr = Array::new(0, 0);
                        for val in values {
                            arr.push(val);
                        }
                        let arr_gc = self.gc.lock().allocate(arr);
                        let arr_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(arr_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::UNION => {
                        let handle_a = match self.set_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let handle_b = match self.set_handle_from_value(args[1]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let set_a_ptr = handle_a as *const SetObject;
                        let set_b_ptr = handle_b as *const SetObject;
                        if set_a_ptr.is_null() || set_b_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid set handle".to_string(),
                            ));
                        }
                        let set_a = unsafe { &*set_a_ptr };
                        let set_b = unsafe { &*set_b_ptr };
                        let mut result = SetObject::new();
                        for val in set_a.values() {
                            result.add(val);
                        }
                        for val in set_b.values() {
                            result.add(val);
                        }
                        let handle = self.allocate_pinned_handle(result);
                        if let Err(e) = stack.push(Value::u64(handle)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::INTERSECTION => {
                        let handle_a = match self.set_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let handle_b = match self.set_handle_from_value(args[1]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let set_a_ptr = handle_a as *const SetObject;
                        let set_b_ptr = handle_b as *const SetObject;
                        if set_a_ptr.is_null() || set_b_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid set handle".to_string(),
                            ));
                        }
                        let set_a = unsafe { &*set_a_ptr };
                        let set_b = unsafe { &*set_b_ptr };
                        let mut result = SetObject::new();
                        for val in set_a.values() {
                            if set_b.has(val) {
                                result.add(val);
                            }
                        }
                        let handle = self.allocate_pinned_handle(result);
                        if let Err(e) = stack.push(Value::u64(handle)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == set::DIFFERENCE => {
                        let handle_a = match self.set_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let handle_b = match self.set_handle_from_value(args[1]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let set_a_ptr = handle_a as *const SetObject;
                        let set_b_ptr = handle_b as *const SetObject;
                        if set_a_ptr.is_null() || set_b_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid set handle".to_string(),
                            ));
                        }
                        let set_a = unsafe { &*set_a_ptr };
                        let set_b = unsafe { &*set_b_ptr };
                        let mut result = SetObject::new();
                        for val in set_a.values() {
                            if !set_b.has(val) {
                                result.add(val);
                            }
                        }
                        let handle = self.allocate_pinned_handle(result);
                        if let Err(e) = stack.push(Value::u64(handle)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_STRING_FROM_CHAR_CODE => {
                        let should_unpack_apply_args = args.len() == 1
                            && (checked_array_ptr(args[0]).is_some()
                                || matches!(
                                    self.exotic_adapter_kind(args[0]),
                                    Some(JsExoticAdapterKind::Arguments)
                                ));
                        let code_units = if should_unpack_apply_args {
                            match self.collect_apply_arguments(args[0], task, module) {
                                Ok(values) => values,
                                Err(_) => args.clone(),
                            }
                        } else {
                            args.clone()
                        };
                        let mut units = Vec::with_capacity(code_units.len());
                        for value in code_units {
                            let number = match self.js_to_number_with_context(value, task, module) {
                                Ok(number) => number,
                                Err(error) => return OpcodeResult::Error(error),
                            };
                            let unit = if !number.is_finite() || number == 0.0 {
                                0u16
                            } else {
                                number.trunc().rem_euclid(65536.0) as u16
                            };
                            units.push(unit);
                        }
                        let result = String::from_utf16_lossy(&units);
                        let raya_string = RayaString::new(result);
                        let gc_ptr = self.gc.lock().allocate(raya_string);
                        let value = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0F00u16 => {
                        // NUMBER_TO_FIXED: format number with fixed decimal places
                        // args[0] = number value, args[1] = digits
                        let value = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0);
                        let digits = args.get(1).and_then(|v| v.as_i32()).unwrap_or(0) as usize;
                        let formatted = format!("{:.prec$}", value, prec = digits);
                        let s = RayaString::new(formatted);
                        let gc_ptr = self.gc.lock().allocate(s);
                        let val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0F01u16 => {
                        // NUMBER_TO_PRECISION: format with N significant digits (or plain if no arg)
                        let value = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0);
                        let formatted = if args.get(1).is_none() {
                            // No precision argument: return plain toString()
                            if value.fract() == 0.0 && value.abs() < i64::MAX as f64 {
                                format!("{}", value as i64)
                            } else {
                                format!("{}", value)
                            }
                        } else {
                            let precision =
                                args.get(1).and_then(|v| v.as_i32()).unwrap_or(1).max(1) as usize;
                            if !value.is_finite() {
                                format!("{}", value)
                            } else if value == 0.0 {
                                if precision == 1 {
                                    "0".to_string()
                                } else {
                                    format!("0.{}", "0".repeat(precision - 1))
                                }
                            } else {
                                let magnitude = value.abs().log10().floor() as i32;
                                let scale_pow = magnitude - precision as i32 + 1;
                                let scale = 10f64.powi(scale_pow);
                                let rounded = (value / scale).round() * scale;
                                let decimal_places =
                                    (precision as i32 - magnitude - 1).max(0) as usize;
                                let mut text = format!("{:.prec$}", rounded, prec = decimal_places);
                                if decimal_places > 0 {
                                    while text.ends_with('0') {
                                        text.pop();
                                    }
                                    if text.ends_with('.') {
                                        text.pop();
                                    }
                                }
                                if text == "-0" {
                                    "0".to_string()
                                } else {
                                    text
                                }
                            }
                        };
                        let s = RayaString::new(formatted);
                        let gc_ptr = self.gc.lock().allocate(s);
                        let val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0F02u16 => {
                        // NUMBER_TO_STRING_RADIX: convert to string with radix
                        let value = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0);
                        let radix = args.get(1).and_then(|v| v.as_i32()).unwrap_or(10);
                        let formatted = if radix == 10 || !(2..=36).contains(&radix) {
                            if value.fract() == 0.0 && value.abs() < i64::MAX as f64 {
                                format!("{}", value as i64)
                            } else {
                                format!("{}", value)
                            }
                        } else {
                            // Integer radix conversion
                            let int_val = value as i64;
                            match radix {
                                2 => format!("{:b}", int_val),
                                8 => format!("{:o}", int_val),
                                16 => format!("{:x}", int_val),
                                _ => {
                                    // General radix conversion
                                    if int_val == 0 {
                                        "0".to_string()
                                    } else {
                                        let negative = int_val < 0;
                                        let mut n = int_val.unsigned_abs();
                                        let mut digits = Vec::new();
                                        let radix = radix as u64;
                                        while n > 0 {
                                            let d = (n % radix) as u8;
                                            digits.push(if d < 10 {
                                                b'0' + d
                                            } else {
                                                b'a' + d - 10
                                            });
                                            n /= radix;
                                        }
                                        digits.reverse();
                                        let s = String::from_utf8(digits).unwrap_or_default();
                                        if negative {
                                            format!("-{}", s)
                                        } else {
                                            s
                                        }
                                    }
                                }
                            }
                        };
                        let s = RayaString::new(formatted);
                        let gc_ptr = self.gc.lock().allocate(s);
                        let val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0F03u16 => {
                        // PARSE_INT: parse string to integer
                        let result = if let Some(ptr) = unsafe { args[0].as_ptr::<RayaString>() } {
                            let s = unsafe { &*ptr.as_ptr() }.data.trim();
                            // Parse integer, handling leading whitespace and optional sign
                            s.parse::<i64>()
                                .map(|v| v as f64)
                                .or_else(|_| s.parse::<f64>().map(|v| v.trunc()))
                                .unwrap_or(f64::NAN)
                        } else if let Some(n) = args[0].as_f64() {
                            n.trunc()
                        } else if let Some(n) = args[0].as_i32() {
                            n as f64
                        } else {
                            f64::NAN
                        };
                        if result.fract() == 0.0
                            && result.is_finite()
                            && result.abs() < i32::MAX as f64
                        {
                            if let Err(e) = stack.push(Value::i32(result as i32)) {
                                return OpcodeResult::Error(e);
                            }
                        } else if let Err(e) = stack.push(Value::f64(result)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0F04u16 => {
                        // PARSE_FLOAT: parse string to float
                        let result = if let Some(ptr) = unsafe { args[0].as_ptr::<RayaString>() } {
                            let s = unsafe { &*ptr.as_ptr() }.data.trim();
                            s.parse::<f64>().unwrap_or(f64::NAN)
                        } else if let Some(n) = args[0].as_f64() {
                            n
                        } else if let Some(n) = args[0].as_i32() {
                            n as f64
                        } else {
                            f64::NAN
                        };
                        if let Err(e) = stack.push(Value::f64(result)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0F05u16 => {
                        // IS_NAN: check if value is NaN
                        let is_nan = if let Some(n) = args[0].as_f64() {
                            n.is_nan()
                        } else if args[0].as_i32().is_some() {
                            false // integers are never NaN
                        } else {
                            true // non-numbers are treated as NaN
                        };
                        if let Err(e) = stack.push(Value::bool(is_nan)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0F06u16 => {
                        // IS_FINITE: check if value is finite
                        let is_finite = if let Some(n) = args[0].as_f64() {
                            n.is_finite()
                        } else if args[0].as_i32().is_some() {
                            true // integers are always finite
                        } else {
                            false
                        };
                        if let Err(e) = stack.push(Value::bool(is_finite)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x2000u16..=0x2014u16 => {
                        let result = (|| -> Result<f64, VmError> {
                            Ok(match native_id {
                                0x2000 => self.js_math_number_arg(&args, 0, task, module)?.abs(),
                                0x2001 => {
                                    let number = self.js_math_number_arg(&args, 0, task, module)?;
                                    if number.is_nan() {
                                        f64::NAN
                                    } else if number == 0.0 {
                                        number
                                    } else if number.is_sign_negative() {
                                        -1.0
                                    } else {
                                        1.0
                                    }
                                }
                                0x2002 => self.js_math_number_arg(&args, 0, task, module)?.floor(),
                                0x2003 => self.js_math_number_arg(&args, 0, task, module)?.ceil(),
                                0x2004 => {
                                    let number = self.js_math_number_arg(&args, 0, task, module)?;
                                    Self::js_math_round(number)
                                }
                                0x2005 => self.js_math_number_arg(&args, 0, task, module)?.trunc(),
                                0x2006 => self.js_math_min_max(&args, true, task, module)?,
                                0x2007 => self.js_math_min_max(&args, false, task, module)?,
                                0x2008 => {
                                    let base = self.js_math_number_arg(&args, 0, task, module)?;
                                    let exponent =
                                        self.js_math_number_arg(&args, 1, task, module)?;
                                    base.powf(exponent)
                                }
                                0x2009 => self.js_math_number_arg(&args, 0, task, module)?.sqrt(),
                                0x200A => self.js_math_number_arg(&args, 0, task, module)?.sin(),
                                0x200B => self.js_math_number_arg(&args, 0, task, module)?.cos(),
                                0x200C => self.js_math_number_arg(&args, 0, task, module)?.tan(),
                                0x200D => self.js_math_number_arg(&args, 0, task, module)?.asin(),
                                0x200E => self.js_math_number_arg(&args, 0, task, module)?.acos(),
                                0x200F => self.js_math_number_arg(&args, 0, task, module)?.atan(),
                                0x2010 => {
                                    let y = self.js_math_number_arg(&args, 0, task, module)?;
                                    let x = self.js_math_number_arg(&args, 1, task, module)?;
                                    y.atan2(x)
                                }
                                0x2011 => self.js_math_number_arg(&args, 0, task, module)?.exp(),
                                0x2012 => self.js_math_number_arg(&args, 0, task, module)?.ln(),
                                0x2013 => self.js_math_number_arg(&args, 0, task, module)?.log10(),
                                0x2014 => rand::random::<f64>(),
                                _ => unreachable!("math native range already matched"),
                            })
                        })();
                        let result = match result {
                            Ok(result) => result,
                            Err(e) => return OpcodeResult::Error(e),
                        };
                        if let Err(e) = stack.push(Value::f64(result)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // Object native calls
                    0x0001u16 => {
                        let target = args.first().copied().unwrap_or(Value::undefined());
                        let value = self.alloc_string_value(format!(
                            "[object {}]",
                            self.object_to_string_tag(target)
                        ));
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0002u16 => {
                        // OBJECT_HASH_CODE: return identity hash from object pointer
                        let hash = if !args.is_empty() {
                            // Use the raw bits of the value as a hash
                            let bits = args[0].as_u64().unwrap_or(0);
                            (bits ^ (bits >> 16)) as i32
                        } else {
                            0
                        };
                        if let Err(e) = stack.push(Value::i32(hash)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0003u16 => {
                        // OBJECT_EQUAL: reference equality
                        let equal = if args.len() >= 2 {
                            args[0].as_u64() == args[1].as_u64()
                        } else {
                            false
                        };
                        if let Err(e) = stack.push(Value::bool(equal)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0008u16 => {
                        let same = if args.len() >= 2 {
                            value_same_value(args[0], args[1])
                        } else {
                            false
                        };
                        if let Err(e) = stack.push(Value::bool(same)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0004u16 => {
                        // OBJECT_DEFINE_PROPERTY(target, key, descriptor) -> target
                        if args.len() < 3 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.defineProperty requires 3 arguments".to_string(),
                            ));
                        }
                        let target = args[0];
                        let key_val = args[1];
                        let descriptor = args[2];

                        if !target.is_ptr() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.defineProperty target must be an object".to_string(),
                            ));
                        }
                        let (Some(key), _) = (match self.property_key_parts_with_context(
                            key_val,
                            "Object.defineProperty",
                            task,
                            module,
                        ) {
                            Ok(parts) => parts,
                            Err(error) => return OpcodeResult::Error(error),
                        }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.defineProperty key must be a string or symbol".to_string(),
                            ));
                        };

                        if let Err(e) = self.apply_descriptor_to_target_with_context(
                            target, &key, descriptor, task, module,
                        ) {
                            return OpcodeResult::Error(e);
                        }
                        if let Err(e) = stack.push(target) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_DEFINE_CLASS_PROPERTY => {
                        // Internal class publication path: materialize method/accessor closures
                        // inside the VM, then define them on constructor/prototype targets.
                        if args.len() < 4 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.defineClassElement requires 4 arguments".to_string(),
                            ));
                        }
                        let target = args[0];
                        let key_val = args[1];
                        let Some(func_id) = args[2].as_i32() else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.defineClassElement func_id must be an int".to_string(),
                            ));
                        };
                        let Some(kind) = args[3].as_i32() else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.defineClassElement kind must be an int".to_string(),
                            ));
                        };

                        let (Some(key), _) = (match self.property_key_parts_with_context(
                            key_val,
                            "Object.defineClassElement",
                            task,
                            module,
                        ) {
                            Ok(parts) => parts,
                            Err(error) => return OpcodeResult::Error(error),
                        }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.defineClassElement key must be a string or symbol"
                                    .to_string(),
                            ));
                        };
                        let debug_class_publication =
                            std::env::var("RAYA_DEBUG_CLASS_PUBLICATION").is_ok();
                        if debug_class_publication {
                            eprintln!(
                                "[class-publish] target={:#x} key={} func_id={} kind={}",
                                target.raw(),
                                key,
                                func_id,
                                kind
                            );
                        }

                        let target_is_constructor =
                            self.constructor_nominal_type_id(target).is_some()
                                || self.type_handle_nominal_id(target).is_some();
                        if target_is_constructor && key == "prototype" {
                            return OpcodeResult::Error(
                                self.raise_task_builtin_error(
                                    task,
                                    "TypeError",
                                    "Classes may not have a static property named prototype"
                                        .to_string(),
                                ),
                            );
                        }

                        let closure = Object::new_closure_with_module(
                            func_id as usize,
                            Vec::new(),
                            module.clone().into(),
                        );
                        let closure_ptr = self.gc.lock().allocate(closure);
                        let closure_value = unsafe {
                            Value::from_ptr(
                                std::ptr::NonNull::new(closure_ptr.as_ptr())
                                    .expect("class element closure ptr"),
                            )
                        };
                        let Some(callable_ptr) = (unsafe { closure_value.as_ptr::<Object>() })
                        else {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "failed to materialize class element callable".to_string(),
                            ));
                        };
                        let callable = unsafe { &mut *callable_ptr.as_ptr() };
                        if let Err(error) = callable.set_callable_home_object(target) {
                            return OpcodeResult::Error(VmError::RuntimeError(error));
                        }

                        let define_result = match kind {
                            0 => self.define_data_property_on_target_with_context(
                                target,
                                &key,
                                closure_value,
                                true,
                                false,
                                true,
                                task,
                                module,
                            ),
                            1 | 2 => {
                                let descriptor = match self.alloc_object_descriptor() {
                                    Ok(descriptor) => descriptor,
                                    Err(error) => return OpcodeResult::Error(error),
                                };
                                let Some(descriptor_ptr) =
                                    (unsafe { descriptor.as_ptr::<Object>() })
                                else {
                                    return OpcodeResult::Error(VmError::RuntimeError(
                                        "Failed to allocate property descriptor object".to_string(),
                                    ));
                                };
                                let descriptor_obj = unsafe { &mut *descriptor_ptr.as_ptr() };
                                let accessor_field = if kind == 1 { "get" } else { "set" };
                                if let Some(field_index) =
                                    self.get_field_index_for_value(descriptor, accessor_field)
                                {
                                    if let Err(error) =
                                        descriptor_obj.set_field(field_index, closure_value)
                                    {
                                        return OpcodeResult::Error(VmError::RuntimeError(error));
                                    }
                                }
                                self.set_descriptor_field_present(descriptor, accessor_field, true);
                                for (field_name, field_value) in [
                                    ("enumerable", Value::bool(false)),
                                    ("configurable", Value::bool(true)),
                                ] {
                                    if let Some(field_index) =
                                        self.get_field_index_for_value(descriptor, field_name)
                                    {
                                        if let Err(error) =
                                            descriptor_obj.set_field(field_index, field_value)
                                        {
                                            return OpcodeResult::Error(VmError::RuntimeError(
                                                error,
                                            ));
                                        }
                                    }
                                    self.set_descriptor_field_present(descriptor, field_name, true);
                                }
                                self.apply_descriptor_to_target_with_context(
                                    target, &key, descriptor, task, module,
                                )
                            }
                            _ => Err(VmError::TypeError(format!(
                                "Object.defineClassElement kind '{}' is invalid",
                                kind
                            ))),
                        };

                        if debug_class_publication {
                            eprintln!("[class-publish] define_result={:?}", define_result);
                            eprintln!(
                                "[class-publish] post metadata_value={:?} metadata_desc={} own_names={:?}",
                                self.metadata_data_property_value(target, &key)
                                    .map(|value| format!("{:#x}", value.raw())),
                                self.metadata_descriptor_property(target, &key).is_some(),
                                self.js_own_property_names(target)
                            );
                        }
                        if let Err(error) = define_result {
                            return OpcodeResult::Error(error);
                        }
                        if let Err(error) = stack.push(target) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }
                    0x0005u16 => {
                        // OBJECT_GET_OWN_PROPERTY_DESCRIPTOR(target, key) -> descriptor | undefined
                        if args.len() < 2 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.getOwnPropertyDescriptor requires 2 arguments".to_string(),
                            ));
                        }
                        let target = args[0];
                        let key_val = args[1];
                        if !target.is_ptr() {
                            if let Err(e) = stack.push(Value::undefined()) {
                                return OpcodeResult::Error(e);
                            }
                            return OpcodeResult::Continue;
                        }
                        let (Some(key), _) = (match self.property_key_parts_with_context(
                            key_val,
                            "Object.getOwnPropertyDescriptor",
                            task,
                            module,
                        ) {
                            Ok(parts) => parts,
                            Err(error) => return OpcodeResult::Error(error),
                        }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.getOwnPropertyDescriptor key must be a string or symbol"
                                    .to_string(),
                            ));
                        };
                        let value = match self.get_descriptor_metadata(target, &key) {
                            Some(descriptor) => match self.clone_descriptor_object(descriptor) {
                                Ok(cloned) => cloned,
                                Err(error) => return OpcodeResult::Error(error),
                            },
                            None => {
                                match self.synthesize_accessor_property_descriptor(target, &key) {
                                    Ok(Some(descriptor)) => descriptor,
                                    Ok(None) => {
                                        match self.synthesize_data_property_descriptor(target, &key)
                                        {
                                            Ok(Some(descriptor)) => descriptor,
                                            Ok(None) => Value::undefined(),
                                            Err(error) => return OpcodeResult::Error(error),
                                        }
                                    }
                                    Err(error) => return OpcodeResult::Error(error),
                                }
                            }
                        };
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_GET_OWN_PROPERTY_SYMBOLS => {
                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.getOwnPropertySymbols requires 1 argument".to_string(),
                            ));
                        }
                        let target = args[0];
                        if !target.is_ptr() {
                            let arr_ptr = self.gc.lock().allocate(Array::new(0, 0));
                            let value = unsafe {
                                Value::from_ptr(
                                    std::ptr::NonNull::new(arr_ptr.as_ptr())
                                        .expect("symbol array ptr"),
                                )
                            };
                            if let Err(e) = stack.push(value) {
                                return OpcodeResult::Error(e);
                            }
                            return OpcodeResult::Continue;
                        }
                        let symbols = self.js_own_property_symbols(target);
                        let mut arr = Array::new(0, 0);
                        for symbol in symbols {
                            arr.push(symbol);
                        }
                        let arr_ptr = self.gc.lock().allocate(arr);
                        let value = unsafe {
                            Value::from_ptr(
                                std::ptr::NonNull::new(arr_ptr.as_ptr()).expect("symbol array ptr"),
                            )
                        };
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_COPY_DATA_PROPERTIES => {
                        if args.len() < 2 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.copyDataProperties requires target and source".to_string(),
                            ));
                        }
                        let target = args[0];
                        let source = args[1];
                        if let Err(error) =
                            self.copy_data_properties_with_context(target, source, task, module)
                        {
                            return OpcodeResult::Error(error);
                        }
                        if let Err(error) = stack.push(target) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }
                    id if id
                        == crate::compiler::native_id::OBJECT_COPY_DATA_PROPERTIES_EXCLUDING =>
                    {
                        if args.len() < 2 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.copyDataPropertiesExcluding requires target and source"
                                    .to_string(),
                            ));
                        }
                        let target = args[0];
                        let source = args[1];
                        let mut excluded_keys = FxHashSet::default();
                        for excluded in args.iter().skip(2) {
                            let key_parts = match self.property_key_parts_with_context(
                                *excluded,
                                "Object.copyDataPropertiesExcluding",
                                task,
                                module,
                            ) {
                                Ok(parts) => parts,
                                Err(error) => return OpcodeResult::Error(error),
                            };
                            let (Some(key), _) = key_parts else {
                                return OpcodeResult::Error(VmError::TypeError(
                                    "Object.copyDataPropertiesExcluding keys must be strings or symbols"
                                        .to_string(),
                                ));
                            };
                            excluded_keys.insert(key);
                        }
                        if let Err(error) = self.copy_data_properties_excluding_with_context(
                            target,
                            source,
                            &excluded_keys,
                            task,
                            module,
                        ) {
                            return OpcodeResult::Error(error);
                        }
                        if let Err(error) = stack.push(target) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }
                    0x0011u16 => {
                        // OBJECT_GET_PROTOTYPE_OF(target) -> prototype | null
                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.getPrototypeOf requires 1 argument".to_string(),
                            ));
                        }
                        let target = args[0];
                        if std::env::var("RAYA_DEBUG_PROTO_RESOLVE").is_ok() {
                            eprintln!(
                                "[get-proto-native] target={:#x} is_object={} callable={} explicit={}",
                                target.raw(),
                                checked_object_ptr(target).is_some(),
                                self.callable_function_info(target).is_some(),
                                self.explicit_object_prototype(target)
                                    .map(|value| format!("{:#x}", value.raw()))
                                    .unwrap_or_else(|| "None".to_string())
                            );
                        }
                        if target.is_null() || target.is_undefined() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Cannot convert undefined or null to object".to_string(),
                            ));
                        }
                        let prototype = self.prototype_of_value(target).unwrap_or(Value::null());
                        if let Err(e) = stack.push(prototype) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_GET_CLASS_VALUE => {
                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.getClassValue requires 1 argument".to_string(),
                            ));
                        }
                        let Some(local_nominal_type_id) =
                            args[0].as_i32().filter(|id| *id >= 0).map(|id| id as usize)
                        else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.getClassValue expects a non-negative nominal type id"
                                    .to_string(),
                            ));
                        };
                        let nominal_type_id =
                            match self.resolve_nominal_type_id(module, local_nominal_type_id) {
                                Ok(id) => id,
                                Err(error) => return OpcodeResult::Error(error),
                            };
                        let Some(value) = self.constructor_value_for_nominal_type(nominal_type_id)
                        else {
                            return OpcodeResult::Error(VmError::RuntimeError(format!(
                                "Object.getClassValue could not resolve nominal type {}",
                                nominal_type_id
                            )));
                        };
                        if std::env::var("RAYA_DEBUG_CLASS_PUBLICATION").is_ok() {
                            eprintln!(
                                "[class-value] nominal_type_id={} value={:#x} is_object={} is_callable={}",
                                nominal_type_id,
                                value.raw(),
                                checked_object_ptr(value).is_some(),
                                checked_callable_ptr(value).is_some()
                            );
                        }
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_IS_EXTENSIBLE => {
                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.isExtensible requires 1 argument".to_string(),
                            ));
                        }
                        if let Err(e) =
                            stack.push(Value::bool(self.is_js_value_extensible(args[0])))
                        {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_PREVENT_EXTENSIONS => {
                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.preventExtensions requires 1 argument".to_string(),
                            ));
                        }
                        let target = args[0];
                        self.set_js_value_extensible(target, false);
                        if let Err(e) = stack.push(target) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_GET_ARGUMENTS_OBJECT => {
                        let arguments = match self.make_arguments_object(stack, module, task) {
                            Ok(value) => value,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if let Err(e) = stack.push(arguments) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_ITERATOR_GET => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.iteratorGet requires 1 argument".to_string(),
                            ));
                        }
                        let iterator = match self.get_iterator_from_value(args[0], task, module) {
                            Ok(value) => value,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if let Err(error) = stack.push(iterator) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_ITERATOR_STEP => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.iteratorStep requires 1 argument".to_string(),
                            ));
                        }
                        let step = match self.iterator_step_result(args[0], task, module) {
                            Ok(Some(value)) => value,
                            Ok(None) => Value::null(),
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if let Err(error) = stack.push(step) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_ITERATOR_VALUE => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.iteratorValue requires 1 argument".to_string(),
                            ));
                        }
                        let value = match self.iterator_result_value(args[0], task, module) {
                            Ok(value) => value,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if let Err(error) = stack.push(value) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_ITERATOR_CLOSE => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.iteratorClose requires 1 argument".to_string(),
                            ));
                        }
                        if let Err(error) = self.iterator_close(args[0], task, module) {
                            return OpcodeResult::Error(error);
                        }
                        stack
                            .push(Value::undefined())
                            .map_or_else(OpcodeResult::Error, |_| OpcodeResult::Continue)
                    }
                    id if id == crate::compiler::native_id::OBJECT_ITERATOR_CLOSE_ON_THROW => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.iteratorCloseOnThrow requires 1 argument".to_string(),
                            ));
                        }
                        let _ = self.iterator_close(args[0], task, module);
                        stack
                            .push(Value::undefined())
                            .map_or_else(OpcodeResult::Error, |_| OpcodeResult::Continue)
                    }
                    id if id == crate::compiler::native_id::OBJECT_ITERATOR_CLOSE_COMPLETION => {
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.iteratorCloseCompletion requires iterator and completion"
                                    .to_string(),
                            ));
                        }
                        let completion =
                            self.iterator_close_completion_value(args[0], args[1], task, module);
                        stack
                            .push(completion)
                            .map_or_else(OpcodeResult::Error, |_| OpcodeResult::Continue)
                    }
                    id if id == crate::compiler::native_id::OBJECT_ITERATOR_APPEND_TO_ARRAY => {
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.iteratorAppendToArray requires target array and iterable"
                                    .to_string(),
                            ));
                        }
                        if let Err(error) =
                            self.append_iterable_to_array(args[0], args[1], task, module)
                        {
                            return OpcodeResult::Error(error);
                        }
                        stack
                            .push(args[0])
                            .map_or_else(OpcodeResult::Error, |_| OpcodeResult::Continue)
                    }
                    id if id == crate::compiler::native_id::OBJECT_GENERATOR_SNAPSHOT_NEW => {
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.generatorSnapshotNew requires yielded array and completion"
                                    .to_string(),
                            ));
                        }
                        let yielded = checked_array_ptr(args[0])
                            .map(|array_ptr| unsafe { &*array_ptr.as_ptr() }.elements.clone())
                            .unwrap_or_default();
                        let iterator = self.generator_snapshot_iterator_object(yielded, args[1]);
                        if let Err(error) = stack.push(iterator) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_GENERATOR_SNAPSHOT_NEXT => {
                        let receiver = args.first().copied().unwrap_or(Value::undefined());
                        let Some(obj_ptr) = checked_object_ptr(receiver) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Generator snapshot next receiver must be an object".to_string(),
                            ));
                        };
                        let obj = unsafe { &mut *obj_ptr.as_ptr() };
                        let Some(generator) = obj.generator_snapshot.as_deref_mut() else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Generator snapshot next receiver is invalid".to_string(),
                            ));
                        };
                        let result = if generator.next_index < generator.yielded.len() {
                            let value = generator.yielded[generator.next_index];
                            generator.next_index += 1;
                            self.generator_result_object(value, false)
                        } else if !generator.completion_emitted {
                            generator.completion_emitted = true;
                            self.generator_result_object(generator.completion, true)
                        } else {
                            self.generator_result_object(Value::undefined(), true)
                        };
                        if let Err(error) = stack.push(result) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_GENERATOR_SNAPSHOT_RETURN => {
                        let receiver = args.first().copied().unwrap_or(Value::undefined());
                        let completion_override = args.get(1).copied();
                        let Some(obj_ptr) = checked_object_ptr(receiver) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Generator snapshot return receiver must be an object".to_string(),
                            ));
                        };
                        let obj = unsafe { &mut *obj_ptr.as_ptr() };
                        let Some(generator) = obj.generator_snapshot.as_deref_mut() else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Generator snapshot return receiver is invalid".to_string(),
                            ));
                        };
                        generator.next_index = generator.yielded.len();
                        generator.completion_emitted = true;
                        let result = self.generator_result_object(
                            completion_override.unwrap_or(generator.completion),
                            true,
                        );
                        if let Err(error) = stack.push(result) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_GENERATOR_SNAPSHOT_ITERATOR => {
                        let receiver = args.first().copied().unwrap_or(Value::undefined());
                        if let Err(error) = stack.push(receiver) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_GENERATOR_NEW => {
                        let Some(task_id) = args
                            .first()
                            .and_then(|value| {
                                value.as_u64().or_else(|| value.as_i32().map(|v| v as u64))
                            })
                            .map(TaskId::from_u64)
                        else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.generatorNew requires a task id".to_string(),
                            ));
                        };
                        let iterator = self.generator_iterator_object(
                            task_id,
                            self.default_generator_instance_prototype(false),
                            false,
                        );
                        if let Err(error) = stack.push(iterator) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_GENERATOR_NEXT => {
                        let receiver = args.first().copied().unwrap_or(Value::undefined());
                        let Some(obj_ptr) = checked_object_ptr(receiver) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Generator next receiver must be an object".to_string(),
                            ));
                        };
                        let (task_id, resume_value, was_started, is_async) = {
                            let obj = unsafe { &mut *obj_ptr.as_ptr() };
                            let Some(generator) = obj.generator_state.as_deref_mut() else {
                                return OpcodeResult::Error(VmError::TypeError(
                                    "Generator next receiver is invalid".to_string(),
                                ));
                            };
                            if generator.closed {
                                let value = if generator.completion_emitted {
                                    Value::undefined()
                                } else {
                                    generator.completion_emitted = true;
                                    generator.completion
                                };
                                let result = self.generator_result_object(value, true);
                                let result = if generator.is_async {
                                    self.settled_task_handle(task, Ok(result)).into_value()
                                } else {
                                    result
                                };
                                if let Err(error) = stack.push(result) {
                                    return OpcodeResult::Error(error);
                                }
                                return OpcodeResult::Continue;
                            }

                            let resume_value = if let Some(completion) =
                                generator.pending_return_completion.take()
                            {
                                self.generator_return_signal(completion)
                            } else {
                                args.get(1).copied().unwrap_or(Value::undefined())
                            };
                            let task_id = generator.task_id;
                            let was_started = generator.started;
                            if !generator.started {
                                generator.started = true;
                            }
                            (task_id, resume_value, was_started, generator.is_async)
                        };

                        let Some(generator_task) = self.tasks.read().get(&task_id).cloned() else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Generator task is no longer available".to_string(),
                            ));
                        };
                        {
                            let obj = unsafe { &mut *obj_ptr.as_ptr() };
                            let Some(generator) = obj.generator_state.as_deref_mut() else {
                                return OpcodeResult::Error(VmError::TypeError(
                                    "Generator next receiver is invalid".to_string(),
                                ));
                            };
                            if was_started {
                                generator_task.set_resume_value(resume_value);
                            }
                        }

                        let run_result = self.run(&generator_task);
                        let result = match run_result {
                            ExecutionResult::Completed(value) => {
                                let obj = unsafe { &mut *obj_ptr.as_ptr() };
                                let Some(generator) = obj.generator_state.as_deref_mut() else {
                                    return OpcodeResult::Error(VmError::TypeError(
                                        "Generator next receiver is invalid".to_string(),
                                    ));
                                };
                                generator_task.complete(value);
                                generator.closed = true;
                                generator.pending_return_completion = None;
                                generator.completion = value;
                                generator.completion_emitted = true;
                                self.generator_result_object(value, true)
                            }
                            ExecutionResult::Suspended(
                                crate::vm::scheduler::SuspendReason::JsGeneratorYield { value },
                            ) => {
                                let obj = unsafe { &mut *obj_ptr.as_ptr() };
                                let Some(generator) = obj.generator_state.as_deref_mut() else {
                                    return OpcodeResult::Error(VmError::TypeError(
                                        "Generator next receiver is invalid".to_string(),
                                    ));
                                };
                                generator_task.suspend(
                                    crate::vm::scheduler::SuspendReason::JsGeneratorYield { value },
                                );
                                self.generator_result_object(value, false)
                            }
                            ExecutionResult::Suspended(reason) => {
                                generator_task.suspend(reason);
                                return OpcodeResult::Error(VmError::RuntimeError(
                                    "Generator suspended with a non-generator reason".to_string(),
                                ));
                            }
                            ExecutionResult::Failed(error) => {
                                if let Some(completion) =
                                    self.take_generator_return_completion(&generator_task)
                                {
                                    let obj = unsafe { &mut *obj_ptr.as_ptr() };
                                    let Some(generator) = obj.generator_state.as_deref_mut() else {
                                        return OpcodeResult::Error(VmError::TypeError(
                                            "Generator next receiver is invalid".to_string(),
                                        ));
                                    };
                                    generator_task.complete(completion);
                                    generator.closed = true;
                                    generator.pending_return_completion = None;
                                    generator.completion = completion;
                                    generator.completion_emitted = true;
                                    self.generator_result_object(completion, true)
                                } else {
                                    let obj = unsafe { &mut *obj_ptr.as_ptr() };
                                    let Some(generator) = obj.generator_state.as_deref_mut() else {
                                        return OpcodeResult::Error(VmError::TypeError(
                                            "Generator next receiver is invalid".to_string(),
                                        ));
                                    };
                                    generator.closed = true;
                                    generator.pending_return_completion = None;
                                    generator.completion = Value::undefined();
                                    generator.completion_emitted = true;
                                    generator_task.fail();
                                    if is_async {
                                        self.ensure_task_exception_for_error(&generator_task, &error);
                                        let exception = generator_task
                                            .current_exception()
                                            .unwrap_or(Value::undefined());
                                        self.settled_task_handle(task, Err(exception)).into_value()
                                    } else {
                                        if !task.has_exception() {
                                            if let Some(exception) =
                                                generator_task.current_exception()
                                            {
                                                task.set_exception(exception);
                                            }
                                        }
                                        return OpcodeResult::Error(error);
                                    }
                                }
                            }
                        };
                        let result = if is_async {
                            self.settled_task_handle(task, Ok(result)).into_value()
                        } else {
                            result
                        };
                        if let Err(error) = stack.push(result) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_GENERATOR_RETURN => {
                        let receiver = args.first().copied().unwrap_or(Value::undefined());
                        let completion_override =
                            args.get(1).copied().unwrap_or(Value::undefined());
                        let Some(obj_ptr) = checked_object_ptr(receiver) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Generator return receiver must be an object".to_string(),
                            ));
                        };
                        let (task_id, already_closed_or_unstarted, is_async) = {
                            let obj = unsafe { &mut *obj_ptr.as_ptr() };
                            let Some(generator) = obj.generator_state.as_deref_mut() else {
                                return OpcodeResult::Error(VmError::TypeError(
                                    "Generator return receiver is invalid".to_string(),
                                ));
                            };
                            if generator.closed || !generator.started {
                                generator.closed = true;
                                generator.pending_return_completion = None;
                                generator.completion = completion_override;
                                generator.completion_emitted = true;
                                (generator.task_id, true, generator.is_async)
                            } else {
                                generator.pending_return_completion = None;
                                (generator.task_id, false, generator.is_async)
                            }
                        };
                        if already_closed_or_unstarted {
                            let result = self.generator_result_object(completion_override, true);
                            let result = if is_async {
                                self.settled_task_handle(task, Ok(result)).into_value()
                            } else {
                                result
                            };
                            if let Err(error) = stack.push(result) {
                                return OpcodeResult::Error(error);
                            }
                            return OpcodeResult::Continue;
                        }

                        let Some(generator_task) = self.tasks.read().get(&task_id).cloned() else {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Generator task is no longer available".to_string(),
                            ));
                        };
                        generator_task
                            .set_resume_value(self.generator_return_signal(completion_override));

                        let run_result = self.run(&generator_task);
                        let result = match run_result {
                            ExecutionResult::Completed(value) => {
                                let obj = unsafe { &mut *obj_ptr.as_ptr() };
                                let Some(generator) = obj.generator_state.as_deref_mut() else {
                                    return OpcodeResult::Error(VmError::TypeError(
                                        "Generator return receiver is invalid".to_string(),
                                    ));
                                };
                                generator_task.complete(value);
                                generator.closed = true;
                                generator.pending_return_completion = None;
                                generator.completion = value;
                                generator.completion_emitted = true;
                                self.generator_result_object(value, true)
                            }
                            ExecutionResult::Suspended(
                                crate::vm::scheduler::SuspendReason::JsGeneratorYield { value },
                            ) => {
                                let obj = unsafe { &mut *obj_ptr.as_ptr() };
                                let Some(generator) = obj.generator_state.as_deref_mut() else {
                                    return OpcodeResult::Error(VmError::TypeError(
                                        "Generator return receiver is invalid".to_string(),
                                    ));
                                };
                                generator_task.suspend(
                                    crate::vm::scheduler::SuspendReason::JsGeneratorYield { value },
                                );
                                generator.pending_return_completion = Some(completion_override);
                                self.generator_result_object(value, false)
                            }
                            ExecutionResult::Suspended(reason) => {
                                generator_task.suspend(reason);
                                return OpcodeResult::Error(VmError::RuntimeError(
                                    "Generator suspended with a non-generator reason".to_string(),
                                ));
                            }
                            ExecutionResult::Failed(error) => {
                                if let Some(completion) =
                                    self.take_generator_return_completion(&generator_task)
                                {
                                    let obj = unsafe { &mut *obj_ptr.as_ptr() };
                                    let Some(generator) = obj.generator_state.as_deref_mut() else {
                                        return OpcodeResult::Error(VmError::TypeError(
                                            "Generator return receiver is invalid".to_string(),
                                        ));
                                    };
                                    generator_task.complete(completion);
                                    generator.closed = true;
                                    generator.pending_return_completion = None;
                                    generator.completion = completion;
                                    generator.completion_emitted = true;
                                    self.generator_result_object(completion, true)
                                } else {
                                    let obj = unsafe { &mut *obj_ptr.as_ptr() };
                                    let Some(generator) = obj.generator_state.as_deref_mut() else {
                                        return OpcodeResult::Error(VmError::TypeError(
                                            "Generator return receiver is invalid".to_string(),
                                        ));
                                    };
                                    generator.closed = true;
                                    generator.pending_return_completion = None;
                                    generator.completion = Value::undefined();
                                    generator.completion_emitted = true;
                                    generator_task.fail();
                                    if is_async {
                                        self.ensure_task_exception_for_error(&generator_task, &error);
                                        let exception = generator_task
                                            .current_exception()
                                            .unwrap_or(Value::undefined());
                                        self.settled_task_handle(task, Err(exception)).into_value()
                                    } else {
                                        if !task.has_exception() {
                                            if let Some(exception) =
                                                generator_task.current_exception()
                                            {
                                                task.set_exception(exception);
                                            }
                                        }
                                        return OpcodeResult::Error(error);
                                    }
                                }
                            }
                        };
                        let result = if is_async {
                            self.settled_task_handle(task, Ok(result)).into_value()
                        } else {
                            result
                        };
                        if let Err(error) = stack.push(result) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_HANDLE_GENERATOR_RESUME => {
                        let resumed = args.first().copied().unwrap_or(Value::undefined());
                        if self.generator_return_signal_value(resumed).is_some() {
                            task.set_exception(resumed);
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Generator return completion".to_string(),
                            ));
                        }
                        if let Err(error) = stack.push(resumed) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_GENERATOR_ITERATOR => {
                        let receiver = args.first().copied().unwrap_or(Value::undefined());
                        if let Err(error) = stack.push(receiver) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_REQUIRE_OBJECT_COERCIBLE => {
                        let value = args.first().copied().unwrap_or(Value::undefined());
                        if value.is_null() || value.is_undefined() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Cannot convert undefined or null to object".to_string(),
                            ));
                        }
                        if let Err(error) = stack.push(value) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_GET_DESTRUCTURING_PROPERTY => {
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.getDestructuringProperty requires target and key"
                                    .to_string(),
                            ));
                        }
                        let target = args[0];
                        if target.is_null() || target.is_undefined() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Cannot convert undefined or null to object".to_string(),
                            ));
                        }
                        let (Some(key_str), _) = (match self.property_key_parts_with_context(
                            args[1],
                            "Object.getDestructuringProperty",
                            task,
                            module,
                        ) {
                            Ok(parts) => parts,
                            Err(error) => return OpcodeResult::Error(error),
                        }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Cannot convert property key to string".to_string(),
                            ));
                        };
                        let value = if self.has_property_via_js_semantics(target, &key_str) {
                            match self.get_property_value_via_js_semantics_with_context(
                                target, &key_str, task, module,
                            ) {
                                Ok(Some(value)) => value,
                                Ok(None) => Value::undefined(),
                                Err(error) => return OpcodeResult::Error(error),
                            }
                        } else {
                            Value::undefined()
                        };
                        if let Err(error) = stack.push(value) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_COERCE_PROPERTY_KEY => {
                        if args.len() != 1 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.coercePropertyKey requires a key".to_string(),
                            ));
                        }
                        let (Some(key_str), _) = (match self.property_key_parts_with_context(
                            args[0],
                            "Object.coercePropertyKey",
                            task,
                            module,
                        ) {
                            Ok(parts) => parts,
                            Err(error) => return OpcodeResult::Error(error),
                        }) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Cannot convert property key to string".to_string(),
                            ));
                        };
                        if let Err(error) = stack.push(self.alloc_string_value(key_str)) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_ASSIGN_BINDING_NAME_IF_MISSING => {
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.assignBindingNameIfMissing requires target and binding name"
                                    .to_string(),
                            ));
                        }
                        let target = args[0];
                        let Some(name_ptr) = checked_string_ptr(args[1]) else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Binding name must be a string".to_string(),
                            ));
                        };
                        let name = unsafe { &*name_ptr.as_ptr() }.data.clone();
                        if !self.has_explicit_own_property_via_js_semantics(target, "name") {
                            if let Err(error) = self.define_data_property_on_target(
                                target,
                                "name",
                                self.alloc_string_value(name),
                                false,
                                false,
                                true,
                            ) {
                                return OpcodeResult::Error(error);
                            }
                        }
                        if let Err(error) = stack.push(target) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_SET_PROTOTYPE_OF => {
                        if args.len() < 2 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.setPrototypeOf requires target and prototype".to_string(),
                            ));
                        }
                        let target = args[0];
                        let prototype = args[1];
                        if target.is_null() || target.is_undefined() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Cannot convert undefined or null to object".to_string(),
                            ));
                        }
                        if !self.js_value_supports_extensibility(target) {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.setPrototypeOf target must be an object".to_string(),
                            ));
                        }
                        if !prototype.is_null() && !self.is_js_object_value(prototype) {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.setPrototypeOf prototype must be an object or null"
                                    .to_string(),
                            ));
                        }
                        if let Err(e) =
                            stack.push(Value::bool(self.set_prototype_of_value(target, prototype)))
                        {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0006u16 => {
                        // OBJECT_DEFINE_PROPERTIES(target, descriptors) -> target
                        if args.len() < 2 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.defineProperties requires 2 arguments".to_string(),
                            ));
                        }
                        let target = args[0];
                        let descriptors_obj = args[1];
                        if !target.is_ptr() {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.defineProperties target must be an object".to_string(),
                            ));
                        }
                        if let Some(desc_ptr) = unsafe { descriptors_obj.as_ptr::<Object>() } {
                            let desc_obj = unsafe { &*desc_ptr.as_ptr() };
                            let field_names = desc_obj
                                .nominal_type_id_usize()
                                .and_then(|nominal_type_id| {
                                    let metadata = self.class_metadata.read();
                                    metadata
                                        .get(nominal_type_id)
                                        .map(|m| m.field_names.clone())
                                        .filter(|names| !names.is_empty())
                                })
                                .or_else(|| self.layout_field_names_for_object(desc_obj))
                                .unwrap_or_default();
                            for (idx, field_name) in field_names.into_iter().enumerate() {
                                if field_name.is_empty() {
                                    continue;
                                }
                                if let Some(descriptor_val) = desc_obj.get_field(idx) {
                                    if let Err(e) = self.apply_descriptor_to_target_with_context(
                                        target,
                                        &field_name,
                                        descriptor_val,
                                        task,
                                        module,
                                    ) {
                                        return OpcodeResult::Error(e);
                                    }
                                }
                            }
                        } else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.defineProperties descriptors must be an object".to_string(),
                            ));
                        }
                        if let Err(e) = stack.push(target) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x000Cu16 => {
                        // OBJECT_DELETE_PROPERTY(target, key) -> bool
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.deleteProperty requires 2 arguments".to_string(),
                            ));
                        }
                        let deleted = match self
                            .delete_property_from_target(args[0], args[1], task, module)
                        {
                            Ok(result) => result,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if let Err(error) = stack.push(Value::bool(deleted)) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::OBJECT_DELETE_PROPERTY_STRICT => {
                        if args.len() != 2 {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Object.deletePropertyStrict requires 2 arguments".to_string(),
                            ));
                        }
                        let deleted = match self
                            .delete_property_from_target(args[0], args[1], task, module)
                        {
                            Ok(result) => result,
                            Err(error) => return OpcodeResult::Error(error),
                        };
                        if !deleted {
                            return OpcodeResult::Error(VmError::TypeError(
                                "Cannot delete non-configurable property".to_string(),
                            ));
                        }
                        if let Err(error) = stack.push(Value::bool(true)) {
                            return OpcodeResult::Error(error);
                        }
                        OpcodeResult::Continue
                    }
                    // Task native calls
                    0x0500u16 => {
                        // TASK_IS_DONE: check if task completed
                        let task_id = args
                            .first()
                            .copied()
                            .and_then(|value| self.promise_handle_from_value(value))
                            .map(|handle| handle.task_id());
                        let tasks = self.tasks.read();
                        let is_done = task_id
                            .and_then(|task_id| tasks.get(&task_id))
                            .map(|t| matches!(t.state(), TaskState::Completed | TaskState::Failed))
                            .unwrap_or(true);
                        if let Err(e) = stack.push(Value::bool(is_done)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0501u16 => {
                        // TASK_IS_CANCELLED: check if task cancelled
                        let task_id = args
                            .first()
                            .copied()
                            .and_then(|value| self.promise_handle_from_value(value))
                            .map(|handle| handle.task_id());
                        let tasks = self.tasks.read();
                        let is_cancelled = task_id
                            .and_then(|task_id| tasks.get(&task_id))
                            .map(|t| t.is_cancelled())
                            .unwrap_or(false);
                        if let Err(e) = stack.push(Value::bool(is_cancelled)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0502u16 => {
                        // TASK_IS_FAILED: check if task failed
                        let task_id = args
                            .first()
                            .copied()
                            .and_then(|value| self.promise_handle_from_value(value))
                            .map(|handle| handle.task_id());
                        let tasks = self.tasks.read();
                        let is_failed = task_id
                            .and_then(|task_id| tasks.get(&task_id))
                            .map(|t| t.state() == TaskState::Failed)
                            .unwrap_or(false);
                        if let Err(e) = stack.push(Value::bool(is_failed)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0503u16 => {
                        // TASK_GET_ERROR: retrieve rejection reason and mark it observed
                        let task_id = args
                            .first()
                            .copied()
                            .and_then(|value| self.promise_handle_from_value(value))
                            .map(|handle| handle.task_id());
                        let tasks = self.tasks.read();
                        let reason = task_id
                            .and_then(|task_id| tasks.get(&task_id))
                            .and_then(|t| {
                                t.mark_rejection_observed();
                                t.current_exception()
                            })
                            .unwrap_or(Value::null());
                        if let Err(e) = stack.push(reason) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0504u16 => {
                        // TASK_MARK_OBSERVED: mark rejection as handled
                        let task_id = args
                            .first()
                            .copied()
                            .and_then(|value| self.promise_handle_from_value(value))
                            .map(|handle| handle.task_id());
                        let tasks = self.tasks.read();
                        if let Some(task) = task_id.and_then(|task_id| tasks.get(&task_id)) {
                            task.mark_rejection_observed();
                        }
                        if let Err(e) = stack.push(Value::null()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0505u16 => {
                        // TASK_GET_RESULT: retrieve a fulfilled task result.
                        let task_id = args
                            .first()
                            .copied()
                            .and_then(|value| self.promise_handle_from_value(value))
                            .map(|handle| handle.task_id());
                        let tasks = self.tasks.read();
                        let result = task_id
                            .and_then(|task_id| tasks.get(&task_id))
                            .and_then(|t| t.result())
                            .unwrap_or(Value::undefined());
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0506u16 => {
                        // TASK_REJECT_NOW: create an already-rejected task handle.
                        let reason = args.first().copied().unwrap_or(Value::undefined());
                        if let Err(e) =
                            stack.push(self.settled_task_handle(task, Err(reason)).into_value())
                        {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0507u16 => {
                        // TASK_ADOPT: preserve an existing task-backed promise; otherwise
                        // wrap the value in an already-fulfilled task handle.
                        let value = args.first().copied().unwrap_or(Value::undefined());
                        let adopted = self
                            .promise_handle_from_value(value)
                            .map(PromiseHandle::into_value)
                            .unwrap_or_else(|| {
                                self.settled_task_handle(task, Ok(value)).into_value()
                            });
                        if let Err(e) = stack.push(adopted) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::HOST_TEST262_ASYNC_DONE => {
                        let error = args.get(1).copied().unwrap_or(Value::undefined());
                        if error.is_null() || error.is_undefined() {
                            self.record_test262_async_callback_success();
                        } else {
                            self.record_test262_async_callback_failure(
                                self.format_test262_async_failure_message(error),
                            );
                        }
                        if let Err(e) = stack.push(Value::undefined()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0508u16 => {
                        let task_handle = args.first().copied().unwrap_or(Value::undefined());
                        let value = args.get(1).copied().unwrap_or(Value::undefined());
                        if let Err(error) = self.settle_existing_task_handle(task_handle, Ok(value))
                        {
                            return OpcodeResult::Error(error);
                        }
                        if let Err(e) = stack.push(Value::undefined()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x0509u16 => {
                        let task_handle = args.first().copied().unwrap_or(Value::undefined());
                        let reason = args.get(1).copied().unwrap_or(Value::undefined());
                        if let Err(error) =
                            self.settle_existing_task_handle(task_handle, Err(reason))
                        {
                            return OpcodeResult::Error(error);
                        }
                        if let Err(e) = stack.push(Value::undefined()) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x050Au16 => {
                        let source = args.first().copied().unwrap_or(Value::undefined());
                        let on_fulfilled = args.get(1).copied().unwrap_or(Value::undefined());
                        let on_rejected = args.get(2).copied().unwrap_or(Value::undefined());
                        if let Err(e) = stack.push(self.promise_chain_handle(
                            source,
                            on_fulfilled,
                            on_rejected,
                            task,
                        )) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x050Bu16 => {
                        let source = args.first().copied().unwrap_or(Value::undefined());
                        let on_finally = args.get(1).copied().unwrap_or(Value::undefined());
                        if let Err(e) =
                            stack.push(self.promise_finally_handle(source, on_finally, task))
                        {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    0x050Cu16 => {
                        let values = args.first().copied().unwrap_or(Value::undefined());
                        match self.promise_all_handle(values, task) {
                            Ok(value) => {
                                if let Err(error) = stack.push(value) {
                                    return OpcodeResult::Error(error);
                                }
                            }
                            Err(error) => return OpcodeResult::Error(error),
                        }
                        OpcodeResult::Continue
                    }
                    0x050Du16 => {
                        let values = args.first().copied().unwrap_or(Value::undefined());
                        match self.promise_race_handle(values, task) {
                            Ok(value) => {
                                if let Err(error) = stack.push(value) {
                                    return OpcodeResult::Error(error);
                                }
                            }
                            Err(error) => return OpcodeResult::Error(error),
                        }
                        OpcodeResult::Continue
                    }
                    // Error native calls
                    0x0600u16 => {
                        // ERROR_STACK (0x0600): return stack trace from error object.
                        // Stack traces are populated at throw time in exceptions.rs
                        // using the structural `stack` field surface.
                        // Normal e.stack access uses LoadFieldExact directly; this native
                        // handler serves as a fallback if called explicitly.
                        let result = if !args.is_empty() {
                            let error_val = args[0];
                            if let Some(obj_ptr) = unsafe { error_val.as_ptr::<Object>() } {
                                let obj = unsafe { &*obj_ptr.as_ptr() };
                                self.get_object_named_field_value(obj, "stack")
                                    .unwrap_or_else(|| {
                                        let s = RayaString::new(String::new());
                                        let gc_ptr = self.gc.lock().allocate(s);
                                        unsafe {
                                            Value::from_ptr(
                                                std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                                            )
                                        }
                                    })
                            } else {
                                let s = RayaString::new(String::new());
                                let gc_ptr = self.gc.lock().allocate(s);
                                unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                                    )
                                }
                            }
                        } else {
                            let s = RayaString::new(String::new());
                            let gc_ptr = self.gc.lock().allocate(s);
                            unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                            }
                        };
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == crate::compiler::native_id::ERROR_TO_STRING => {
                        let error_val = args.first().copied().unwrap_or(Value::null());
                        let name_val = self
                            .get_field_value_by_name(error_val, "name")
                            .unwrap_or(Value::null());
                        let message_val = self
                            .get_field_value_by_name(error_val, "message")
                            .unwrap_or(Value::null());

                        let to_string = |value: Value| -> String {
                            if value.is_null() {
                                return String::new();
                            }
                            if let Some(ptr) = unsafe { value.as_ptr::<RayaString>() } {
                                return unsafe { &*ptr.as_ptr() }.data.clone();
                            }
                            if let Some(i) = value.as_i32() {
                                return i.to_string();
                            }
                            if let Some(f) = value.as_f64() {
                                if f.fract() == 0.0 {
                                    return format!("{}", f as i64);
                                }
                                return f.to_string();
                            }
                            if let Some(b) = value.as_bool() {
                                return b.to_string();
                            }
                            String::new()
                        };

                        let mut name = to_string(name_val);
                        if name.is_empty() {
                            name = "Error".to_string();
                        }
                        let message = to_string(message_val);
                        let rendered = if message.is_empty() {
                            name
                        } else {
                            format!("{}: {}", name, message)
                        };
                        let s = RayaString::new(rendered);
                        let gc_ptr = self.gc.lock().allocate(s);
                        let value = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // Date native calls
                    id if id == date::NOW => {
                        use std::time::{SystemTime, UNIX_EPOCH};
                        let now = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_millis() as f64)
                            .unwrap_or(0.0);
                        if let Err(e) = stack.push(Value::f64(now)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_TIME => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0);
                        if let Err(e) = stack.push(Value::f64(timestamp)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_FULL_YEAR => {
                        // args[0] is the timestamp in milliseconds (as f64 number)
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_full_year())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_MONTH => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_month())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_DATE => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_date())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_DAY => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_day())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_HOURS => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_hours())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_MINUTES => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_minutes())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_SECONDS => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_seconds())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_MILLISECONDS => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::i32(date.get_milliseconds())) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::GET_TIMEZONE_OFFSET => {
                        if let Err(e) = stack.push(Value::i32(0)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::CONSTRUCT => {
                        let mut parts = if let Some(arg_list) = args.first().copied() {
                            match self.collect_apply_arguments(arg_list, task, module) {
                                Ok(values) => values,
                                Err(error) => return OpcodeResult::Error(error),
                            }
                        } else {
                            Vec::new()
                        };
                        if !parts.is_empty() && self.js_value_supports_extensibility(parts[0]) {
                            parts.remove(0);
                        }
                        let timestamp = if parts.is_empty() {
                            DateObject::now().timestamp_ms as f64
                        } else if parts.len() == 1 {
                            let value = parts[0];
                            if let Some(string_ptr) = unsafe { value.as_ptr::<RayaString>() } {
                                let source = unsafe { &*string_ptr.as_ptr() }.data.clone();
                                match DateObject::parse(&source) {
                                    Some(timestamp) => timestamp as f64,
                                    None => f64::NAN,
                                }
                            } else {
                                match self.js_to_number_with_context(value, task, module) {
                                    Ok(number) => number,
                                    Err(error) => return OpcodeResult::Error(error),
                                }
                            }
                        } else {
                            let mut numeric_parts = [0.0_f64; 7];
                            numeric_parts[2] = 1.0;
                            for (index, value) in parts.iter().take(7).copied().enumerate() {
                                let number = match self.js_to_number_with_context(value, task, module) {
                                    Ok(number) => number,
                                    Err(error) => return OpcodeResult::Error(error),
                                };
                                if !number.is_finite() {
                                    numeric_parts[index] = number;
                                    continue;
                                }
                                numeric_parts[index] = number.trunc();
                            }
                            if numeric_parts.iter().any(|value| value.is_nan() || value.is_infinite()) {
                                f64::NAN
                            } else {
                                let mut year = numeric_parts[0] as i32;
                                if (0..=99).contains(&year) {
                                    year += 1900;
                                }
                                DateObject::from_local_components(
                                    year,
                                    numeric_parts[1] as i32,
                                    numeric_parts[2] as i32,
                                    numeric_parts[3] as i32,
                                    numeric_parts[4] as i32,
                                    numeric_parts[5] as i32,
                                    numeric_parts[6] as i32,
                                ) as f64
                            }
                        };
                        if let Err(e) = stack.push(Value::f64(timestamp)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // Date setters: args[0]=timestamp, args[1]=new value, returns new timestamp as f64
                    id if id == date::SET_FULL_YEAR => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let val = args[1]
                            .as_f64()
                            .or_else(|| args[1].as_i64().map(|v| v as f64))
                            .or_else(|| args[1].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i32;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_full_year(val) as f64)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::SET_MONTH => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let val = args[1]
                            .as_f64()
                            .or_else(|| args[1].as_i64().map(|v| v as f64))
                            .or_else(|| args[1].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i32;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_month(val) as f64)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::SET_DATE => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let val = args[1]
                            .as_f64()
                            .or_else(|| args[1].as_i64().map(|v| v as f64))
                            .or_else(|| args[1].as_i32().map(|v| v as f64))
                            .unwrap_or(1.0) as i32;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_date(val) as f64)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::SET_HOURS => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let val = args[1]
                            .as_f64()
                            .or_else(|| args[1].as_i64().map(|v| v as f64))
                            .or_else(|| args[1].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i32;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_hours(val) as f64)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::SET_MINUTES => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let val = args[1]
                            .as_f64()
                            .or_else(|| args[1].as_i64().map(|v| v as f64))
                            .or_else(|| args[1].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i32;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_minutes(val) as f64)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::SET_SECONDS => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let val = args[1]
                            .as_f64()
                            .or_else(|| args[1].as_i64().map(|v| v as f64))
                            .or_else(|| args[1].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i32;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_seconds(val) as f64)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::SET_MILLISECONDS => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let val = args[1]
                            .as_f64()
                            .or_else(|| args[1].as_i64().map(|v| v as f64))
                            .or_else(|| args[1].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i32;
                        let date = DateObject::from_timestamp(timestamp);
                        if let Err(e) = stack.push(Value::f64(date.set_milliseconds(val) as f64)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // Date string formatting: args[0]=timestamp, returns string
                    id if id == date::TO_STRING => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        let s = RayaString::new(date.to_string_repr());
                        let gc_ptr = self.gc.lock().allocate(s);
                        let value = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::TO_ISO_STRING => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        let s = RayaString::new(date.to_iso_string());
                        let gc_ptr = self.gc.lock().allocate(s);
                        let value = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::TO_DATE_STRING => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        let s = RayaString::new(date.to_date_string());
                        let gc_ptr = self.gc.lock().allocate(s);
                        let value = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == date::TO_TIME_STRING => {
                        let timestamp = args[0]
                            .as_f64()
                            .or_else(|| args[0].as_i64().map(|v| v as f64))
                            .or_else(|| args[0].as_i32().map(|v| v as f64))
                            .unwrap_or(0.0) as i64;
                        let date = DateObject::from_timestamp(timestamp);
                        let s = RayaString::new(date.to_time_string());
                        let gc_ptr = self.gc.lock().allocate(s);
                        let value = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(value) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // Date.parse: args[0]=string, returns timestamp f64 (NaN on failure)
                    id if id == date::PARSE => {
                        let input = if !args.is_empty() && args[0].is_ptr() {
                            if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let result = match DateObject::parse(&input) {
                            Some(ts) => Value::f64(ts as f64),
                            None => Value::f64(f64::NAN),
                        };
                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // RegExp native calls
                    id if id == regexp::NEW => {
                        let pattern_arg = native_arg(&args, 0);
                        let flags_arg = native_arg(&args, 1);
                        let pattern = if pattern_arg.is_ptr() {
                            if let Some(s) = unsafe { pattern_arg.as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let flags = if flags_arg.is_ptr() {
                            if let Some(s) = unsafe { flags_arg.as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        match RegExpObject::new(&pattern, &flags) {
                            Ok(re) => {
                                let handle = self.allocate_pinned_handle(re);
                                if let Err(e) = stack.push(Value::u64(handle)) {
                                    return OpcodeResult::Error(e);
                                }
                                OpcodeResult::Continue
                            }
                            Err(e) => OpcodeResult::Error(VmError::RuntimeError(format!(
                                "Invalid regex: {}",
                                e
                            ))),
                        }
                    }
                    id if id == regexp::TEST => {
                        let regexp_arg = native_arg(&args, 0);
                        let input_arg = native_arg(&args, 1);
                        let handle = match self.regexp_handle_from_value(regexp_arg) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let input = if input_arg.is_ptr() {
                            if let Some(s) = unsafe { input_arg.as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let re_ptr = handle as *const RegExpObject;
                        if re_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid regexp handle".to_string(),
                            ));
                        }
                        let re = unsafe { &*re_ptr };
                        if let Err(e) = stack.push(Value::bool(re.test(&input))) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == regexp::EXEC => {
                        let regexp_arg = native_arg(&args, 0);
                        let input_arg = native_arg(&args, 1);
                        let handle = match self.regexp_handle_from_value(regexp_arg) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let input = if input_arg.is_ptr() {
                            if let Some(s) = unsafe { input_arg.as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let re_ptr = handle as *const RegExpObject;
                        if re_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid regexp handle".to_string(),
                            ));
                        }
                        let re = unsafe { &*re_ptr };
                        match re.exec(&input) {
                            Some((matched, index, groups)) => {
                                let mut arr = Array::new(0, 0);
                                let matched_str = RayaString::new(matched);
                                let gc_ptr = self.gc.lock().allocate(matched_str);
                                let matched_val = unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                                    )
                                };
                                arr.push(matched_val);
                                arr.push(Value::i32(index as i32));
                                for group in groups {
                                    let group_str = RayaString::new(group);
                                    let gc_ptr = self.gc.lock().allocate(group_str);
                                    let group_val = unsafe {
                                        Value::from_ptr(
                                            std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                                        )
                                    };
                                    arr.push(group_val);
                                }
                                let arr_gc = self.gc.lock().allocate(arr);
                                let arr_val = unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap(),
                                    )
                                };
                                if let Err(e) = stack.push(arr_val) {
                                    return OpcodeResult::Error(e);
                                }
                            }
                            None => {
                                if let Err(e) = stack.push(Value::null()) {
                                    return OpcodeResult::Error(e);
                                }
                            }
                        }
                        OpcodeResult::Continue
                    }
                    id if id == regexp::EXEC_ALL => {
                        let regexp_arg = native_arg(&args, 0);
                        let input_arg = native_arg(&args, 1);
                        let handle = match self.regexp_handle_from_value(regexp_arg) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let input = if input_arg.is_ptr() {
                            if let Some(s) = unsafe { input_arg.as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let re_ptr = handle as *const RegExpObject;
                        if re_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid regexp handle".to_string(),
                            ));
                        }
                        let re = unsafe { &*re_ptr };
                        let matches = re.exec_all(&input);
                        let mut result_arr = Array::new(0, 0);
                        for (matched, index, groups) in matches {
                            let mut match_arr = Array::new(0, 0);
                            let matched_str = RayaString::new(matched);
                            let gc_ptr = self.gc.lock().allocate(matched_str);
                            let matched_val = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                            };
                            match_arr.push(matched_val);
                            match_arr.push(Value::i32(index as i32));
                            for group in groups {
                                let group_str = RayaString::new(group);
                                let gc_ptr = self.gc.lock().allocate(group_str);
                                let group_val = unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                                    )
                                };
                                match_arr.push(group_val);
                            }
                            let match_arr_gc = self.gc.lock().allocate(match_arr);
                            let match_arr_val = unsafe {
                                Value::from_ptr(
                                    std::ptr::NonNull::new(match_arr_gc.as_ptr()).unwrap(),
                                )
                            };
                            result_arr.push(match_arr_val);
                        }
                        let arr_gc = self.gc.lock().allocate(result_arr);
                        let arr_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(arr_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == regexp::REPLACE => {
                        let regexp_arg = native_arg(&args, 0);
                        let input_arg = native_arg(&args, 1);
                        let replacement_arg = native_arg(&args, 2);
                        let handle = match self.regexp_handle_from_value(regexp_arg) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let input = if input_arg.is_ptr() {
                            if let Some(s) = unsafe { input_arg.as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let replacement = if replacement_arg.is_ptr() {
                            if let Some(s) = unsafe { replacement_arg.as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let re_ptr = handle as *const RegExpObject;
                        if re_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid regexp handle".to_string(),
                            ));
                        }
                        let re = unsafe { &*re_ptr };
                        let result = re.replace(&input, &replacement);
                        let result_str = RayaString::new(result);
                        let gc_ptr = self.gc.lock().allocate(result_str);
                        let result_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(result_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == regexp::SPLIT => {
                        let regexp_arg = native_arg(&args, 0);
                        let input_arg = native_arg(&args, 1);
                        let limit_arg = native_arg(&args, 2);
                        let handle = match self.regexp_handle_from_value(regexp_arg) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let input = if input_arg.is_ptr() {
                            if let Some(s) = unsafe { input_arg.as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let raw_limit = limit_arg
                            .as_i32()
                            .or_else(|| limit_arg.as_i64().map(|v| v as i32))
                            .unwrap_or(0);
                        let limit = if raw_limit > 0 {
                            Some(raw_limit as usize)
                        } else {
                            None
                        };
                        let re_ptr = handle as *const RegExpObject;
                        if re_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid regexp handle".to_string(),
                            ));
                        }
                        let re = unsafe { &*re_ptr };
                        let parts = re.split(&input, limit);
                        let mut arr = Array::new(0, 0);
                        for part in parts {
                            let s = RayaString::new(part);
                            let gc_ptr = self.gc.lock().allocate(s);
                            let val = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                            };
                            arr.push(val);
                        }
                        let arr_gc = self.gc.lock().allocate(arr);
                        let arr_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(arr_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    id if id == regexp::REPLACE_MATCHES => {
                        // REGEXP_REPLACE_MATCHES: Get match data for replaceWith intrinsic
                        // Args: regexp handle, input string
                        // Returns: array of [matched_text, start_index] arrays, respecting 'g' flag
                        let handle = match self.regexp_handle_from_value(args[0]) {
                            Ok(h) => h,
                            Err(err) => return OpcodeResult::Error(err),
                        };
                        let input = if args[1].is_ptr() {
                            if let Some(s) = unsafe { args[1].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let re_ptr = handle as *const RegExpObject;
                        if re_ptr.is_null() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "Invalid regexp handle".to_string(),
                            ));
                        }
                        let re = unsafe { &*re_ptr };
                        let is_global = re.flags.contains('g');
                        let mut result_arr = Array::new(0, 0);
                        if is_global {
                            for m in re.compiled.find_iter(&input) {
                                let mut match_arr = Array::new(0, 0);
                                let match_str = RayaString::new(m.as_str().to_string());
                                let gc_ptr = self.gc.lock().allocate(match_str);
                                let match_val = unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                                    )
                                };
                                match_arr.push(match_val);
                                match_arr.push(Value::i32(m.start() as i32));
                                let match_arr_gc = self.gc.lock().allocate(match_arr);
                                let match_arr_val = unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(match_arr_gc.as_ptr()).unwrap(),
                                    )
                                };
                                result_arr.push(match_arr_val);
                            }
                        } else if let Some(m) = re.compiled.find(&input) {
                            let mut match_arr = Array::new(0, 0);
                            let match_str = RayaString::new(m.as_str().to_string());
                            let gc_ptr = self.gc.lock().allocate(match_str);
                            let match_val = unsafe {
                                Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap())
                            };
                            match_arr.push(match_val);
                            match_arr.push(Value::i32(m.start() as i32));
                            let match_arr_gc = self.gc.lock().allocate(match_arr);
                            let match_arr_val = unsafe {
                                Value::from_ptr(
                                    std::ptr::NonNull::new(match_arr_gc.as_ptr()).unwrap(),
                                )
                            };
                            result_arr.push(match_arr_val);
                        }
                        let arr_gc = self.gc.lock().allocate(result_arr);
                        let arr_val = unsafe {
                            Value::from_ptr(std::ptr::NonNull::new(arr_gc.as_ptr()).unwrap())
                        };
                        if let Err(e) = stack.push(arr_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    // JSON.stringify
                    0x0C00 => {
                        use crate::vm::json;

                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "JSON.stringify requires 1 argument".to_string(),
                            ));
                        }
                        let value = args[0];

                        // Stringify the Value using js_classify() dispatch plus the
                        // runtime property-key registry for dynamic object lanes.
                        match json::stringify::stringify_with_runtime_metadata(
                            value,
                            |key| self.prop_key_name(key),
                            |layout_id| self.structural_layout_names(layout_id),
                        ) {
                            Ok(json_str) => {
                                let result_str = RayaString::new(json_str);
                                let gc_ptr = self.gc.lock().allocate(result_str);
                                let result_val = unsafe {
                                    Value::from_ptr(
                                        std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap(),
                                    )
                                };
                                if let Err(e) = stack.push(result_val) {
                                    return OpcodeResult::Error(e);
                                }
                            }
                            Err(e) => {
                                return OpcodeResult::Error(e);
                            }
                        }
                        OpcodeResult::Continue
                    }

                    // JSON.parse
                    0x0C01 => {
                        use crate::vm::json;

                        if args.is_empty() {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "JSON.parse requires 1 argument".to_string(),
                            ));
                        }
                        let json_str = if args[0].is_ptr() {
                            if let Some(s) = unsafe { args[0].as_ptr::<RayaString>() } {
                                unsafe { &*s.as_ptr() }.data.clone()
                            } else {
                                return OpcodeResult::Error(VmError::TypeError(
                                    "JSON.parse requires a string argument".to_string(),
                                ));
                            }
                        } else {
                            return OpcodeResult::Error(VmError::TypeError(
                                "JSON.parse requires a string argument".to_string(),
                            ));
                        };

                        // Parse JSON directly into the unified Object + dyn_map carrier
                        // used by the interpreter.
                        let result = {
                            let mut gc = self.gc.lock();
                            let mut prop_keys = self.prop_keys.write();
                            match json::parser::parse_with_prop_key_interner(
                                &json_str,
                                &mut gc,
                                &mut |name| prop_keys.intern(name),
                            ) {
                                Ok(v) => v,
                                Err(e) => return OpcodeResult::Error(e),
                            }
                        };

                        if let Err(e) = stack.push(result) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    // JSON.merge(dest, source) - copy all properties from source to dest
                    0x0C03 => {
                        if args.len() < 2 {
                            return OpcodeResult::Error(VmError::RuntimeError(
                                "JSON.merge requires 2 arguments (dest, source)".to_string(),
                            ));
                        }
                        let dest_val = args[0];
                        let source_val = args[1];

                        // If source is null/non-object, just push dest unchanged
                        if !source_val.is_ptr() {
                            if let Err(e) = stack.push(dest_val) {
                                return OpcodeResult::Error(e);
                            }
                            return OpcodeResult::Continue;
                        }

                        let pairs = self.collect_dynamic_entries(source_val);
                        if !pairs.is_empty() && dest_val.is_ptr() {
                            self.merge_dynamic_entries_into(dest_val, &pairs);
                        }

                        // Push dest back (it's been mutated in place)
                        if let Err(e) = stack.push(dest_val) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }

                    _ => {
                        // Check if this is a reflect method - pass args directly (don't push/pop)
                        if crate::vm::builtin::is_reflect_method(native_id) {
                            match self.call_reflect_method(task, stack, native_id, args, module) {
                                Ok(()) => return OpcodeResult::Continue,
                                Err(e) => return OpcodeResult::Error(e),
                            }
                        }

                        // Check if this is a runtime method (std:runtime)
                        if crate::vm::builtin::is_runtime_method(native_id) {
                            match self.call_runtime_method(task, stack, native_id, args, module) {
                                Ok(()) => return OpcodeResult::Continue,
                                Err(e) => return OpcodeResult::Error(e),
                            }
                        }

                        // Other native calls not yet implemented
                        OpcodeResult::Error(VmError::RuntimeError(format!(
                            "NativeCall {:#06x} not yet implemented in Interpreter (args={})",
                            native_id,
                            args.len()
                        )))
                    }
                }
            }

            Opcode::ModuleNativeCall => {
                use crate::vm::abi::{native_to_value, value_to_native, EngineContext};
                use raya_sdk::NativeCallResult;

                let local_idx = match Self::read_u16(code, ip) {
                    Ok(v) => v,
                    Err(e) => return OpcodeResult::Error(e),
                };
                let arg_count = match Self::read_u8(code, ip) {
                    Ok(v) => v as usize,
                    Err(e) => return OpcodeResult::Error(e),
                };

                // Pop arguments
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    match stack.pop() {
                        Ok(v) => args.push(v),
                        Err(e) => return OpcodeResult::Error(e),
                    }
                }
                args.reverse();

                // Create EngineContext for handler
                let ctx = EngineContext::new(
                    self.gc,
                    self.classes,
                    self.layouts,
                    task.id(),
                    self.class_metadata,
                );

                // Convert arguments to NativeValue (zero-cost)
                let native_args: Vec<raya_sdk::NativeValue> =
                    args.iter().map(|v| value_to_native(*v)).collect();

                // Dispatch via module-local resolved native table.
                let resolved = self.module_resolved_natives(module);
                match resolved.call(local_idx, &ctx, &native_args) {
                    NativeCallResult::Value(val) => {
                        if let Err(e) = stack.push(native_to_value(val)) {
                            return OpcodeResult::Error(e);
                        }
                        OpcodeResult::Continue
                    }
                    NativeCallResult::Suspend(io_request) => {
                        use crate::vm::scheduler::{IoSubmission, SuspendReason};
                        if let Some(tx) = self.io_submit_tx {
                            let _ = tx.send(IoSubmission {
                                task_id: task.id(),
                                request: io_request,
                            });
                        }
                        OpcodeResult::Suspend(SuspendReason::IoWait)
                    }
                    NativeCallResult::Unhandled => OpcodeResult::Error(VmError::RuntimeError(
                        format!("ModuleNativeCall index {} unhandled", local_idx),
                    )),
                    NativeCallResult::Error(msg) => OpcodeResult::Error(VmError::RuntimeError(msg)),
                }
            }

            _ => OpcodeResult::Error(VmError::RuntimeError(format!(
                "Unexpected opcode in exec_native_ops: {:?}",
                opcode
            ))),
        }
    }
}

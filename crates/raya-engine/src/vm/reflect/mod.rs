//! Reflection API Runtime
//!
//! This module implements the runtime support for the Reflect API,
//! providing metadata storage, class introspection, and type information.
//!
//! ## Native Call IDs
//!
//! Reflect methods use the 0x0Dxx range:
//! - 0x0D00-0x0D09: Metadata operations
//! - 0x0D10-0x0D1F: Class introspection
//! - 0x0D20-0x0D2F: Field access
//! - 0x0D30-0x0D3F: Method invocation
//!
//! ## Usage
//!
//! Metadata can be attached to any object (target) and optionally to
//! specific properties on that object.
//!
//! ```typescript
//! // Attach metadata to a class
//! Reflect.defineMetadata("design:type", String, MyClass);
//!
//! // Attach metadata to a property
//! Reflect.defineMetadata("design:type", Number, MyClass.prototype, "age");
//!
//! // Retrieve metadata
//! const type = Reflect.getMetadata("design:type", MyClass);
//!
//! // Class introspection (requires --emit-reflection)
//! const cls = Reflect.getClass(obj);
//! const allClasses = Reflect.getAllClasses();
//! ```

mod bootstrap;
mod bytecode_builder;
mod class_metadata;
mod dynamic_module;
mod function_builder;
mod generic_metadata;
mod introspection;
mod metadata;
mod permissions;
mod proxy;
mod runtime_builder;
mod snapshot;
mod type_builder;

pub use class_metadata::{ClassMetadata, ClassMetadataRegistry};
pub use introspection::{
    get_all_classes, get_class, get_class_by_name, get_class_hierarchy, get_class_id,
    get_type_info_for_value, is_instance_of, is_subclass_of, ConstructorInfo, DecoratorInfo,
    FieldInfo, MethodInfo, Modifiers, ParameterInfo, TypeInfo, TypeKind,
};
pub use metadata::{MetadataKey, MetadataStore, PropertyKey};
pub use proxy::{
    get_trap_method, is_proxy, prepare_get_trap, prepare_has_trap, prepare_invoke_trap,
    prepare_set_trap, try_unwrap_proxy, unwrap_proxy_deep, unwrap_proxy_target, TrapMethod,
    TrapResult, UnwrappedProxy,
};
pub use snapshot::{FieldSnapshot, ObjectDiff, ObjectSnapshot, SnapshotContext, SnapshotValue, ValueChange};
pub use type_builder::{
    DynamicClassBuilder, FieldDefinition, MethodDefinition, ParameterDefinition,
    SubclassDefinition,
};
pub use runtime_builder::{
    ClassBuilder, ClassBuilderRegistry, DynamicClosure, DynamicFunction,
    DynamicFunctionRegistry, GenericOrigin, NativeCallbackId, NativeCallbackRegistry,
    SpecializationCache,
};
pub use generic_metadata::{
    GenericParameterInfo, GenericTypeInfo, GenericTypeRegistry, SpecializedTypeInfo,
    looks_like_monomorphized, parse_monomorphized_name,
};
pub use bytecode_builder::{
    BytecodeBuilder, BytecodeBuilderRegistry, CompiledFunction, ConstantValue,
    Label, LocalVariable, StackType, ValidationResult, opcode as bc_opcode,
};
pub use permissions::{
    check_code_generation, check_field_read, check_field_write, check_invoke,
    check_type_creation, ModulePermissionRule, PermissionStore, ReflectionPermission,
};
pub use dynamic_module::{
    DynamicExport, DynamicModule, DynamicModuleInfo, DynamicModuleRegistry,
    ImportResolution, ModuleState, DYNAMIC_FUNCTION_BASE, DYNAMIC_MODULE_BASE,
};
pub use bootstrap::{
    BootstrapContext, BootstrapInfo, ExecutionOptions, ExecutionResult,
    core_class_ids, bootstrap_native_ids, is_bootstrapped, mark_bootstrapped,
};
pub use function_builder::{
    DecoratorApplication, DecoratorRegistry, DecoratorTargetType,
    FunctionWrapper, HookType, WrapperFunction, WrapperFunctionRegistry, WrapperHook,
};

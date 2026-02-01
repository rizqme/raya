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

mod class_metadata;
mod introspection;
mod metadata;

pub use class_metadata::{ClassMetadata, ClassMetadataRegistry};
pub use introspection::{
    get_all_classes, get_class, get_class_by_name, get_class_hierarchy, get_class_id,
    get_type_info_for_value, is_instance_of, is_subclass_of, ConstructorInfo, DecoratorInfo,
    FieldInfo, MethodInfo, Modifiers, ParameterInfo, TypeInfo, TypeKind,
};
pub use metadata::{MetadataKey, MetadataStore, PropertyKey};

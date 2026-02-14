//! Dynamic Type Builder for Reflection API
//!
//! Provides infrastructure for creating classes at runtime via the Reflect API.
//! This module implements Phase 10: Dynamic Subclass Creation.
//!
//! ## Native Call IDs (0x0DC0-0x0DCF)
//!
//! | ID     | Method                      | Description                          |
//! |--------|-----------------------------|------------------------------------- |
//! | 0x0DC0 | createSubclass              | Create a new subclass                |
//! | 0x0DC1 | extendWith                  | Add fields to a class                |
//! | 0x0DC2 | defineClass                 | Create a new root class              |
//! | 0x0DC3 | addMethod                   | Add method to class                  |
//! | 0x0DC4 | setConstructor              | Set class constructor                |

use crate::vm::object::{Class, VTable};
use crate::vm::reflect::{ClassMetadata, FieldInfo, MethodInfo, TypeInfo, TypeKind};
use crate::vm::value::Value;
use crate::vm::interpreter::ClassRegistry;

/// Definition for a field to be added to a dynamic class
#[derive(Debug, Clone)]
pub struct FieldDefinition {
    /// Field name
    pub name: String,
    /// Type information
    pub type_info: TypeInfo,
    /// Initial value (if any)
    pub initial_value: Option<Value>,
    /// Whether this is a static field
    pub is_static: bool,
    /// Whether this field is readonly
    pub is_readonly: bool,
}

impl FieldDefinition {
    /// Create a new field definition with minimal required information
    pub fn new(name: String, type_name: &str) -> Self {
        Self {
            name,
            type_info: TypeInfo::primitive(type_name),
            initial_value: None,
            is_static: false,
            is_readonly: false,
        }
    }

    /// Create a field definition with a class type
    pub fn with_class_type(name: String, class_name: &str, class_id: usize) -> Self {
        Self {
            name,
            type_info: TypeInfo::class(class_name, class_id),
            initial_value: None,
            is_static: false,
            is_readonly: false,
        }
    }

    /// Set the initial value
    pub fn initial_value(mut self, value: Value) -> Self {
        self.initial_value = Some(value);
        self
    }

    /// Mark as static field
    pub fn as_static(mut self) -> Self {
        self.is_static = true;
        self
    }

    /// Mark as readonly
    pub fn as_readonly(mut self) -> Self {
        self.is_readonly = true;
        self
    }
}

/// Definition for a method to be added to a dynamic class
#[derive(Debug, Clone)]
pub struct MethodDefinition {
    /// Method name
    pub name: String,
    /// Function ID for the implementation (references a compiled function)
    pub function_id: usize,
    /// Whether this is a static method
    pub is_static: bool,
    /// Whether this is an async method
    pub is_async: bool,
    /// Return type info
    pub return_type: TypeInfo,
    /// Parameter type infos
    pub parameters: Vec<ParameterDefinition>,
}

impl MethodDefinition {
    /// Create a new method definition
    pub fn new(name: String, function_id: usize) -> Self {
        Self {
            name,
            function_id,
            is_static: false,
            is_async: false,
            return_type: TypeInfo::primitive("void"),
            parameters: Vec::new(),
        }
    }

    /// Mark as static method
    pub fn as_static(mut self) -> Self {
        self.is_static = true;
        self
    }

    /// Mark as async method
    pub fn as_async(mut self) -> Self {
        self.is_async = true;
        self
    }

    /// Set return type
    pub fn returns(mut self, type_info: TypeInfo) -> Self {
        self.return_type = type_info;
        self
    }

    /// Add a parameter
    pub fn with_param(mut self, param: ParameterDefinition) -> Self {
        self.parameters.push(param);
        self
    }
}

/// Parameter definition for method signatures
#[derive(Debug, Clone)]
pub struct ParameterDefinition {
    /// Parameter name
    pub name: String,
    /// Parameter type info
    pub type_info: TypeInfo,
    /// Whether this parameter is optional
    pub is_optional: bool,
}

impl ParameterDefinition {
    /// Create a new parameter definition
    pub fn new(name: String, type_name: &str) -> Self {
        Self {
            name,
            type_info: TypeInfo::primitive(type_name),
            is_optional: false,
        }
    }

    /// Mark as optional
    pub fn optional(mut self) -> Self {
        self.is_optional = true;
        self
    }
}

/// Complete definition for creating a dynamic subclass
#[derive(Debug, Clone)]
pub struct SubclassDefinition {
    /// Fields to add to the subclass (in addition to inherited fields)
    pub fields: Vec<FieldDefinition>,
    /// Methods to add or override
    pub methods: Vec<MethodDefinition>,
    /// Constructor function ID (if any)
    pub constructor_id: Option<usize>,
    /// Interfaces implemented by this class
    pub interfaces: Vec<String>,
}

impl SubclassDefinition {
    /// Create a new empty subclass definition
    pub fn new() -> Self {
        Self {
            fields: Vec::new(),
            methods: Vec::new(),
            constructor_id: None,
            interfaces: Vec::new(),
        }
    }

    /// Add a field to the definition
    pub fn add_field(mut self, field: FieldDefinition) -> Self {
        self.fields.push(field);
        self
    }

    /// Add a method to the definition
    pub fn add_method(mut self, method: MethodDefinition) -> Self {
        self.methods.push(method);
        self
    }

    /// Set the constructor
    pub fn with_constructor(mut self, function_id: usize) -> Self {
        self.constructor_id = Some(function_id);
        self
    }

    /// Add an interface implementation
    pub fn implements(mut self, interface_name: String) -> Self {
        self.interfaces.push(interface_name);
        self
    }

    /// Count instance fields (non-static)
    pub fn instance_field_count(&self) -> usize {
        self.fields.iter().filter(|f| !f.is_static).count()
    }

    /// Count static fields
    pub fn static_field_count(&self) -> usize {
        self.fields.iter().filter(|f| f.is_static).count()
    }
}

impl Default for SubclassDefinition {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for creating dynamic classes at runtime
pub struct DynamicClassBuilder {
    /// Next class ID to allocate
    next_id: usize,
}

impl DynamicClassBuilder {
    /// Create a new builder with the next available class ID
    pub fn new(next_id: usize) -> Self {
        Self { next_id }
    }

    /// Create a new root class (no parent)
    pub fn create_root_class(
        &mut self,
        name: String,
        definition: &SubclassDefinition,
    ) -> (Class, ClassMetadata) {
        let class_id = self.next_id;
        self.next_id += 1;

        let instance_field_count = definition.instance_field_count();
        let static_field_count = definition.static_field_count();

        // Create the Class object
        let mut class = if static_field_count > 0 {
            Class::with_static_fields(class_id, name.clone(), instance_field_count, static_field_count)
        } else {
            Class::new(class_id, name.clone(), instance_field_count)
        };

        // Set constructor if provided
        if let Some(ctor_id) = definition.constructor_id {
            class.set_constructor(ctor_id);
        }

        // Add methods to vtable
        for method_def in &definition.methods {
            if !method_def.is_static {
                class.add_method(method_def.function_id);
            }
        }

        // Create metadata
        let metadata = self.build_metadata(class_id, &definition);

        (class, metadata)
    }

    /// Create a subclass extending a parent class
    pub fn create_subclass(
        &mut self,
        name: String,
        parent: &Class,
        parent_metadata: Option<&ClassMetadata>,
        definition: &SubclassDefinition,
    ) -> (Class, ClassMetadata) {
        let class_id = self.next_id;
        self.next_id += 1;

        // Calculate total field count (inherited + new)
        let inherited_fields = parent.field_count;
        let new_instance_fields = definition.instance_field_count();
        let total_field_count = inherited_fields + new_instance_fields;

        let static_field_count = definition.static_field_count();

        // Create the Class object with parent
        let mut class = Class::with_parent(
            class_id,
            name.clone(),
            total_field_count,
            parent.id,
        );

        // Copy static fields from definition
        if static_field_count > 0 {
            class.static_fields = vec![Value::null(); static_field_count];
        }

        // Set constructor if provided
        if let Some(ctor_id) = definition.constructor_id {
            class.set_constructor(ctor_id);
        }

        // Copy parent vtable and add new methods
        class.vtable = parent.vtable.clone();
        for method_def in &definition.methods {
            if !method_def.is_static {
                class.add_method(method_def.function_id);
            }
        }

        // Create metadata (inheriting from parent if available)
        let metadata = self.build_subclass_metadata(
            class_id,
            inherited_fields,
            parent_metadata,
            definition,
        );

        (class, metadata)
    }

    /// Extend an existing class with additional fields (creates a new class)
    pub fn extend_with_fields(
        &mut self,
        original: &Class,
        original_metadata: Option<&ClassMetadata>,
        new_fields: &[FieldDefinition],
    ) -> (Class, ClassMetadata) {
        let class_id = self.next_id;
        self.next_id += 1;

        // Count new instance fields
        let new_instance_fields: usize = new_fields.iter()
            .filter(|f| !f.is_static)
            .count();
        let new_static_fields: usize = new_fields.iter()
            .filter(|f| f.is_static)
            .count();

        let total_field_count = original.field_count + new_instance_fields;

        // Create new class (same parent as original, but with extended name)
        let new_name = format!("{}$Extended", original.name);
        let mut class = if let Some(parent_id) = original.parent_id {
            Class::with_parent(class_id, new_name.clone(), total_field_count, parent_id)
        } else {
            Class::new(class_id, new_name.clone(), total_field_count)
        };

        // Copy static fields (original + new)
        let total_static = original.static_field_count() + new_static_fields;
        if total_static > 0 {
            class.static_fields = vec![Value::null(); total_static];
            // Copy original static values
            for (i, val) in original.static_fields.iter().enumerate() {
                class.static_fields[i] = *val;
            }
        }

        // Copy constructor
        class.constructor_id = original.constructor_id;

        // Copy vtable
        class.vtable = original.vtable.clone();

        // Build metadata with extended fields
        let metadata = self.build_extended_metadata(
            class_id,
            original.field_count,
            original.static_field_count(),
            original_metadata,
            new_fields,
        );

        (class, metadata)
    }

    /// Build metadata for a new root class
    fn build_metadata(&self, class_id: usize, definition: &SubclassDefinition) -> ClassMetadata {
        let mut metadata = ClassMetadata::new();

        let mut field_index = 0;
        let mut static_index = 0;

        for field_def in &definition.fields {
            let field_info = FieldInfo {
                name: field_def.name.clone(),
                type_info: field_def.type_info.clone(),
                declaring_class_id: class_id,
                field_index: if field_def.is_static { static_index } else { field_index },
                is_static: field_def.is_static,
                is_readonly: field_def.is_readonly,
            };

            if field_def.is_static {
                metadata.add_static_field(field_def.name.clone(), static_index);
                static_index += 1;
            } else {
                metadata.add_field_info(field_info);
                field_index += 1;
            }
        }

        let mut method_index = 0;
        for method_def in &definition.methods {
            let method_info = MethodInfo {
                name: method_def.name.clone(),
                return_type: method_def.return_type.clone(),
                parameters: method_def.parameters.iter().enumerate().map(|(i, p)| {
                    crate::vm::reflect::ParameterInfo {
                        name: p.name.clone(),
                        type_info: p.type_info.clone(),
                        index: i,
                        is_optional: p.is_optional,
                    }
                }).collect(),
                declaring_class_id: class_id,
                method_index,
                is_static: method_def.is_static,
                is_async: method_def.is_async,
            };
            metadata.add_method_info(method_info);
            method_index += 1;
        }

        // Add interfaces
        for interface in &definition.interfaces {
            metadata.add_interface(interface.clone());
        }

        metadata
    }

    /// Build metadata for a subclass (inherits parent metadata)
    fn build_subclass_metadata(
        &self,
        class_id: usize,
        inherited_field_count: usize,
        parent_metadata: Option<&ClassMetadata>,
        definition: &SubclassDefinition,
    ) -> ClassMetadata {
        let mut metadata = ClassMetadata::new();

        // Copy parent field metadata
        if let Some(parent_meta) = parent_metadata {
            for field_info in parent_meta.get_all_field_infos() {
                metadata.add_field_info(field_info.clone());
            }
            for method_info in parent_meta.get_all_method_infos() {
                metadata.add_method_info(method_info.clone());
            }
            for interface in parent_meta.get_interfaces() {
                metadata.add_interface(interface.clone());
            }
        }

        // Add new fields starting at inherited_field_count
        let mut field_index = inherited_field_count;
        let mut static_index = 0;

        for field_def in &definition.fields {
            let field_info = FieldInfo {
                name: field_def.name.clone(),
                type_info: field_def.type_info.clone(),
                declaring_class_id: class_id,
                field_index: if field_def.is_static { static_index } else { field_index },
                is_static: field_def.is_static,
                is_readonly: field_def.is_readonly,
            };

            if field_def.is_static {
                metadata.add_static_field(field_def.name.clone(), static_index);
                static_index += 1;
            } else {
                metadata.add_field_info(field_info);
                field_index += 1;
            }
        }

        // Add new methods
        let mut method_index = metadata.methods.len();
        for method_def in &definition.methods {
            let method_info = MethodInfo {
                name: method_def.name.clone(),
                return_type: method_def.return_type.clone(),
                parameters: method_def.parameters.iter().enumerate().map(|(i, p)| {
                    crate::vm::reflect::ParameterInfo {
                        name: p.name.clone(),
                        type_info: p.type_info.clone(),
                        index: i,
                        is_optional: p.is_optional,
                    }
                }).collect(),
                declaring_class_id: class_id,
                method_index,
                is_static: method_def.is_static,
                is_async: method_def.is_async,
            };
            metadata.add_method_info(method_info);
            method_index += 1;
        }

        // Add interfaces
        for interface in &definition.interfaces {
            metadata.add_interface(interface.clone());
        }

        metadata
    }

    /// Build metadata for extended class
    fn build_extended_metadata(
        &self,
        class_id: usize,
        original_field_count: usize,
        original_static_count: usize,
        original_metadata: Option<&ClassMetadata>,
        new_fields: &[FieldDefinition],
    ) -> ClassMetadata {
        let mut metadata = ClassMetadata::new();

        // Copy original metadata
        if let Some(orig_meta) = original_metadata {
            for field_info in orig_meta.get_all_field_infos() {
                metadata.add_field_info(field_info.clone());
            }
            for method_info in orig_meta.get_all_method_infos() {
                metadata.add_method_info(method_info.clone());
            }
            for interface in orig_meta.get_interfaces() {
                metadata.add_interface(interface.clone());
            }
        }

        // Add new fields
        let mut field_index = original_field_count;
        let mut static_index = original_static_count;

        for field_def in new_fields {
            let field_info = FieldInfo {
                name: field_def.name.clone(),
                type_info: field_def.type_info.clone(),
                declaring_class_id: class_id,
                field_index: if field_def.is_static { static_index } else { field_index },
                is_static: field_def.is_static,
                is_readonly: field_def.is_readonly,
            };

            if field_def.is_static {
                metadata.add_static_field(field_def.name.clone(), static_index);
                static_index += 1;
            } else {
                metadata.add_field_info(field_info);
                field_index += 1;
            }
        }

        metadata
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field_definition() {
        let field = FieldDefinition::new("age".to_string(), "number")
            .initial_value(Value::i32(0))
            .as_readonly();

        assert_eq!(field.name, "age");
        assert_eq!(field.type_info.name, "number");
        assert!(field.is_readonly);
        assert!(!field.is_static);
        assert!(field.initial_value.is_some());
    }

    #[test]
    fn test_static_field() {
        let field = FieldDefinition::new("count".to_string(), "number")
            .as_static();

        assert!(field.is_static);
        assert!(!field.is_readonly);
    }

    #[test]
    fn test_method_definition() {
        let method = MethodDefinition::new("greet".to_string(), 42)
            .returns(TypeInfo::primitive("string"))
            .as_async();

        assert_eq!(method.name, "greet");
        assert_eq!(method.function_id, 42);
        assert!(method.is_async);
        assert!(!method.is_static);
    }

    #[test]
    fn test_subclass_definition() {
        let def = SubclassDefinition::new()
            .add_field(FieldDefinition::new("name".to_string(), "string"))
            .add_field(FieldDefinition::new("age".to_string(), "number"))
            .add_field(FieldDefinition::new("count".to_string(), "number").as_static())
            .add_method(MethodDefinition::new("greet".to_string(), 10))
            .with_constructor(5)
            .implements("Serializable".to_string());

        assert_eq!(def.instance_field_count(), 2);
        assert_eq!(def.static_field_count(), 1);
        assert_eq!(def.methods.len(), 1);
        assert_eq!(def.constructor_id, Some(5));
        assert_eq!(def.interfaces.len(), 1);
    }

    #[test]
    fn test_create_root_class() {
        let mut builder = DynamicClassBuilder::new(0);

        let def = SubclassDefinition::new()
            .add_field(FieldDefinition::new("x".to_string(), "number"))
            .add_field(FieldDefinition::new("y".to_string(), "number"));

        let (class, metadata) = builder.create_root_class("Point".to_string(), &def);

        assert_eq!(class.id, 0);
        assert_eq!(class.name, "Point");
        assert_eq!(class.field_count, 2);
        assert!(class.parent_id.is_none());

        assert!(metadata.has_field("x"));
        assert!(metadata.has_field("y"));
        assert_eq!(metadata.get_field_index("x"), Some(0));
        assert_eq!(metadata.get_field_index("y"), Some(1));
    }

    #[test]
    fn test_create_subclass() {
        let mut builder = DynamicClassBuilder::new(0);

        // Create parent class
        let parent_def = SubclassDefinition::new()
            .add_field(FieldDefinition::new("x".to_string(), "number"))
            .add_field(FieldDefinition::new("y".to_string(), "number"));
        let (parent_class, parent_meta) = builder.create_root_class("Point".to_string(), &parent_def);

        // Create subclass
        let child_def = SubclassDefinition::new()
            .add_field(FieldDefinition::new("color".to_string(), "string"));
        let (child_class, child_meta) = builder.create_subclass(
            "ColoredPoint".to_string(),
            &parent_class,
            Some(&parent_meta),
            &child_def,
        );

        assert_eq!(child_class.id, 1);
        assert_eq!(child_class.name, "ColoredPoint");
        assert_eq!(child_class.field_count, 3); // 2 inherited + 1 new
        assert_eq!(child_class.parent_id, Some(0));

        // Check inherited fields
        assert!(child_meta.has_field("x"));
        assert!(child_meta.has_field("y"));
        // Check new field
        assert!(child_meta.has_field("color"));
        assert_eq!(child_meta.get_field_index("color"), Some(2));
    }

    #[test]
    fn test_extend_with_fields() {
        let mut builder = DynamicClassBuilder::new(0);

        // Create original class
        let orig_def = SubclassDefinition::new()
            .add_field(FieldDefinition::new("x".to_string(), "number"));
        let (orig_class, orig_meta) = builder.create_root_class("Point".to_string(), &orig_def);

        // Extend with new field
        let new_fields = vec![
            FieldDefinition::new("y".to_string(), "number"),
        ];
        let (extended_class, extended_meta) = builder.extend_with_fields(
            &orig_class,
            Some(&orig_meta),
            &new_fields,
        );

        assert_eq!(extended_class.id, 1);
        assert_eq!(extended_class.name, "Point$Extended");
        assert_eq!(extended_class.field_count, 2);

        assert!(extended_meta.has_field("x"));
        assert!(extended_meta.has_field("y"));
    }
}

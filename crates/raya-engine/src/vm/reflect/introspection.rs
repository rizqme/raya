//! Class Introspection for Reflection API
//!
//! Provides runtime class information queries (requires `--emit-reflection`).
//!
//! ## Native Call IDs (0x0D10-0x0D1F)
//!
//! | ID     | Method                      | Description                          |
//! |--------|-----------------------------|------------------------------------- |
//! | 0x0D10 | getClass                    | Get class of an object               |
//! | 0x0D11 | getClassByName              | Lookup class by name                 |
//! | 0x0D12 | getAllClasses               | Get all registered classes           |
//! | 0x0D13 | getClassesWithDecorator     | Filter classes by decorator          |
//! | 0x0D14 | isSubclassOf                | Check inheritance relationship       |
//! | 0x0D15 | isInstanceOf                | Type guard for class membership      |
//! | 0x0D16 | getTypeInfo                 | Get type info for target             |
//! | 0x0D17 | getTypeInfoProperty         | Get type info for property           |

use crate::vm::object::{Class, Object};
use crate::vm::value::Value;
use crate::vm::vm::ClassRegistry;

/// Runtime type information (simplified for basic introspection)
#[derive(Debug, Clone)]
pub struct TypeInfo {
    /// Type kind
    pub kind: TypeKind,
    /// Type name
    pub name: String,
    /// Class ID (for class types)
    pub class_id: Option<usize>,
    /// Element type (for arrays)
    pub element_type: Option<Box<TypeInfo>>,
    /// Union member types
    pub union_members: Option<Vec<TypeInfo>>,
    /// Type arguments (for generics)
    pub type_arguments: Option<Vec<TypeInfo>>,
}

/// Type kind enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    /// Primitive types (number, boolean, null, string)
    Primitive,
    /// Class types
    Class,
    /// Interface types
    Interface,
    /// Union types
    Union,
    /// Function types
    Function,
    /// Array types
    Array,
    /// Generic types
    Generic,
}

impl TypeInfo {
    /// Create a primitive type info
    pub fn primitive(name: &str) -> Self {
        Self {
            kind: TypeKind::Primitive,
            name: name.to_string(),
            class_id: None,
            element_type: None,
            union_members: None,
            type_arguments: None,
        }
    }

    /// Create a class type info
    pub fn class(name: &str, class_id: usize) -> Self {
        Self {
            kind: TypeKind::Class,
            name: name.to_string(),
            class_id: Some(class_id),
            element_type: None,
            union_members: None,
            type_arguments: None,
        }
    }

    /// Create an array type info
    pub fn array(element_type: TypeInfo) -> Self {
        Self {
            kind: TypeKind::Array,
            name: format!("{}[]", element_type.name),
            class_id: None,
            element_type: Some(Box::new(element_type)),
            union_members: None,
            type_arguments: None,
        }
    }
}

/// Field information for reflection
#[derive(Debug, Clone)]
pub struct FieldInfo {
    /// Field name
    pub name: String,
    /// Field type info
    pub type_info: TypeInfo,
    /// Declaring class ID
    pub declaring_class_id: usize,
    /// Field index within the class
    pub field_index: usize,
    /// Whether the field is static
    pub is_static: bool,
    /// Whether the field is readonly
    pub is_readonly: bool,
}

/// Method information for reflection
#[derive(Debug, Clone)]
pub struct MethodInfo {
    /// Method name
    pub name: String,
    /// Return type info
    pub return_type: TypeInfo,
    /// Parameter infos
    pub parameters: Vec<ParameterInfo>,
    /// Declaring class ID
    pub declaring_class_id: usize,
    /// Method index in vtable
    pub method_index: usize,
    /// Whether the method is static
    pub is_static: bool,
    /// Whether the method is async
    pub is_async: bool,
}

/// Constructor information for reflection
#[derive(Debug, Clone)]
pub struct ConstructorInfo {
    /// Parameter infos
    pub parameters: Vec<ParameterInfo>,
    /// Declaring class ID
    pub declaring_class_id: usize,
    /// Constructor function ID
    pub function_id: usize,
}

/// Parameter information for reflection
#[derive(Debug, Clone)]
pub struct ParameterInfo {
    /// Parameter name
    pub name: String,
    /// Parameter type info
    pub type_info: TypeInfo,
    /// Parameter index
    pub index: usize,
    /// Whether the parameter is optional
    pub is_optional: bool,
}

/// Modifier flags for class members
#[derive(Debug, Clone, Copy, Default)]
pub struct Modifiers {
    /// Public visibility
    pub is_public: bool,
    /// Private visibility
    pub is_private: bool,
    /// Protected visibility
    pub is_protected: bool,
    /// Static member
    pub is_static: bool,
    /// Readonly field
    pub is_readonly: bool,
    /// Abstract member
    pub is_abstract: bool,
}

/// Decorator information for reflection
#[derive(Debug, Clone)]
pub struct DecoratorInfo {
    /// Decorator name
    pub name: String,
    /// Decorator function reference (as Value for runtime)
    pub decorator_id: usize,
    /// Decorator arguments (stored as Values)
    pub args: Vec<Value>,
}

// ============================================================================
// Introspection Functions
// ============================================================================

/// Get class ID from an object
///
/// Returns the class ID if the value is an object, None otherwise.
pub fn get_class_id(obj: Value) -> Option<usize> {
    if !obj.is_ptr() {
        return None;
    }

    // Try to interpret as Object
    let obj_ptr = unsafe { obj.as_ptr::<Object>() };
    if let Some(ptr) = obj_ptr {
        let obj_ref = unsafe { &*ptr.as_ptr() };
        return Some(obj_ref.class_id);
    }

    None
}

/// Get class by ID from registry
pub fn get_class<'a>(registry: &'a ClassRegistry, class_id: usize) -> Option<&'a Class> {
    registry.get_class(class_id)
}

/// Get class by name from registry
pub fn get_class_by_name<'a>(registry: &'a ClassRegistry, name: &str) -> Option<&'a Class> {
    registry.get_class_by_name(name)
}

/// Get all classes from registry
pub fn get_all_classes(registry: &ClassRegistry) -> Vec<&Class> {
    registry.iter().map(|(_, class)| class).collect()
}

/// Check if a class is a subclass of another class
pub fn is_subclass_of(registry: &ClassRegistry, sub_class_id: usize, super_class_id: usize) -> bool {
    if sub_class_id == super_class_id {
        return true;
    }

    let mut current_id = sub_class_id;
    while let Some(class) = registry.get_class(current_id) {
        if let Some(parent_id) = class.parent_id {
            if parent_id == super_class_id {
                return true;
            }
            current_id = parent_id;
        } else {
            break;
        }
    }

    false
}

/// Check if a value is an instance of a class
pub fn is_instance_of(registry: &ClassRegistry, obj: Value, class_id: usize) -> bool {
    if let Some(obj_class_id) = get_class_id(obj) {
        is_subclass_of(registry, obj_class_id, class_id)
    } else {
        false
    }
}

/// Get the class hierarchy (inheritance chain) for a class
///
/// Returns a vector of classes from the given class up to the root.
/// The first element is the class itself, the last is the root ancestor.
pub fn get_class_hierarchy(registry: &ClassRegistry, class_id: usize) -> Vec<&Class> {
    let mut hierarchy = Vec::new();
    let mut current_id = Some(class_id);

    while let Some(id) = current_id {
        if let Some(class) = registry.get_class(id) {
            hierarchy.push(class);
            current_id = class.parent_id;
        } else {
            break;
        }
    }

    hierarchy
}

/// Get the type info for a value (basic implementation)
pub fn get_type_info_for_value(obj: Value) -> TypeInfo {
    if obj.is_null() {
        TypeInfo::primitive("null")
    } else if obj.as_bool().is_some() {
        TypeInfo::primitive("boolean")
    } else if obj.as_i32().is_some() || obj.as_f64().is_some() {
        TypeInfo::primitive("number")
    } else if obj.is_ptr() {
        // Could be string, array, object, etc.
        // For now, return a generic object type
        TypeInfo {
            kind: TypeKind::Class,
            name: "object".to_string(),
            class_id: None,
            element_type: None,
            union_members: None,
            type_arguments: None,
        }
    } else {
        TypeInfo::primitive("unknown")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::object::Object;

    #[test]
    fn test_type_info_primitive() {
        let info = TypeInfo::primitive("number");
        assert_eq!(info.kind, TypeKind::Primitive);
        assert_eq!(info.name, "number");
        assert!(info.class_id.is_none());
    }

    #[test]
    fn test_type_info_class() {
        let info = TypeInfo::class("User", 5);
        assert_eq!(info.kind, TypeKind::Class);
        assert_eq!(info.name, "User");
        assert_eq!(info.class_id, Some(5));
    }

    #[test]
    fn test_type_info_array() {
        let elem = TypeInfo::primitive("number");
        let arr = TypeInfo::array(elem);
        assert_eq!(arr.kind, TypeKind::Array);
        assert_eq!(arr.name, "number[]");
        assert!(arr.element_type.is_some());
    }

    #[test]
    fn test_modifiers_default() {
        let mods = Modifiers::default();
        assert!(!mods.is_public);
        assert!(!mods.is_private);
        assert!(!mods.is_static);
    }

    #[test]
    fn test_field_info() {
        let field = FieldInfo {
            name: "age".to_string(),
            type_info: TypeInfo::primitive("number"),
            declaring_class_id: 0,
            field_index: 0,
            is_static: false,
            is_readonly: false,
        };
        assert_eq!(field.name, "age");
        assert!(!field.is_static);
    }

    #[test]
    fn test_is_subclass_of() {
        let mut registry = ClassRegistry::new();

        // Create class hierarchy: Animal -> Dog
        let animal = Class::new(0, "Animal".to_string(), 1);
        let dog = Class::with_parent(1, "Dog".to_string(), 2, 0);

        registry.register_class(animal);
        registry.register_class(dog);

        // Dog is subclass of Animal
        assert!(is_subclass_of(&registry, 1, 0));
        // Dog is subclass of itself
        assert!(is_subclass_of(&registry, 1, 1));
        // Animal is not subclass of Dog
        assert!(!is_subclass_of(&registry, 0, 1));
    }

    #[test]
    fn test_get_all_classes() {
        let mut registry = ClassRegistry::new();

        let class1 = Class::new(0, "Foo".to_string(), 1);
        let class2 = Class::new(1, "Bar".to_string(), 2);

        registry.register_class(class1);
        registry.register_class(class2);

        let all = get_all_classes(&registry);
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_get_type_info_for_primitives() {
        assert_eq!(get_type_info_for_value(Value::null()).name, "null");
        assert_eq!(get_type_info_for_value(Value::bool(true)).name, "boolean");
        assert_eq!(get_type_info_for_value(Value::i32(42)).name, "number");
        assert_eq!(get_type_info_for_value(Value::f64(3.14)).name, "number");
    }

    #[test]
    fn test_get_class_hierarchy() {
        let mut registry = ClassRegistry::new();

        // Create class hierarchy: Animal -> Dog -> Labrador
        let animal = Class::new(0, "Animal".to_string(), 1);
        let dog = Class::with_parent(1, "Dog".to_string(), 2, 0);
        let labrador = Class::with_parent(2, "Labrador".to_string(), 3, 1);

        registry.register_class(animal);
        registry.register_class(dog);
        registry.register_class(labrador);

        // Get hierarchy for Labrador
        let hierarchy = get_class_hierarchy(&registry, 2);
        assert_eq!(hierarchy.len(), 3);
        assert_eq!(hierarchy[0].name, "Labrador");
        assert_eq!(hierarchy[1].name, "Dog");
        assert_eq!(hierarchy[2].name, "Animal");

        // Get hierarchy for Animal (root)
        let animal_hierarchy = get_class_hierarchy(&registry, 0);
        assert_eq!(animal_hierarchy.len(), 1);
        assert_eq!(animal_hierarchy[0].name, "Animal");
    }

    #[test]
    fn test_get_class_by_name() {
        let mut registry = ClassRegistry::new();

        let point = Class::new(0, "Point".to_string(), 2);
        let circle = Class::new(1, "Circle".to_string(), 3);

        registry.register_class(point);
        registry.register_class(circle);

        // Lookup by name
        let found = get_class_by_name(&registry, "Point");
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, 0);

        let found = get_class_by_name(&registry, "Circle");
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, 1);

        // Non-existent class
        let not_found = get_class_by_name(&registry, "Unknown");
        assert!(not_found.is_none());
    }

    #[test]
    fn test_get_class() {
        let mut registry = ClassRegistry::new();

        let user = Class::new(0, "User".to_string(), 3);
        registry.register_class(user);

        // Get by ID
        let found = get_class(&registry, 0);
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "User");
        assert_eq!(found.unwrap().field_count, 3);

        // Non-existent ID
        let not_found = get_class(&registry, 99);
        assert!(not_found.is_none());
    }

    #[test]
    fn test_get_class_id_for_object() {
        use crate::vm::gc::GarbageCollector;
        use crate::vm::vm::VmContextId;
        use crate::vm::types::create_standard_registry;
        use parking_lot::Mutex;
        use std::sync::Arc;

        // Create a GC and allocate an object
        let context_id = VmContextId::new();
        let type_registry = Arc::new(create_standard_registry());
        let gc = Arc::new(Mutex::new(GarbageCollector::new(context_id, type_registry)));
        let obj = Object::new(5, 2);

        let gc_ptr = gc.lock().allocate(obj);
        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };

        // Get class ID from the object value
        let class_id = get_class_id(value);
        assert_eq!(class_id, Some(5));

        // Primitives don't have class IDs
        assert!(get_class_id(Value::null()).is_none());
        assert!(get_class_id(Value::i32(42)).is_none());
        assert!(get_class_id(Value::bool(true)).is_none());
    }

    #[test]
    fn test_is_instance_of_with_inheritance() {
        use crate::vm::gc::GarbageCollector;
        use crate::vm::vm::VmContextId;
        use crate::vm::types::create_standard_registry;
        use parking_lot::Mutex;
        use std::sync::Arc;

        let mut registry = ClassRegistry::new();

        // Create class hierarchy: Animal (0) -> Dog (1) -> Labrador (2)
        let animal = Class::new(0, "Animal".to_string(), 1);
        let dog = Class::with_parent(1, "Dog".to_string(), 2, 0);
        let labrador = Class::with_parent(2, "Labrador".to_string(), 3, 1);

        registry.register_class(animal);
        registry.register_class(dog);
        registry.register_class(labrador);

        // Create a Labrador instance (class_id = 2)
        let context_id = VmContextId::new();
        let type_registry = Arc::new(create_standard_registry());
        let gc = Arc::new(Mutex::new(GarbageCollector::new(context_id, type_registry)));
        let obj = Object::new(2, 3); // class_id = 2 (Labrador)
        let gc_ptr = gc.lock().allocate(obj);
        let lab_value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };

        // Labrador is instance of Labrador, Dog, and Animal
        assert!(is_instance_of(&registry, lab_value, 2)); // Labrador
        assert!(is_instance_of(&registry, lab_value, 1)); // Dog
        assert!(is_instance_of(&registry, lab_value, 0)); // Animal

        // Create a Dog instance
        let dog_obj = Object::new(1, 2);
        let dog_ptr = gc.lock().allocate(dog_obj);
        let dog_value = unsafe { Value::from_ptr(std::ptr::NonNull::new(dog_ptr.as_ptr()).unwrap()) };

        // Dog is instance of Dog and Animal, but not Labrador
        assert!(is_instance_of(&registry, dog_value, 1)); // Dog
        assert!(is_instance_of(&registry, dog_value, 0)); // Animal
        assert!(!is_instance_of(&registry, dog_value, 2)); // NOT Labrador

        // Primitives are not instances of any class
        assert!(!is_instance_of(&registry, Value::i32(42), 0));
    }

    #[test]
    fn test_deep_inheritance_chain() {
        let mut registry = ClassRegistry::new();

        // Create deep hierarchy: A (0) -> B (1) -> C (2) -> D (3) -> E (4)
        let a = Class::new(0, "A".to_string(), 1);
        let b = Class::with_parent(1, "B".to_string(), 1, 0);
        let c = Class::with_parent(2, "C".to_string(), 1, 1);
        let d = Class::with_parent(3, "D".to_string(), 1, 2);
        let e = Class::with_parent(4, "E".to_string(), 1, 3);

        registry.register_class(a);
        registry.register_class(b);
        registry.register_class(c);
        registry.register_class(d);
        registry.register_class(e);

        // Test hierarchy from E
        let hierarchy = get_class_hierarchy(&registry, 4);
        assert_eq!(hierarchy.len(), 5);
        assert_eq!(hierarchy[0].name, "E");
        assert_eq!(hierarchy[4].name, "A");

        // E is subclass of all ancestors
        assert!(is_subclass_of(&registry, 4, 3));
        assert!(is_subclass_of(&registry, 4, 2));
        assert!(is_subclass_of(&registry, 4, 1));
        assert!(is_subclass_of(&registry, 4, 0));

        // A is not subclass of any descendants
        assert!(!is_subclass_of(&registry, 0, 1));
        assert!(!is_subclass_of(&registry, 0, 4));
    }
}

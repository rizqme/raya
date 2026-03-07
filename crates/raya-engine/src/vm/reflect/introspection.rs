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

use crate::vm::interpreter::ClassRegistry;
use crate::vm::json::view::{js_classify, JSView};
use crate::vm::object::{Class, LayoutId};
use crate::vm::value::Value;

/// Runtime type information (simplified for basic introspection)
#[derive(Debug, Clone)]
pub struct TypeInfo {
    /// Type kind
    pub kind: TypeKind,
    /// Type name
    pub name: String,
    /// Nominal runtime type identity (for class types)
    pub nominal_type_id: Option<usize>,
    /// Physical layout identity for structural/runtime object layouts
    pub layout_id: Option<LayoutId>,
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
            nominal_type_id: None,
            layout_id: None,
            element_type: None,
            union_members: None,
            type_arguments: None,
        }
    }

    /// Create a class type info
    pub fn class(name: &str, nominal_type_id: usize) -> Self {
        Self {
            kind: TypeKind::Class,
            name: name.to_string(),
            nominal_type_id: Some(nominal_type_id),
            layout_id: None,
            element_type: None,
            union_members: None,
            type_arguments: None,
        }
    }

    /// Create a structural object type info with physical layout identity.
    pub fn structural_object(layout_id: LayoutId) -> Self {
        Self {
            kind: TypeKind::Class,
            name: "object".to_string(),
            nominal_type_id: None,
            layout_id: Some(layout_id),
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
            nominal_type_id: None,
            layout_id: None,
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
    /// Declaring nominal type ID
    pub declaring_nominal_type_id: usize,
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
    /// Declaring nominal type ID
    pub declaring_nominal_type_id: usize,
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
    /// Declaring nominal type ID
    pub declaring_nominal_type_id: usize,
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

/// Get the nominal runtime type ID from an object value.
pub fn get_nominal_type_id(obj: Value) -> Option<usize> {
    if let JSView::Struct {
        nominal_type_id: Some(id),
        ..
    } = js_classify(obj)
    {
        return Some(id as usize);
    }
    None
}

/// Get the physical layout ID from an object value.
pub fn get_layout_id(obj: Value) -> Option<LayoutId> {
    if let JSView::Struct { layout_id, .. } = js_classify(obj) {
        return Some(layout_id);
    }
    None
}

/// Get class by nominal type ID from registry.
pub fn get_class(registry: &ClassRegistry, nominal_type_id: usize) -> Option<&Class> {
    registry.get_class(nominal_type_id)
}

/// Get class by name from registry
pub fn get_class_by_name<'a>(registry: &'a ClassRegistry, name: &str) -> Option<&'a Class> {
    registry.get_class_by_name(name)
}

/// Get all classes from registry
pub fn get_all_classes(registry: &ClassRegistry) -> Vec<&Class> {
    registry.iter().map(|(_, class)| class).collect()
}

/// Check if one nominal type is a subclass of another.
pub fn is_subclass_of(
    registry: &ClassRegistry,
    sub_nominal_type_id: usize,
    super_nominal_type_id: usize,
) -> bool {
    if sub_nominal_type_id == super_nominal_type_id {
        return true;
    }

    let mut current_id = sub_nominal_type_id;
    while let Some(class) = registry.get_class(current_id) {
        if let Some(parent_id) = class.parent_id {
            if parent_id == super_nominal_type_id {
                return true;
            }
            current_id = parent_id;
        } else {
            break;
        }
    }

    false
}

/// Check if a value is an instance of a nominal type.
pub fn is_instance_of(registry: &ClassRegistry, obj: Value, nominal_type_id: usize) -> bool {
    if let Some(object_nominal_type_id) = get_nominal_type_id(obj) {
        is_subclass_of(registry, object_nominal_type_id, nominal_type_id)
    } else {
        false
    }
}

/// Get the class hierarchy (inheritance chain) for a nominal type.
///
/// Returns a vector of classes from the given nominal type up to the root.
/// The first element is the type itself, the last is the root ancestor.
pub fn get_class_hierarchy(registry: &ClassRegistry, nominal_type_id: usize) -> Vec<&Class> {
    let mut hierarchy = Vec::new();
    let mut current_id = Some(nominal_type_id);

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
    match js_classify(obj) {
        JSView::Null => TypeInfo::primitive("null"),
        JSView::Bool(_) => TypeInfo::primitive("boolean"),
        JSView::Int(_) | JSView::Number(_) => TypeInfo::primitive("number"),
        JSView::Str(_) => TypeInfo::primitive("string"),
        JSView::Arr(_) => TypeInfo::array(TypeInfo::primitive("unknown")),
        JSView::Struct {
            layout_id,
            nominal_type_id,
            ..
        } => {
            if let Some(nominal_type_id) = nominal_type_id {
                let mut info = TypeInfo::class("object", nominal_type_id as usize);
                info.layout_id = Some(layout_id);
                info
            } else {
                TypeInfo::structural_object(layout_id)
            }
        }
        JSView::Other => TypeInfo::primitive("unknown"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::object::Object;

    fn class_with_layout(id: usize, name: &str, field_count: usize) -> Class {
        Class::new(id, name.to_string(), field_count)
    }

    fn class_with_parent_and_layout(
        id: usize,
        name: &str,
        field_count: usize,
        parent_id: usize,
    ) -> Class {
        Class::with_parent(id, name.to_string(), field_count, parent_id)
    }

    #[test]
    fn test_type_info_primitive() {
        let info = TypeInfo::primitive("number");
        assert_eq!(info.kind, TypeKind::Primitive);
        assert_eq!(info.name, "number");
        assert!(info.nominal_type_id.is_none());
        assert!(info.layout_id.is_none());
    }

    #[test]
    fn test_type_info_class() {
        let info = TypeInfo::class("User", 5);
        assert_eq!(info.kind, TypeKind::Class);
        assert_eq!(info.name, "User");
        assert_eq!(info.nominal_type_id, Some(5));
        assert!(info.layout_id.is_none());
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
            declaring_nominal_type_id: 0,
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
        let animal = class_with_layout(1, "Animal", 1);
        let dog = class_with_parent_and_layout(2, "Dog", 2, 1);

        registry.register_class(animal);
        registry.register_class(dog);

        // Dog is subclass of Animal
        assert!(is_subclass_of(&registry, 2, 1));
        // Dog is subclass of itself
        assert!(is_subclass_of(&registry, 2, 2));
        // Animal is not subclass of Dog
        assert!(!is_subclass_of(&registry, 1, 2));
    }

    #[test]
    fn test_get_all_classes() {
        let mut registry = ClassRegistry::new();

        let class1 = class_with_layout(1, "Foo", 1);
        let class2 = class_with_layout(2, "Bar", 2);

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
    fn test_get_type_info_for_structural_object_exposes_layout_id() {
        use crate::vm::gc::GarbageCollector;
        use crate::vm::interpreter::VmContextId;
        use crate::vm::types::create_standard_registry;
        use parking_lot::Mutex;
        use std::sync::Arc;

        let obj = Object::new_synthetic_structural(2);
        let expected_layout = obj.layout_id();
        let context_id = VmContextId::new();
        let type_registry = Arc::new(create_standard_registry());
        let gc = Arc::new(Mutex::new(GarbageCollector::new(context_id, type_registry)));
        let ptr = gc.lock().allocate(obj);
        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(ptr.as_ptr()).unwrap()) };

        let info = get_type_info_for_value(value);
        assert_eq!(info.kind, TypeKind::Class);
        assert_eq!(info.name, "object");
        assert!(info.nominal_type_id.is_none());
        assert_eq!(info.layout_id, Some(expected_layout));
    }

    #[test]
    fn test_get_class_hierarchy() {
        let mut registry = ClassRegistry::new();

        // Create class hierarchy: Animal -> Dog -> Labrador
        let animal = class_with_layout(1, "Animal", 1);
        let dog = class_with_parent_and_layout(2, "Dog", 2, 1);
        let labrador = class_with_parent_and_layout(3, "Labrador", 3, 2);

        registry.register_class(animal);
        registry.register_class(dog);
        registry.register_class(labrador);

        // Get hierarchy for Labrador
        let hierarchy = get_class_hierarchy(&registry, 3);
        assert_eq!(hierarchy.len(), 3);
        assert_eq!(hierarchy[0].name, "Labrador");
        assert_eq!(hierarchy[1].name, "Dog");
        assert_eq!(hierarchy[2].name, "Animal");

        // Get hierarchy for Animal (root)
        let animal_hierarchy = get_class_hierarchy(&registry, 1);
        assert_eq!(animal_hierarchy.len(), 1);
        assert_eq!(animal_hierarchy[0].name, "Animal");
    }

    #[test]
    fn test_get_class_by_name() {
        let mut registry = ClassRegistry::new();

        let point = class_with_layout(1, "Point", 2);
        let circle = class_with_layout(2, "Circle", 3);

        registry.register_class(point);
        registry.register_class(circle);

        // Lookup by name
        let found = get_class_by_name(&registry, "Point");
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, 1);

        let found = get_class_by_name(&registry, "Circle");
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, 2);

        // Non-existent class
        let not_found = get_class_by_name(&registry, "Unknown");
        assert!(not_found.is_none());
    }

    #[test]
    fn test_get_class() {
        let mut registry = ClassRegistry::new();

        let user = class_with_layout(1, "User", 3);
        registry.register_class(user);

        // Get by ID
        let found = get_class(&registry, 1);
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "User");
        assert_eq!(found.unwrap().field_count, 3);

        // Non-existent ID
        let not_found = get_class(&registry, 99);
        assert!(not_found.is_none());
    }

    #[test]
    fn test_get_nominal_type_id_for_object() {
        use crate::vm::gc::GarbageCollector;
        use crate::vm::interpreter::VmContextId;
        use crate::vm::types::create_standard_registry;
        use parking_lot::Mutex;
        use std::sync::Arc;

        // Create a GC and allocate an object
        let context_id = VmContextId::new();
        let type_registry = Arc::new(create_standard_registry());
        let gc = Arc::new(Mutex::new(GarbageCollector::new(context_id, type_registry)));
        let obj = Object::new_synthetic_nominal(5, 2);

        let gc_ptr = gc.lock().allocate(obj);
        let value = unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };

        // Get nominal runtime type ID from the object value
        let nominal_type_id = get_nominal_type_id(value);
        assert_eq!(nominal_type_id, Some(5));
        assert!(get_layout_id(value).is_some());

        // Primitives don't have nominal runtime type IDs
        assert!(get_nominal_type_id(Value::null()).is_none());
        assert!(get_nominal_type_id(Value::i32(42)).is_none());
        assert!(get_nominal_type_id(Value::bool(true)).is_none());
        assert!(get_layout_id(Value::null()).is_none());
    }

    #[test]
    fn test_is_instance_of_with_inheritance() {
        use crate::vm::gc::GarbageCollector;
        use crate::vm::interpreter::VmContextId;
        use crate::vm::types::create_standard_registry;
        use parking_lot::Mutex;
        use std::sync::Arc;

        let mut registry = ClassRegistry::new();

        // Create class hierarchy: Animal (1) -> Dog (2) -> Labrador (3)
        let animal = class_with_layout(1, "Animal", 1);
        let dog = class_with_parent_and_layout(2, "Dog", 2, 1);
        let labrador = class_with_parent_and_layout(3, "Labrador", 3, 2);

        registry.register_class(animal);
        registry.register_class(dog);
        registry.register_class(labrador);

        // Create a Labrador instance (nominal_type_id = 3)
        let context_id = VmContextId::new();
        let type_registry = Arc::new(create_standard_registry());
        let gc = Arc::new(Mutex::new(GarbageCollector::new(context_id, type_registry)));
        let obj = Object::new_synthetic_nominal(3, 3); // nominal_type_id = 3 (Labrador)
        let gc_ptr = gc.lock().allocate(obj);
        let lab_value =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) };

        // Labrador is instance of Labrador, Dog, and Animal
        assert!(is_instance_of(&registry, lab_value, 3)); // Labrador
        assert!(is_instance_of(&registry, lab_value, 2)); // Dog
        assert!(is_instance_of(&registry, lab_value, 1)); // Animal

        // Create a Dog instance
        let dog_obj = Object::new_synthetic_nominal(2, 2);
        let dog_ptr = gc.lock().allocate(dog_obj);
        let dog_value =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(dog_ptr.as_ptr()).unwrap()) };

        // Dog is instance of Dog and Animal, but not Labrador
        assert!(is_instance_of(&registry, dog_value, 2)); // Dog
        assert!(is_instance_of(&registry, dog_value, 1)); // Animal
        assert!(!is_instance_of(&registry, dog_value, 3)); // NOT Labrador

        // Primitives are not instances of any class
        assert!(!is_instance_of(&registry, Value::i32(42), 1));
    }

    #[test]
    fn test_deep_inheritance_chain() {
        let mut registry = ClassRegistry::new();

        // Create deep hierarchy: A (1) -> B (2) -> C (3) -> D (4) -> E (5)
        let a = class_with_layout(1, "A", 1);
        let b = class_with_parent_and_layout(2, "B", 1, 1);
        let c = class_with_parent_and_layout(3, "C", 1, 2);
        let d = class_with_parent_and_layout(4, "D", 1, 3);
        let e = class_with_parent_and_layout(5, "E", 1, 4);

        registry.register_class(a);
        registry.register_class(b);
        registry.register_class(c);
        registry.register_class(d);
        registry.register_class(e);

        // Test hierarchy from E
        let hierarchy = get_class_hierarchy(&registry, 5);
        assert_eq!(hierarchy.len(), 5);
        assert_eq!(hierarchy[0].name, "E");
        assert_eq!(hierarchy[4].name, "A");

        // E is subclass of all ancestors
        assert!(is_subclass_of(&registry, 5, 4));
        assert!(is_subclass_of(&registry, 5, 3));
        assert!(is_subclass_of(&registry, 5, 2));
        assert!(is_subclass_of(&registry, 5, 1));

        // A is not subclass of any descendants
        assert!(!is_subclass_of(&registry, 1, 2));
        assert!(!is_subclass_of(&registry, 1, 5));
    }

    #[test]
    fn test_type_kind_values() {
        // Test all TypeKind variants
        assert_eq!(TypeKind::Primitive as u8, 0);
        assert_eq!(TypeKind::Class as u8, 1);
        assert_eq!(TypeKind::Interface as u8, 2);
        assert_eq!(TypeKind::Union as u8, 3);
        assert_eq!(TypeKind::Function as u8, 4);
        assert_eq!(TypeKind::Array as u8, 5);
        assert_eq!(TypeKind::Generic as u8, 6);
    }

    #[test]
    fn test_type_info_for_all_primitives() {
        // Test all primitive types
        let types = ["string", "number", "boolean", "null", "void", "any"];
        for type_name in types.iter() {
            let info = TypeInfo::primitive(type_name);
            assert_eq!(info.kind, TypeKind::Primitive);
            assert_eq!(info.name, *type_name);
            assert!(info.nominal_type_id.is_none());
            assert!(info.layout_id.is_none());
            assert!(info.element_type.is_none());
            assert!(info.union_members.is_none());
            assert!(info.type_arguments.is_none());
        }
    }

    #[test]
    fn test_type_info_class_with_id() {
        let info = TypeInfo::class("MyClass", 42);
        assert_eq!(info.kind, TypeKind::Class);
        assert_eq!(info.name, "MyClass");
        assert_eq!(info.nominal_type_id, Some(42));
        assert!(info.layout_id.is_none());
        assert!(info.element_type.is_none());
        assert!(info.union_members.is_none());
    }

    #[test]
    fn test_type_info_nested_array() {
        // Create number[][]
        let num = TypeInfo::primitive("number");
        let num_arr = TypeInfo::array(num);
        let num_arr_arr = TypeInfo::array(num_arr);

        assert_eq!(num_arr_arr.kind, TypeKind::Array);
        assert_eq!(num_arr_arr.name, "number[][]");

        let inner = num_arr_arr.element_type.as_ref().unwrap();
        assert_eq!(inner.kind, TypeKind::Array);
        assert_eq!(inner.name, "number[]");

        let innermost = inner.element_type.as_ref().unwrap();
        assert_eq!(innermost.kind, TypeKind::Primitive);
        assert_eq!(innermost.name, "number");
    }

    #[test]
    fn test_is_assignable_same_type() {
        // Same type is always assignable to itself
        let mut registry = ClassRegistry::new();
        let user = class_with_layout(1, "User", 2);
        registry.register_class(user);

        // Same class should be assignable to itself
        assert!(is_subclass_of(&registry, 1, 1));
    }

    #[test]
    fn test_cast_with_inheritance() {
        use crate::vm::gc::GarbageCollector;
        use crate::vm::interpreter::VmContextId;
        use crate::vm::types::create_standard_registry;
        use parking_lot::Mutex;
        use std::sync::Arc;

        let mut registry = ClassRegistry::new();

        // Animal (1) -> Dog (2)
        let animal = class_with_layout(1, "Animal", 1);
        let dog = class_with_parent_and_layout(2, "Dog", 2, 1);
        registry.register_class(animal);
        registry.register_class(dog);

        // Create a Dog instance
        let context_id = VmContextId::new();
        let type_registry = Arc::new(create_standard_registry());
        let gc = Arc::new(Mutex::new(GarbageCollector::new(context_id, type_registry)));
        let dog_obj = Object::new_synthetic_nominal(2, 2); // nominal_type_id = 2 (Dog)
        let dog_ptr = gc.lock().allocate(dog_obj);
        let dog_value =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(dog_ptr.as_ptr()).unwrap()) };

        // Dog can be cast to Animal (upcast)
        assert!(is_instance_of(&registry, dog_value, 1));
        // Dog can be cast to Dog
        assert!(is_instance_of(&registry, dog_value, 2));

        // Create an Animal instance
        let animal_obj = Object::new_synthetic_nominal(1, 1); // nominal_type_id = 1 (Animal)
        let animal_ptr = gc.lock().allocate(animal_obj);
        let animal_value =
            unsafe { Value::from_ptr(std::ptr::NonNull::new(animal_ptr.as_ptr()).unwrap()) };

        // Animal cannot be cast to Dog (downcast fails)
        assert!(!is_instance_of(&registry, animal_value, 2));
        // Animal can be cast to Animal
        assert!(is_instance_of(&registry, animal_value, 1));
    }

    #[test]
    fn test_null_is_not_instance() {
        let registry = ClassRegistry::new();

        // null is not an instance of any class
        assert!(!is_instance_of(&registry, Value::null(), 1));
        assert!(!is_instance_of(&registry, Value::null(), 999));
    }

    #[test]
    fn test_primitives_are_not_instances() {
        let registry = ClassRegistry::new();

        // Primitives are not instances of any class
        assert!(!is_instance_of(&registry, Value::i32(42), 1));
        assert!(!is_instance_of(&registry, Value::f64(3.14), 1));
        assert!(!is_instance_of(&registry, Value::bool(true), 1));
    }
}

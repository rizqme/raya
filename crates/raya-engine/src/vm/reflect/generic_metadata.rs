//! Generic Type Metadata for Reflection API
//!
//! Tracks generic type origins through monomorphization for runtime inspection.
//! When Raya compiles generic types like `Box<T>`, they become concrete types
//! like `Box_number` or `Box_string`. This module preserves that relationship.
//!
//! ## Native Call IDs (0x0DD0-0x0DDF)
//!
//! | ID     | Method                      | Description                          |
//! |--------|-----------------------------|------------------------------------- |
//! | 0x0DD0 | getGenericOrigin            | Get generic class name               |
//! | 0x0DD1 | getTypeParameters           | Get type parameter info              |
//! | 0x0DD2 | getTypeArguments            | Get actual type arguments            |
//! | 0x0DD3 | isGenericInstance           | Check if monomorphized               |
//! | 0x0DD4 | getGenericBase              | Get base generic class ID            |
//! | 0x0DD5 | findSpecializations         | Find all monomorphized versions      |

use std::collections::HashMap;

use super::introspection::TypeInfo;

/// Information about a type parameter in a generic definition
#[derive(Debug, Clone)]
pub struct GenericParameterInfo {
    /// Parameter name (e.g., "T", "K", "V")
    pub name: String,
    /// Position in the type parameter list (0-indexed)
    pub index: usize,
    /// Constraint on the type parameter (e.g., "extends Comparable")
    pub constraint: Option<TypeInfo>,
}

impl GenericParameterInfo {
    /// Create a new type parameter info
    pub fn new(name: String, index: usize) -> Self {
        Self {
            name,
            index,
            constraint: None,
        }
    }

    /// Create a type parameter with a constraint
    pub fn with_constraint(name: String, index: usize, constraint: TypeInfo) -> Self {
        Self {
            name,
            index,
            constraint: Some(constraint),
        }
    }
}

/// Complete information about a generic class
#[derive(Debug, Clone)]
pub struct GenericTypeInfo {
    /// Original generic name (e.g., "Box", "Map")
    pub name: String,
    /// Type parameters in order (e.g., ["T"] for Box<T>, ["K", "V"] for Map<K, V>)
    pub type_parameters: Vec<GenericParameterInfo>,
    /// Base generic class ID (if the generic definition was compiled)
    pub base_class_id: Option<usize>,
}

impl GenericTypeInfo {
    /// Create a new generic type info
    pub fn new(name: String) -> Self {
        Self {
            name,
            type_parameters: Vec::new(),
            base_class_id: None,
        }
    }

    /// Add a type parameter
    pub fn add_parameter(&mut self, name: String, constraint: Option<TypeInfo>) {
        let index = self.type_parameters.len();
        let param = if let Some(c) = constraint {
            GenericParameterInfo::with_constraint(name, index, c)
        } else {
            GenericParameterInfo::new(name, index)
        };
        self.type_parameters.push(param);
    }

    /// Set the base class ID
    pub fn with_base_class(mut self, class_id: usize) -> Self {
        self.base_class_id = Some(class_id);
        self
    }
}

/// Information about a monomorphized (specialized) class
#[derive(Debug, Clone)]
pub struct SpecializedTypeInfo {
    /// The specialized class ID (e.g., class ID for Box_number)
    pub class_id: usize,
    /// The specialized class name (e.g., "Box_number")
    pub class_name: String,
    /// Original generic name (e.g., "Box")
    pub generic_name: String,
    /// Type arguments in order (e.g., [TypeInfo(number)] for Box<number>)
    pub type_arguments: Vec<TypeInfo>,
}

impl SpecializedTypeInfo {
    /// Create new specialized type info
    pub fn new(
        class_id: usize,
        class_name: String,
        generic_name: String,
        type_arguments: Vec<TypeInfo>,
    ) -> Self {
        Self {
            class_id,
            class_name,
            generic_name,
            type_arguments,
        }
    }
}

/// Registry for tracking generic type relationships
///
/// This registry is populated during compilation when generic types are monomorphized.
/// It allows runtime inspection of generic type origins and specializations.
#[derive(Debug, Default)]
pub struct GenericTypeRegistry {
    /// Generic definitions: generic name -> GenericTypeInfo
    generics: HashMap<String, GenericTypeInfo>,
    /// Specialized classes: specialized class ID -> SpecializedTypeInfo
    specializations: HashMap<usize, SpecializedTypeInfo>,
    /// Reverse lookup: generic name -> list of specialized class IDs
    specialization_index: HashMap<String, Vec<usize>>,
}

impl GenericTypeRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            generics: HashMap::new(),
            specializations: HashMap::new(),
            specialization_index: HashMap::new(),
        }
    }

    /// Register a generic type definition
    pub fn register_generic(&mut self, info: GenericTypeInfo) {
        let name = info.name.clone();
        self.generics.insert(name.clone(), info);
        // Initialize the specialization index for this generic
        self.specialization_index.entry(name).or_default();
    }

    /// Register a specialized (monomorphized) type
    pub fn register_specialization(&mut self, info: SpecializedTypeInfo) {
        let generic_name = info.generic_name.clone();
        let class_id = info.class_id;

        self.specializations.insert(class_id, info);
        self.specialization_index
            .entry(generic_name)
            .or_default()
            .push(class_id);
    }

    /// Get generic type definition by name
    pub fn get_generic(&self, name: &str) -> Option<&GenericTypeInfo> {
        self.generics.get(name)
    }

    /// Get specialization info by class ID
    pub fn get_specialization(&self, class_id: usize) -> Option<&SpecializedTypeInfo> {
        self.specializations.get(&class_id)
    }

    /// Check if a class is a generic instance (monomorphized)
    pub fn is_generic_instance(&self, class_id: usize) -> bool {
        self.specializations.contains_key(&class_id)
    }

    /// Get the generic origin name for a specialized class
    pub fn get_generic_origin(&self, class_id: usize) -> Option<&str> {
        self.specializations
            .get(&class_id)
            .map(|s| s.generic_name.as_str())
    }

    /// Get type arguments for a specialized class
    pub fn get_type_arguments(&self, class_id: usize) -> Option<&[TypeInfo]> {
        self.specializations
            .get(&class_id)
            .map(|s| s.type_arguments.as_slice())
    }

    /// Get type parameters for a generic class
    pub fn get_type_parameters(&self, generic_name: &str) -> Option<&[GenericParameterInfo]> {
        self.generics
            .get(generic_name)
            .map(|g| g.type_parameters.as_slice())
    }

    /// Get base generic class ID
    pub fn get_generic_base(&self, generic_name: &str) -> Option<usize> {
        self.generics.get(generic_name).and_then(|g| g.base_class_id)
    }

    /// Find all specializations of a generic type
    pub fn find_specializations(&self, generic_name: &str) -> Vec<usize> {
        self.specialization_index
            .get(generic_name)
            .cloned()
            .unwrap_or_default()
    }

    /// Get all registered generic names
    pub fn generic_names(&self) -> impl Iterator<Item = &str> {
        self.generics.keys().map(|s| s.as_str())
    }

    /// Get count of registered generics
    pub fn generic_count(&self) -> usize {
        self.generics.len()
    }

    /// Get count of registered specializations
    pub fn specialization_count(&self) -> usize {
        self.specializations.len()
    }
}

/// Helper to parse a monomorphized class name and extract generic info
///
/// Convention: `GenericName_TypeArg1_TypeArg2` (e.g., `Box_number`, `Map_string_number`)
pub fn parse_monomorphized_name(class_name: &str) -> Option<(String, Vec<String>)> {
    // Find the first underscore that separates generic name from type args
    let parts: Vec<&str> = class_name.splitn(2, '_').collect();
    if parts.len() != 2 {
        return None;
    }

    let generic_name = parts[0].to_string();
    let type_args: Vec<String> = parts[1].split('_').map(|s| s.to_string()).collect();

    Some((generic_name, type_args))
}

/// Check if a class name looks like a monomorphized generic
pub fn looks_like_monomorphized(class_name: &str) -> bool {
    // Simple heuristic: contains underscore and first part is capitalized
    if let Some(idx) = class_name.find('_') {
        let prefix = &class_name[..idx];
        prefix.chars().next().is_some_and(|c| c.is_uppercase())
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generic_parameter_info() {
        let param = GenericParameterInfo::new("T".to_string(), 0);
        assert_eq!(param.name, "T");
        assert_eq!(param.index, 0);
        assert!(param.constraint.is_none());
    }

    #[test]
    fn test_generic_parameter_with_constraint() {
        let constraint = TypeInfo::primitive("Comparable");
        let param =
            GenericParameterInfo::with_constraint("T".to_string(), 0, constraint);
        assert!(param.constraint.is_some());
    }

    #[test]
    fn test_generic_type_info() {
        let mut info = GenericTypeInfo::new("Box".to_string());
        info.add_parameter("T".to_string(), None);

        assert_eq!(info.name, "Box");
        assert_eq!(info.type_parameters.len(), 1);
        assert_eq!(info.type_parameters[0].name, "T");
    }

    #[test]
    fn test_specialized_type_info() {
        let type_arg = TypeInfo::primitive("number");
        let info = SpecializedTypeInfo::new(
            10,
            "Box_number".to_string(),
            "Box".to_string(),
            vec![type_arg],
        );

        assert_eq!(info.class_id, 10);
        assert_eq!(info.class_name, "Box_number");
        assert_eq!(info.generic_name, "Box");
        assert_eq!(info.type_arguments.len(), 1);
    }

    #[test]
    fn test_generic_type_registry_register() {
        let mut registry = GenericTypeRegistry::new();

        // Register generic
        let mut box_generic = GenericTypeInfo::new("Box".to_string());
        box_generic.add_parameter("T".to_string(), None);
        registry.register_generic(box_generic);

        // Register specialization
        let spec = SpecializedTypeInfo::new(
            10,
            "Box_number".to_string(),
            "Box".to_string(),
            vec![TypeInfo::primitive("number")],
        );
        registry.register_specialization(spec);

        assert!(registry.get_generic("Box").is_some());
        assert!(registry.get_specialization(10).is_some());
    }

    #[test]
    fn test_is_generic_instance() {
        let mut registry = GenericTypeRegistry::new();

        let spec = SpecializedTypeInfo::new(
            10,
            "Box_number".to_string(),
            "Box".to_string(),
            vec![TypeInfo::primitive("number")],
        );
        registry.register_specialization(spec);

        assert!(registry.is_generic_instance(10));
        assert!(!registry.is_generic_instance(11));
    }

    #[test]
    fn test_get_generic_origin() {
        let mut registry = GenericTypeRegistry::new();

        let spec = SpecializedTypeInfo::new(
            10,
            "Box_number".to_string(),
            "Box".to_string(),
            vec![TypeInfo::primitive("number")],
        );
        registry.register_specialization(spec);

        assert_eq!(registry.get_generic_origin(10), Some("Box"));
        assert_eq!(registry.get_generic_origin(11), None);
    }

    #[test]
    fn test_get_type_arguments() {
        let mut registry = GenericTypeRegistry::new();

        let spec = SpecializedTypeInfo::new(
            10,
            "Box_number".to_string(),
            "Box".to_string(),
            vec![TypeInfo::primitive("number")],
        );
        registry.register_specialization(spec);

        let args = registry.get_type_arguments(10).unwrap();
        assert_eq!(args.len(), 1);
        assert_eq!(args[0].name, "number");
    }

    #[test]
    fn test_get_type_parameters() {
        let mut registry = GenericTypeRegistry::new();

        let mut box_generic = GenericTypeInfo::new("Box".to_string());
        box_generic.add_parameter("T".to_string(), None);
        registry.register_generic(box_generic);

        let params = registry.get_type_parameters("Box").unwrap();
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "T");
    }

    #[test]
    fn test_find_specializations() {
        let mut registry = GenericTypeRegistry::new();

        // Register Box generic
        let mut box_generic = GenericTypeInfo::new("Box".to_string());
        box_generic.add_parameter("T".to_string(), None);
        registry.register_generic(box_generic);

        // Register multiple specializations
        registry.register_specialization(SpecializedTypeInfo::new(
            10,
            "Box_number".to_string(),
            "Box".to_string(),
            vec![TypeInfo::primitive("number")],
        ));
        registry.register_specialization(SpecializedTypeInfo::new(
            11,
            "Box_string".to_string(),
            "Box".to_string(),
            vec![TypeInfo::primitive("string")],
        ));

        let specs = registry.find_specializations("Box");
        assert_eq!(specs.len(), 2);
        assert!(specs.contains(&10));
        assert!(specs.contains(&11));

        // No specializations for unknown generic
        let empty = registry.find_specializations("Unknown");
        assert!(empty.is_empty());
    }

    #[test]
    fn test_parse_monomorphized_name() {
        let result = parse_monomorphized_name("Box_number");
        assert!(result.is_some());
        let (name, args) = result.unwrap();
        assert_eq!(name, "Box");
        assert_eq!(args, vec!["number".to_string()]);

        let result = parse_monomorphized_name("Map_string_number");
        assert!(result.is_some());
        let (name, args) = result.unwrap();
        assert_eq!(name, "Map");
        assert_eq!(args, vec!["string".to_string(), "number".to_string()]);

        let result = parse_monomorphized_name("PlainClass");
        assert!(result.is_none());
    }

    #[test]
    fn test_looks_like_monomorphized() {
        assert!(looks_like_monomorphized("Box_number"));
        assert!(looks_like_monomorphized("Map_string_number"));
        assert!(!looks_like_monomorphized("PlainClass"));
        assert!(!looks_like_monomorphized("lowercase_class"));
    }

    #[test]
    fn test_multi_param_generic() {
        let mut registry = GenericTypeRegistry::new();

        // Register Map<K, V>
        let mut map_generic = GenericTypeInfo::new("Map".to_string());
        map_generic.add_parameter("K".to_string(), None);
        map_generic.add_parameter("V".to_string(), None);
        registry.register_generic(map_generic);

        // Register Map<string, number>
        registry.register_specialization(SpecializedTypeInfo::new(
            20,
            "Map_string_number".to_string(),
            "Map".to_string(),
            vec![
                TypeInfo::primitive("string"),
                TypeInfo::primitive("number"),
            ],
        ));

        let params = registry.get_type_parameters("Map").unwrap();
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "K");
        assert_eq!(params[1].name, "V");

        let args = registry.get_type_arguments(20).unwrap();
        assert_eq!(args.len(), 2);
        assert_eq!(args[0].name, "string");
        assert_eq!(args[1].name, "number");
    }
}

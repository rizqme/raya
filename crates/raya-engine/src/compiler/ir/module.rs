//! IR Module
//!
//! Top-level container for a compiled module.

use super::function::IrFunction;
use super::instr::{ClassId, FunctionId};
use crate::parser::TypeId;
use rustc_hash::FxHashMap;

/// An IR module (compilation unit)
#[derive(Debug, Clone)]
pub struct IrModule {
    /// Module name
    pub name: String,
    /// Functions in this module
    pub functions: Vec<IrFunction>,
    /// Classes in this module
    pub classes: Vec<IrClass>,
    /// Function lookup by name
    function_map: FxHashMap<String, FunctionId>,
    /// Class lookup by name
    class_map: FxHashMap<String, ClassId>,
}

impl IrModule {
    /// Create a new empty module
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            functions: Vec::new(),
            classes: Vec::new(),
            function_map: FxHashMap::default(),
            class_map: FxHashMap::default(),
        }
    }

    /// Add a function to the module
    pub fn add_function(&mut self, func: IrFunction) -> FunctionId {
        let id = FunctionId(self.functions.len() as u32);
        self.function_map.insert(func.name.clone(), id);
        self.functions.push(func);
        id
    }

    /// Add a class to the module
    pub fn add_class(&mut self, class: IrClass) -> ClassId {
        let id = ClassId(self.classes.len() as u32);
        self.class_map.insert(class.name.clone(), id);
        self.classes.push(class);
        id
    }

    /// Get a function by ID
    pub fn get_function(&self, id: FunctionId) -> Option<&IrFunction> {
        self.functions.get(id.0 as usize)
    }

    /// Get a function by ID mutably
    pub fn get_function_mut(&mut self, id: FunctionId) -> Option<&mut IrFunction> {
        self.functions.get_mut(id.0 as usize)
    }

    /// Get a function by name
    pub fn get_function_by_name(&self, name: &str) -> Option<&IrFunction> {
        self.function_map
            .get(name)
            .and_then(|&id| self.get_function(id))
    }

    /// Get a function ID by name
    pub fn get_function_id(&self, name: &str) -> Option<FunctionId> {
        self.function_map.get(name).copied()
    }

    /// Get a class by ID
    pub fn get_class(&self, id: ClassId) -> Option<&IrClass> {
        self.classes.get(id.0 as usize)
    }

    /// Get a class by ID mutably
    pub fn get_class_mut(&mut self, id: ClassId) -> Option<&mut IrClass> {
        self.classes.get_mut(id.0 as usize)
    }

    /// Get a class by name
    pub fn get_class_by_name(&self, name: &str) -> Option<&IrClass> {
        self.class_map
            .get(name)
            .and_then(|&id| self.get_class(id))
    }

    /// Get a class ID by name
    pub fn get_class_id(&self, name: &str) -> Option<ClassId> {
        self.class_map.get(name).copied()
    }

    /// Get the number of functions
    pub fn function_count(&self) -> usize {
        self.functions.len()
    }

    /// Get the number of classes
    pub fn class_count(&self) -> usize {
        self.classes.len()
    }

    /// Iterate over all functions
    pub fn functions(&self) -> impl Iterator<Item = &IrFunction> {
        self.functions.iter()
    }

    /// Iterate over all classes
    pub fn classes(&self) -> impl Iterator<Item = &IrClass> {
        self.classes.iter()
    }

    /// Validate the entire module
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        for (i, func) in self.functions.iter().enumerate() {
            if let Err(e) = func.validate() {
                errors.push(format!("Function '{}' ({}): {}", func.name, i, e));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Get total instruction count across all functions
    pub fn total_instruction_count(&self) -> usize {
        self.functions.iter().map(|f| f.instruction_count()).sum()
    }
}

/// An IR class definition
#[derive(Debug, Clone)]
pub struct IrClass {
    /// Class name
    pub name: String,
    /// Fields in this class
    pub fields: Vec<IrField>,
    /// Method function IDs
    pub methods: Vec<FunctionId>,
    /// Constructor function ID (if any)
    pub constructor: Option<FunctionId>,
    /// Parent class ID (if any)
    pub parent: Option<ClassId>,
}

impl IrClass {
    /// Create a new class
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            fields: Vec::new(),
            methods: Vec::new(),
            constructor: None,
            parent: None,
        }
    }

    /// Add a field to this class
    pub fn add_field(&mut self, field: IrField) -> u16 {
        let index = self.fields.len() as u16;
        self.fields.push(field);
        index
    }

    /// Add a method to this class
    pub fn add_method(&mut self, method_id: FunctionId) {
        self.methods.push(method_id);
    }

    /// Get a field by name
    pub fn get_field(&self, name: &str) -> Option<(u16, &IrField)> {
        self.fields
            .iter()
            .enumerate()
            .find(|(_, f)| f.name == name)
            .map(|(i, f)| (i as u16, f))
    }

    /// Get the number of fields
    pub fn field_count(&self) -> usize {
        self.fields.len()
    }
}

/// An IR field definition
#[derive(Debug, Clone)]
pub struct IrField {
    /// Field name
    pub name: String,
    /// Field type
    pub ty: TypeId,
    /// Field index in the object layout
    pub index: u16,
    /// Whether this field is readonly
    pub readonly: bool,
}

impl IrField {
    /// Create a new field
    pub fn new(name: impl Into<String>, ty: TypeId, index: u16) -> Self {
        Self {
            name: name.into(),
            ty,
            index,
            readonly: false,
        }
    }

    /// Create a readonly field
    pub fn readonly(name: impl Into<String>, ty: TypeId, index: u16) -> Self {
        Self {
            name: name.into(),
            ty,
            index,
            readonly: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::ir::block::{BasicBlock, BasicBlockId, Terminator};
    use crate::compiler::ir::value::{Register, RegisterId};

    fn make_simple_function(name: &str) -> IrFunction {
        let mut func = IrFunction::new(name, vec![], TypeId::new(0));
        let mut block = BasicBlock::new(BasicBlockId(0));
        block.set_terminator(Terminator::Return(None));
        func.add_block(block);
        func
    }

    #[test]
    fn test_module_new() {
        let module = IrModule::new("test_module");
        assert_eq!(module.name, "test_module");
        assert!(module.functions.is_empty());
        assert!(module.classes.is_empty());
    }

    #[test]
    fn test_module_add_function() {
        let mut module = IrModule::new("test");
        let func = make_simple_function("foo");
        let id = module.add_function(func);

        assert_eq!(id, FunctionId(0));
        assert_eq!(module.function_count(), 1);
        assert!(module.get_function(id).is_some());
        assert!(module.get_function_by_name("foo").is_some());
    }

    #[test]
    fn test_module_add_class() {
        let mut module = IrModule::new("test");
        let class = IrClass::new("MyClass");
        let id = module.add_class(class);

        assert_eq!(id, ClassId(0));
        assert_eq!(module.class_count(), 1);
        assert!(module.get_class(id).is_some());
        assert!(module.get_class_by_name("MyClass").is_some());
    }

    #[test]
    fn test_class_add_field() {
        let mut class = IrClass::new("Point");
        let idx = class.add_field(IrField::new("x", TypeId::new(1), 0));
        assert_eq!(idx, 0);
        assert_eq!(class.field_count(), 1);

        let (field_idx, field) = class.get_field("x").unwrap();
        assert_eq!(field_idx, 0);
        assert_eq!(field.name, "x");
    }

    #[test]
    fn test_module_validate() {
        let mut module = IrModule::new("test");
        let func = make_simple_function("main");
        module.add_function(func);

        assert!(module.validate().is_ok());
    }
}

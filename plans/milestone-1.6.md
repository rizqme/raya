# Milestone 1.6: Object Model

**Status:** ✅ Complete
**Goal:** Implement heap-allocated objects, arrays, and strings with class-based structure
**Dependencies:** Milestone 1.5 (Basic Bytecode Interpreter), Milestone 1.3 (GC Foundation)

---

## Overview

This milestone implements the complete object model for Raya, including heap-allocated objects with class-based structure, dynamic arrays, and string operations. Objects are allocated on the per-context heap, managed by the GC, and support field access, method dispatch via vtables, and proper memory management.

**Key Features:**
- **Object Allocation:** NEW opcode for heap-allocated objects
- **Field Access:** LOAD_FIELD/STORE_FIELD with optional fast-path optimization
- **Virtual Dispatch:** VTable-based method dispatch for polymorphism
- **Arrays:** Dynamic arrays with type safety and bounds checking
- **Strings:** Heap-allocated strings with length and concatenation
- **GC Integration:** Objects and arrays managed by garbage collector

**Foundations Already Complete:**
- ✅ Basic Object/Class/VTable structures ([object.rs:1-95](crates/raya-core/src/object.rs#L1-L95))
- ✅ Per-context heap allocation ([gc/heap.rs](crates/raya-core/src/gc/heap.rs))
- ✅ Tagged pointer Value type with heap pointer support ([value.rs:60-73](crates/raya-core/src/value.rs#L60-L73))
- ✅ Object-related opcodes defined ([opcode.rs:208-236](crates/raya-bytecode/src/opcode.rs#L208-L236))

---

## Architecture

```
┌─────────────────────────────────────────────┐
│          Object Model Runtime               │
├─────────────────────────────────────────────┤
│  Class Registry                             │
│    ├─ Class metadata (name, field count)   │
│    ├─ VTable (method function IDs)         │
│    └─ Inheritance chain                     │
├─────────────────────────────────────────────┤
│  Object Allocation                          │
│    ├─ NEW: allocate on GC heap             │
│    ├─ Initialize fields to null            │
│    └─ Return GC-managed pointer            │
├─────────────────────────────────────────────┤
│  Field Access                               │
│    ├─ LOAD_FIELD: bounds-checked read      │
│    ├─ STORE_FIELD: bounds-checked write    │
│    └─ Fast variants: inline offset         │
├─────────────────────────────────────────────┤
│  Method Dispatch                            │
│    ├─ CALL_METHOD: vtable lookup           │
│    ├─ Polymorphic dispatch                 │
│    └─ Dynamic method resolution            │
├─────────────────────────────────────────────┤
│  Arrays                                     │
│    ├─ NEW_ARRAY: allocate with length      │
│    ├─ LOAD_ELEM/STORE_ELEM: indexed access │
│    ├─ ARRAY_LEN: get array length          │
│    └─ Bounds checking on all operations    │
├─────────────────────────────────────────────┤
│  Strings                                    │
│    ├─ Heap-allocated string objects        │
│    ├─ SCONCAT: string concatenation        │
│    └─ SLEN: get string length              │
└─────────────────────────────────────────────┘
```

---

## Task Breakdown

### Task 1: Extend Object Model

**File:** `crates/raya-core/src/object.rs`

Enhance existing Object/Class structures with full functionality.

```rust
//! Object model and class system

use crate::value::Value;
use crate::gc::GcPtr;

/// Object instance (heap-allocated)
#[derive(Debug, Clone)]
pub struct Object {
    /// Class ID (index into VM class registry)
    pub class_id: usize,
    /// Field values
    pub fields: Vec<Value>,
}

impl Object {
    /// Create a new object with uninitialized fields
    pub fn new(class_id: usize, field_count: usize) -> Self {
        Self {
            class_id,
            fields: vec![Value::null(); field_count],
        }
    }

    /// Get a field value by index
    pub fn get_field(&self, index: usize) -> Option<Value> {
        self.fields.get(index).copied()
    }

    /// Set a field value by index
    pub fn set_field(&mut self, index: usize, value: Value) -> Result<(), String> {
        if index < self.fields.len() {
            self.fields[index] = value;
            Ok(())
        } else {
            Err(format!(
                "Field index {} out of bounds (object has {} fields)",
                index,
                self.fields.len()
            ))
        }
    }

    /// Get number of fields
    pub fn field_count(&self) -> usize {
        self.fields.len()
    }
}

/// Class definition metadata
#[derive(Debug, Clone)]
pub struct Class {
    /// Class ID (unique identifier)
    pub id: usize,
    /// Class name
    pub name: String,
    /// Number of fields (including inherited)
    pub field_count: usize,
    /// Parent class ID (None for root classes)
    pub parent_id: Option<usize>,
    /// Virtual method table
    pub vtable: VTable,
}

impl Class {
    /// Create a new class
    pub fn new(id: usize, name: String, field_count: usize) -> Self {
        Self {
            id,
            name,
            field_count,
            parent_id: None,
            vtable: VTable::new(),
        }
    }

    /// Create a new class with parent
    pub fn with_parent(
        id: usize,
        name: String,
        field_count: usize,
        parent_id: usize,
    ) -> Self {
        Self {
            id,
            name,
            field_count,
            parent_id: Some(parent_id),
            vtable: VTable::new(),
        }
    }

    /// Add a method to the vtable
    pub fn add_method(&mut self, function_id: usize) {
        self.vtable.add_method(function_id);
    }

    /// Get method from vtable
    pub fn get_method(&self, method_index: usize) -> Option<usize> {
        self.vtable.get_method(method_index)
    }
}

/// Virtual method table for dynamic dispatch
#[derive(Debug, Clone)]
pub struct VTable {
    /// Method function IDs (indexed by method slot)
    pub methods: Vec<usize>,
}

impl VTable {
    /// Create a new empty vtable
    pub fn new() -> Self {
        Self {
            methods: Vec::new(),
        }
    }

    /// Add a method to the vtable (appends to end)
    pub fn add_method(&mut self, function_id: usize) {
        self.methods.push(function_id);
    }

    /// Get method function ID by index
    pub fn get_method(&self, index: usize) -> Option<usize> {
        self.methods.get(index).copied()
    }

    /// Get number of methods
    pub fn method_count(&self) -> usize {
        self.methods.len()
    }

    /// Override a method at specific index
    pub fn override_method(&mut self, index: usize, function_id: usize) -> Result<(), String> {
        if index < self.methods.len() {
            self.methods[index] = function_id;
            Ok(())
        } else {
            Err(format!("Method index {} out of bounds", index))
        }
    }
}

impl Default for VTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Array object (heap-allocated)
#[derive(Debug, Clone)]
pub struct Array {
    /// Element type ID (for type checking)
    pub type_id: usize,
    /// Array elements
    pub elements: Vec<Value>,
}

impl Array {
    /// Create a new array with given length
    pub fn new(type_id: usize, length: usize) -> Self {
        Self {
            type_id,
            elements: vec![Value::null(); length],
        }
    }

    /// Get array length
    pub fn len(&self) -> usize {
        self.elements.len()
    }

    /// Check if array is empty
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// Get element at index
    pub fn get(&self, index: usize) -> Option<Value> {
        self.elements.get(index).copied()
    }

    /// Set element at index
    pub fn set(&mut self, index: usize, value: Value) -> Result<(), String> {
        if index < self.elements.len() {
            self.elements[index] = value;
            Ok(())
        } else {
            Err(format!(
                "Array index {} out of bounds (length: {})",
                index,
                self.elements.len()
            ))
        }
    }
}

/// String object (heap-allocated)
#[derive(Debug, Clone)]
pub struct RayaString {
    /// UTF-8 string data
    pub data: String,
}

impl RayaString {
    /// Create a new string
    pub fn new(data: String) -> Self {
        Self { data }
    }

    /// Get string length (in bytes)
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if string is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Concatenate two strings
    pub fn concat(&self, other: &RayaString) -> RayaString {
        RayaString::new(format!("{}{}", self.data, other.data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_creation() {
        let obj = Object::new(0, 3);
        assert_eq!(obj.field_count(), 3);
        assert_eq!(obj.class_id, 0);
    }

    #[test]
    fn test_object_field_access() {
        let mut obj = Object::new(0, 2);
        let value = Value::i32(42);

        obj.set_field(0, value).unwrap();
        assert_eq!(obj.get_field(0).unwrap(), value);

        obj.set_field(1, Value::bool(true)).unwrap();
        assert_eq!(obj.get_field(1).unwrap(), Value::bool(true));
    }

    #[test]
    fn test_object_field_bounds() {
        let mut obj = Object::new(0, 2);
        assert!(obj.set_field(2, Value::null()).is_err());
        assert_eq!(obj.get_field(10), None);
    }

    #[test]
    fn test_class_creation() {
        let class = Class::new(0, "Point".to_string(), 2);
        assert_eq!(class.id, 0);
        assert_eq!(class.name, "Point");
        assert_eq!(class.field_count, 2);
        assert_eq!(class.parent_id, None);
    }

    #[test]
    fn test_class_with_parent() {
        let class = Class::with_parent(1, "ColoredPoint".to_string(), 3, 0);
        assert_eq!(class.parent_id, Some(0));
        assert_eq!(class.field_count, 3);
    }

    #[test]
    fn test_vtable() {
        let mut vtable = VTable::new();
        vtable.add_method(10); // function ID 10
        vtable.add_method(20); // function ID 20

        assert_eq!(vtable.method_count(), 2);
        assert_eq!(vtable.get_method(0), Some(10));
        assert_eq!(vtable.get_method(1), Some(20));
        assert_eq!(vtable.get_method(2), None);
    }

    #[test]
    fn test_vtable_override() {
        let mut vtable = VTable::new();
        vtable.add_method(10);
        vtable.add_method(20);

        vtable.override_method(0, 30).unwrap();
        assert_eq!(vtable.get_method(0), Some(30));
    }

    #[test]
    fn test_array_creation() {
        let arr = Array::new(0, 5);
        assert_eq!(arr.len(), 5);
        assert_eq!(arr.type_id, 0);
    }

    #[test]
    fn test_array_access() {
        let mut arr = Array::new(0, 3);

        arr.set(0, Value::i32(10)).unwrap();
        arr.set(1, Value::i32(20)).unwrap();
        arr.set(2, Value::i32(30)).unwrap();

        assert_eq!(arr.get(0), Some(Value::i32(10)));
        assert_eq!(arr.get(1), Some(Value::i32(20)));
        assert_eq!(arr.get(2), Some(Value::i32(30)));
    }

    #[test]
    fn test_array_bounds() {
        let mut arr = Array::new(0, 2);
        assert!(arr.set(2, Value::null()).is_err());
        assert_eq!(arr.get(5), None);
    }

    #[test]
    fn test_string_creation() {
        let s = RayaString::new("hello".to_string());
        assert_eq!(s.len(), 5);
        assert_eq!(s.data, "hello");
    }

    #[test]
    fn test_string_concat() {
        let s1 = RayaString::new("hello".to_string());
        let s2 = RayaString::new(" world".to_string());
        let s3 = s1.concat(&s2);

        assert_eq!(s3.data, "hello world");
    }
}
```

**Tests:**
- [x] Object creation with field count
- [x] Field get/set operations
- [x] Field bounds checking
- [x] Class creation with and without parent
- [x] VTable method add/get/override
- [x] Array creation and access
- [x] Array bounds checking
- [x] String creation and concatenation

---

### Task 2: Add Class Registry to VM

**File:** `crates/raya-core/src/vm/mod.rs`

Add class registry for managing class definitions at runtime.

```rust
use crate::object::Class;
use std::collections::HashMap;

/// Class registry for the VM
#[derive(Debug)]
pub struct ClassRegistry {
    /// Classes indexed by ID
    classes: Vec<Class>,
    /// Class name to ID mapping
    name_to_id: HashMap<String, usize>,
}

impl ClassRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            classes: Vec::new(),
            name_to_id: HashMap::new(),
        }
    }

    /// Register a new class
    pub fn register_class(&mut self, class: Class) -> usize {
        let id = class.id;
        let name = class.name.clone();

        self.classes.push(class);
        self.name_to_id.insert(name, id);

        id
    }

    /// Get class by ID
    pub fn get_class(&self, id: usize) -> Option<&Class> {
        self.classes.get(id)
    }

    /// Get mutable class by ID
    pub fn get_class_mut(&mut self, id: usize) -> Option<&mut Class> {
        self.classes.get_mut(id)
    }

    /// Get class by name
    pub fn get_class_by_name(&self, name: &str) -> Option<&Class> {
        self.name_to_id
            .get(name)
            .and_then(|id| self.classes.get(*id))
    }

    /// Get next available class ID
    pub fn next_class_id(&self) -> usize {
        self.classes.len()
    }
}

impl Default for ClassRegistry {
    fn default() -> Self {
        Self::new()
    }
}
```

Add to VM structure in `crates/raya-core/src/vm/mod.rs`:

```rust
pub struct Vm {
    /// Execution stack
    pub stack: Stack,
    /// Class registry
    pub classes: ClassRegistry,
    /// Global variables
    pub globals: HashMap<String, Value>,
    /// VM context (heap, GC, resources)
    pub context: VmContext,
}

impl Vm {
    pub fn new() -> Self {
        Self {
            stack: Stack::new(),
            classes: ClassRegistry::new(),
            globals: HashMap::new(),
            context: VmContext::new(),
        }
    }
}
```

**Tests:**
- [ ] Register class and retrieve by ID
- [ ] Register class and retrieve by name
- [ ] Multiple class registrations
- [ ] Class ID generation

---

### Task 3: Implement Object Allocation Opcode

**File:** `crates/raya-core/src/vm/interpreter.rs`

Implement NEW opcode for heap-allocated objects.

```rust
use crate::object::{Object, Array, RayaString};
use crate::gc::GcPtr;

impl Vm {
    /// NEW - Allocate new object
    /// Operands: u16 classIndex
    fn op_new(&mut self, class_index: usize) -> VmResult<()> {
        // Look up class
        let class = self.classes
            .get_class(class_index)
            .ok_or_else(|| VmError::RuntimeError(
                format!("Invalid class index: {}", class_index)
            ))?;

        // Create object with correct field count
        let obj = Object::new(class_index, class.field_count);

        // Allocate on GC heap
        let gc_ptr = self.context.gc_mut().allocate(obj);

        // Push GC pointer as value
        let value = unsafe { Value::from_ptr(gc_ptr.as_non_null()) };
        self.stack.push(value)?;

        Ok(())
    }

    /// NEW_ARRAY - Allocate new array
    /// Operands: u16 typeIndex
    /// Stack: [length] -> [array]
    fn op_new_array(&mut self, type_index: usize) -> VmResult<()> {
        // Pop length from stack
        let length_val = self.stack.pop()?;
        let length = length_val.as_i32()
            .ok_or_else(|| VmError::TypeError(
                "Array length must be a number".to_string()
            ))? as usize;

        // Bounds check (reasonable maximum)
        if length > 10_000_000 {
            return Err(VmError::RuntimeError(
                format!("Array length {} too large", length)
            ));
        }

        // Create array
        let arr = Array::new(type_index, length);

        // Allocate on GC heap
        let gc_ptr = self.context.gc_mut().allocate(arr);

        // Push GC pointer as value
        let value = unsafe { Value::from_ptr(gc_ptr.as_non_null()) };
        self.stack.push(value)?;

        Ok(())
    }
}
```

**Tests:**
- [ ] NEW allocates object on heap
- [ ] NEW with valid class index
- [ ] NEW with invalid class index (error)
- [ ] NEW_ARRAY creates array with correct length
- [ ] NEW_ARRAY with invalid length (error)
- [ ] NEW_ARRAY with negative length (error)

---

### Task 4: Implement Field Access Opcodes

**File:** `crates/raya-core/src/vm/interpreter.rs` (continued)

Implement LOAD_FIELD and STORE_FIELD opcodes.

```rust
impl Vm {
    /// LOAD_FIELD - Load field from object
    /// Operands: u16 fieldOffset
    /// Stack: [object] -> [value]
    fn op_load_field(&mut self, field_offset: usize) -> VmResult<()> {
        // Pop object from stack
        let obj_val = self.stack.pop()?;

        // Check it's a pointer
        if !obj_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected object for field access".to_string()
            ));
        }

        // Get object from GC heap
        // SAFETY: Value is tagged as pointer, managed by GC
        let obj_ptr = unsafe { obj_val.as_ptr::<Object>() };
        let obj = unsafe { obj_ptr.as_ref() };

        // Load field
        let value = obj.get_field(field_offset)
            .ok_or_else(|| VmError::RuntimeError(
                format!("Field offset {} out of bounds", field_offset)
            ))?;

        // Push field value
        self.stack.push(value)?;

        Ok(())
    }

    /// STORE_FIELD - Store value to object field
    /// Operands: u16 fieldOffset
    /// Stack: [object, value] -> []
    fn op_store_field(&mut self, field_offset: usize) -> VmResult<()> {
        // Pop value and object from stack
        let value = self.stack.pop()?;
        let obj_val = self.stack.pop()?;

        // Check it's a pointer
        if !obj_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected object for field access".to_string()
            ));
        }

        // Get mutable object from GC heap
        // SAFETY: Value is tagged as pointer, managed by GC
        let obj_ptr = unsafe { obj_val.as_ptr::<Object>() };
        let obj = unsafe { obj_ptr.as_mut() };

        // Store field
        obj.set_field(field_offset, value)
            .map_err(|e| VmError::RuntimeError(e))?;

        Ok(())
    }

    /// LOAD_FIELD_FAST - Optimized field load with inline offset
    /// Operands: u8 offset
    /// Stack: [object] -> [value]
    fn op_load_field_fast(&mut self, offset: u8) -> VmResult<()> {
        // Delegate to regular LOAD_FIELD
        self.op_load_field(offset as usize)
    }

    /// STORE_FIELD_FAST - Optimized field store with inline offset
    /// Operands: u8 offset
    /// Stack: [object, value] -> []
    fn op_store_field_fast(&mut self, offset: u8) -> VmResult<()> {
        // Delegate to regular STORE_FIELD
        self.op_store_field(offset as usize)
    }
}
```

**Tests:**
- [ ] LOAD_FIELD reads correct field value
- [ ] LOAD_FIELD with invalid offset (error)
- [ ] LOAD_FIELD on non-object (error)
- [ ] STORE_FIELD writes correct value
- [ ] STORE_FIELD with invalid offset (error)
- [ ] STORE_FIELD on non-object (error)
- [ ] LOAD_FIELD_FAST works same as LOAD_FIELD
- [ ] STORE_FIELD_FAST works same as STORE_FIELD

---

### Task 5: Implement Array Access Opcodes

**File:** `crates/raya-core/src/vm/interpreter.rs` (continued)

Implement array element access opcodes.

```rust
impl Vm {
    /// LOAD_ELEM - Load array element
    /// Stack: [array, index] -> [value]
    fn op_load_elem(&mut self) -> VmResult<()> {
        // Pop index and array from stack
        let index_val = self.stack.pop()?;
        let array_val = self.stack.pop()?;

        // Check array is a pointer
        if !array_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected array for element access".to_string()
            ));
        }

        // Check index is a number
        let index = index_val.as_i32()
            .ok_or_else(|| VmError::TypeError(
                "Array index must be a number".to_string()
            ))? as usize;

        // Get array from GC heap
        // SAFETY: Value is tagged as pointer, managed by GC
        let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
        let arr = unsafe { arr_ptr.as_ref() };

        // Load element
        let value = arr.get(index)
            .ok_or_else(|| VmError::RuntimeError(
                format!("Array index {} out of bounds (length: {})", index, arr.len())
            ))?;

        // Push element value
        self.stack.push(value)?;

        Ok(())
    }

    /// STORE_ELEM - Store array element
    /// Stack: [array, index, value] -> []
    fn op_store_elem(&mut self) -> VmResult<()> {
        // Pop value, index, and array from stack
        let value = self.stack.pop()?;
        let index_val = self.stack.pop()?;
        let array_val = self.stack.pop()?;

        // Check array is a pointer
        if !array_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected array for element access".to_string()
            ));
        }

        // Check index is a number
        let index = index_val.as_i32()
            .ok_or_else(|| VmError::TypeError(
                "Array index must be a number".to_string()
            ))? as usize;

        // Get mutable array from GC heap
        // SAFETY: Value is tagged as pointer, managed by GC
        let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
        let arr = unsafe { arr_ptr.as_mut() };

        // Store element
        arr.set(index, value)
            .map_err(|e| VmError::RuntimeError(e))?;

        Ok(())
    }

    /// ARRAY_LEN - Get array length
    /// Stack: [array] -> [length]
    fn op_array_len(&mut self) -> VmResult<()> {
        // Pop array from stack
        let array_val = self.stack.pop()?;

        // Check array is a pointer
        if !array_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected array for length operation".to_string()
            ));
        }

        // Get array from GC heap
        // SAFETY: Value is tagged as pointer, managed by GC
        let arr_ptr = unsafe { array_val.as_ptr::<Array>() };
        let arr = unsafe { arr_ptr.as_ref() };

        // Push length as i32
        self.stack.push(Value::i32(arr.len() as i32))?;

        Ok(())
    }
}
```

**Tests:**
- [ ] LOAD_ELEM reads correct element
- [ ] LOAD_ELEM with out-of-bounds index (error)
- [ ] LOAD_ELEM on non-array (error)
- [ ] STORE_ELEM writes correct value
- [ ] STORE_ELEM with out-of-bounds index (error)
- [ ] STORE_ELEM on non-array (error)
- [ ] ARRAY_LEN returns correct length
- [ ] ARRAY_LEN on non-array (error)

---

### Task 6: Implement String Operations

**File:** `crates/raya-core/src/vm/interpreter.rs` (continued)

Implement string concatenation and length opcodes.

```rust
impl Vm {
    /// SCONCAT - Concatenate two strings
    /// Stack: [string1, string2] -> [result]
    fn op_sconcat(&mut self) -> VmResult<()> {
        // Pop two strings from stack
        let str2_val = self.stack.pop()?;
        let str1_val = self.stack.pop()?;

        // Check both are pointers
        if !str1_val.is_ptr() || !str2_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected strings for concatenation".to_string()
            ));
        }

        // Get strings from GC heap
        // SAFETY: Values are tagged as pointers, managed by GC
        let str1_ptr = unsafe { str1_val.as_ptr::<RayaString>() };
        let str2_ptr = unsafe { str2_val.as_ptr::<RayaString>() };
        let str1 = unsafe { str1_ptr.as_ref() };
        let str2 = unsafe { str2_ptr.as_ref() };

        // Concatenate
        let result = str1.concat(str2);

        // Allocate result on GC heap
        let gc_ptr = self.context.gc_mut().allocate(result);

        // Push result
        let value = unsafe { Value::from_ptr(gc_ptr.as_non_null()) };
        self.stack.push(value)?;

        Ok(())
    }

    /// SLEN - Get string length
    /// Stack: [string] -> [length]
    fn op_slen(&mut self) -> VmResult<()> {
        // Pop string from stack
        let str_val = self.stack.pop()?;

        // Check it's a pointer
        if !str_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected string for length operation".to_string()
            ));
        }

        // Get string from GC heap
        // SAFETY: Value is tagged as pointer, managed by GC
        let str_ptr = unsafe { str_val.as_ptr::<RayaString>() };
        let string = unsafe { str_ptr.as_ref() };

        // Push length as i32
        self.stack.push(Value::i32(string.len() as i32))?;

        Ok(())
    }
}
```

**Tests:**
- [ ] SCONCAT concatenates two strings correctly
- [ ] SCONCAT on non-string (error)
- [ ] SLEN returns correct length
- [ ] SLEN on non-string (error)

---

### Task 7: Implement Method Dispatch

**File:** `crates/raya-core/src/vm/interpreter.rs` (continued)

Implement CALL_METHOD opcode for virtual method dispatch.

```rust
impl Vm {
    /// CALL_METHOD - Call method via vtable dispatch
    /// Operands: u16 methodIndex, u8 argCount
    /// Stack: [object, arg1, arg2, ...] -> [result]
    fn op_call_method(
        &mut self,
        method_index: usize,
        arg_count: usize,
        module: &Module,
    ) -> VmResult<()> {
        // Peek at object (receiver) on stack without popping
        // Object is at stack position: stack_top - arg_count
        let receiver_pos = self.stack.len().checked_sub(arg_count + 1)
            .ok_or_else(|| VmError::StackUnderflow)?;

        let receiver_val = self.stack.peek(receiver_pos)?;

        // Check receiver is an object
        if !receiver_val.is_ptr() {
            return Err(VmError::TypeError(
                "Expected object for method call".to_string()
            ));
        }

        // Get object from GC heap
        // SAFETY: Value is tagged as pointer, managed by GC
        let obj_ptr = unsafe { receiver_val.as_ptr::<Object>() };
        let obj = unsafe { obj_ptr.as_ref() };

        // Look up class
        let class = self.classes
            .get_class(obj.class_id)
            .ok_or_else(|| VmError::RuntimeError(
                format!("Invalid class ID: {}", obj.class_id)
            ))?;

        // Look up method in vtable
        let function_id = class.vtable.get_method(method_index)
            .ok_or_else(|| VmError::RuntimeError(
                format!("Method index {} not found in vtable", method_index)
            ))?;

        // Get function from module
        let function = module.functions
            .get(function_id)
            .ok_or_else(|| VmError::RuntimeError(
                format!("Invalid function ID: {}", function_id)
            ))?;

        // Execute function (implementation same as CALL)
        // Arguments are already on stack in correct order
        self.execute_function(function, module)?;

        Ok(())
    }
}
```

**Tests:**
- [ ] CALL_METHOD invokes correct method
- [ ] CALL_METHOD with inheritance (vtable override)
- [ ] CALL_METHOD with invalid method index (error)
- [ ] CALL_METHOD on non-object (error)
- [ ] CALL_METHOD with correct argument passing

---

### Task 8: Integrate Opcodes into Dispatch Loop

**File:** `crates/raya-core/src/vm/interpreter.rs`

Add new opcodes to the main interpreter dispatch loop.

```rust
impl Vm {
    fn execute_function(
        &mut self,
        function: &Function,
        module: &Module,
    ) -> VmResult<Value> {
        // ... existing setup ...

        loop {
            let opcode = Opcode::from_u8(code[ip])
                .ok_or(VmError::InvalidOpcode(code[ip]))?;
            ip += 1;

            match opcode {
                // ... existing opcodes ...

                // Object operations
                Opcode::New => {
                    let class_index = self.read_u16(code, &mut ip)? as usize;
                    self.op_new(class_index)?;
                }
                Opcode::LoadField => {
                    let field_offset = self.read_u16(code, &mut ip)? as usize;
                    self.op_load_field(field_offset)?;
                }
                Opcode::StoreField => {
                    let field_offset = self.read_u16(code, &mut ip)? as usize;
                    self.op_store_field(field_offset)?;
                }
                Opcode::LoadFieldFast => {
                    let offset = self.read_u8(code, &mut ip)?;
                    self.op_load_field_fast(offset)?;
                }
                Opcode::StoreFieldFast => {
                    let offset = self.read_u8(code, &mut ip)?;
                    self.op_store_field_fast(offset)?;
                }

                // Array operations
                Opcode::NewArray => {
                    let type_index = self.read_u16(code, &mut ip)? as usize;
                    self.op_new_array(type_index)?;
                }
                Opcode::LoadElem => self.op_load_elem()?,
                Opcode::StoreElem => self.op_store_elem()?,
                Opcode::ArrayLen => self.op_array_len()?,

                // String operations
                Opcode::Sconcat => self.op_sconcat()?,
                Opcode::Slen => self.op_slen()?,

                // Method dispatch
                Opcode::CallMethod => {
                    let method_index = self.read_u16(code, &mut ip)? as usize;
                    let arg_count = self.read_u8(code, &mut ip)? as usize;
                    self.op_call_method(method_index, arg_count, module)?;
                }

                _ => {
                    return Err(VmError::RuntimeError(
                        format!("Unimplemented opcode: {:?}", opcode)
                    ));
                }
            }
        }
    }
}
```

---

## Integration Tests

**File:** `crates/raya-core/tests/object_model_tests.rs`

```rust
use raya_core::vm::Vm;
use raya_core::value::Value;
use raya_core::object::Class;
use raya_bytecode::{Module, Function, Opcode};

#[test]
fn test_object_creation_and_field_access() {
    // Create Point class with 2 fields (x, y)
    let mut vm = Vm::new();
    let point_class = Class::new(0, "Point".to_string(), 2);
    vm.classes.register_class(point_class);

    // Bytecode: new Point(), set x=10, y=20, read x
    let mut module = Module::new();
    let main_fn = Function {
        id: 0,
        name: "main".to_string(),
        param_count: 0,
        local_count: 1,
        code: vec![
            // new Point() -> local 0
            Opcode::New as u8, 0, 0,  // class index 0
            Opcode::StoreLocal as u8, 0,

            // obj.x = 10
            Opcode::LoadLocal as u8, 0,
            Opcode::ConstI32 as u8, 10, 0, 0, 0,
            Opcode::StoreField as u8, 0, 0,  // field offset 0

            // obj.y = 20
            Opcode::LoadLocal as u8, 0,
            Opcode::ConstI32 as u8, 20, 0, 0, 0,
            Opcode::StoreField as u8, 1, 0,  // field offset 1

            // return obj.x
            Opcode::LoadLocal as u8, 0,
            Opcode::LoadField as u8, 0, 0,  // field offset 0
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(10));
}

#[test]
fn test_array_operations() {
    // Bytecode: arr = new Array(3), arr[0]=10, arr[1]=20, arr[2]=30, return arr[1]
    let mut module = Module::new();
    let main_fn = Function {
        id: 0,
        name: "main".to_string(),
        param_count: 0,
        local_count: 1,
        code: vec![
            // new Array(3) -> local 0
            Opcode::ConstI32 as u8, 3, 0, 0, 0,  // length
            Opcode::NewArray as u8, 0, 0,  // type index 0
            Opcode::StoreLocal as u8, 0,

            // arr[0] = 10
            Opcode::LoadLocal as u8, 0,
            Opcode::ConstI32 as u8, 0, 0, 0, 0,  // index 0
            Opcode::ConstI32 as u8, 10, 0, 0, 0,  // value 10
            Opcode::StoreElem as u8,

            // arr[1] = 20
            Opcode::LoadLocal as u8, 0,
            Opcode::ConstI32 as u8, 1, 0, 0, 0,  // index 1
            Opcode::ConstI32 as u8, 20, 0, 0, 0,  // value 20
            Opcode::StoreElem as u8,

            // arr[2] = 30
            Opcode::LoadLocal as u8, 0,
            Opcode::ConstI32 as u8, 2, 0, 0, 0,  // index 2
            Opcode::ConstI32 as u8, 30, 0, 0, 0,  // value 30
            Opcode::StoreElem as u8,

            // return arr[1]
            Opcode::LoadLocal as u8, 0,
            Opcode::ConstI32 as u8, 1, 0, 0, 0,  // index 1
            Opcode::LoadElem as u8,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(20));
}

#[test]
fn test_array_length() {
    // Bytecode: arr = new Array(5), return arr.length
    let mut module = Module::new();
    let main_fn = Function {
        id: 0,
        name: "main".to_string(),
        param_count: 0,
        local_count: 1,
        code: vec![
            // new Array(5)
            Opcode::ConstI32 as u8, 5, 0, 0, 0,
            Opcode::NewArray as u8, 0, 0,
            Opcode::StoreLocal as u8, 0,

            // return arr.length
            Opcode::LoadLocal as u8, 0,
            Opcode::ArrayLen as u8,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let mut vm = Vm::new();
    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(5));
}

#[test]
fn test_array_bounds_check() {
    // Bytecode: arr = new Array(2), arr[5] = 10 (should fail)
    let mut module = Module::new();
    let main_fn = Function {
        id: 0,
        name: "main".to_string(),
        param_count: 0,
        local_count: 1,
        code: vec![
            Opcode::ConstI32 as u8, 2, 0, 0, 0,
            Opcode::NewArray as u8, 0, 0,
            Opcode::StoreLocal as u8, 0,

            Opcode::LoadLocal as u8, 0,
            Opcode::ConstI32 as u8, 5, 0, 0, 0,  // index 5 (out of bounds)
            Opcode::ConstI32 as u8, 10, 0, 0, 0,
            Opcode::StoreElem as u8,
            Opcode::Return as u8,
        ],
    };
    module.functions.push(main_fn);

    let mut vm = Vm::new();
    let result = vm.execute(&module);
    assert!(result.is_err());
}

#[test]
fn test_method_dispatch() {
    // Class with one method
    let mut vm = Vm::new();
    let mut point_class = Class::new(0, "Point".to_string(), 2);

    // Add method to vtable (function ID 1)
    point_class.add_method(1);
    vm.classes.register_class(point_class);

    let mut module = Module::new();

    // Function 0: main
    let main_fn = Function {
        id: 0,
        name: "main".to_string(),
        param_count: 0,
        local_count: 1,
        code: vec![
            // new Point()
            Opcode::New as u8, 0, 0,
            Opcode::StoreLocal as u8, 0,

            // obj.method(42)
            Opcode::LoadLocal as u8, 0,
            Opcode::ConstI32 as u8, 42, 0, 0, 0,
            Opcode::CallMethod as u8, 0, 0, 1,  // method 0, 1 arg
            Opcode::Return as u8,
        ],
    };

    // Function 1: method (returns argument)
    let method_fn = Function {
        id: 1,
        name: "method".to_string(),
        param_count: 2,  // self + arg
        local_count: 2,
        code: vec![
            Opcode::LoadLocal as u8, 1,  // return arg
            Opcode::Return as u8,
        ],
    };

    module.functions.push(main_fn);
    module.functions.push(method_fn);

    let result = vm.execute(&module).unwrap();
    assert_eq!(result, Value::i32(42));
}
```

---

## Acceptance Criteria

- [ ] Objects can be allocated on the heap with NEW opcode
- [ ] Field access (LOAD_FIELD, STORE_FIELD) works correctly with bounds checking
- [ ] Fast-path field access opcodes work
- [ ] Arrays can be created with NEW_ARRAY
- [ ] Array element access (LOAD_ELEM, STORE_ELEM) works with bounds checking
- [ ] ARRAY_LEN returns correct length
- [ ] String concatenation (SCONCAT) works
- [ ] String length (SLEN) works
- [ ] Method dispatch via vtables works correctly
- [ ] Class registry can register and lookup classes
- [ ] GC integration: objects/arrays allocated on heap are managed
- [ ] All object model unit tests pass
- [ ] All integration tests pass
- [ ] Error handling provides clear messages for:
  - Invalid class index
  - Field out of bounds
  - Array out of bounds
  - Type errors (non-object, non-array)
- [ ] Code coverage >85% for object model modules

---

## Reference Documentation

- **LANG.md Section 9:** Classes and class-based structure
- **LANG.md Section 11:** Arrays
- **OPCODE.md Section 3.10:** Object operations
- **OPCODE.md Section 3.11:** Array operations
- **OPCODE.md Section 3.7:** String operations
- **ARCHITECTURE.md Section 2:** Object representation
- **ARCHITECTURE.md Section 5:** GC and heap management

---

## Next Steps

After completing this milestone:

1. **Milestone 1.7:** Complete GC with precise marking for objects/arrays
2. **Milestone 1.8:** Interfaces and type system runtime support
3. **Milestone 1.9:** Task scheduler for goroutine-style concurrency

---

## Notes

### Implementation Order

1. Enhance object.rs with full Object/Class/Array/String implementations
2. Add ClassRegistry to VM
3. Implement NEW and NEW_ARRAY opcodes with GC integration
4. Implement field access (LOAD_FIELD, STORE_FIELD)
5. Implement array access (LOAD_ELEM, STORE_ELEM, ARRAY_LEN)
6. Implement string operations (SCONCAT, SLEN)
7. Implement method dispatch (CALL_METHOD)
8. Add all opcodes to dispatch loop
9. Write comprehensive integration tests
10. Test GC interaction (allocate many objects, trigger GC)

### GC Integration

Objects, arrays, and strings are heap-allocated and must be:
- Allocated via `VmContext.gc_mut().allocate()`
- Returned as GC pointers wrapped in Value
- Tracked by GC for collection
- Properly traversed during mark phase (future: Milestone 1.7)

### Performance Considerations

- Use LOAD_FIELD_FAST/STORE_FIELD_FAST for known field offsets (u8 operand vs u16)
- Consider vtable caching for hot method calls
- Array bounds checks cannot be elided (safety-first)
- String concatenation creates new allocation (consider string builder for multiple concats)

### Type Safety

All operations must check:
- Value is pointer before dereferencing
- Field/array index is in bounds
- Correct object type for operation
- GC pointer validity

### Future Enhancements

- **Field name lookup:** Runtime field access by name (reflection mode)
- **Property descriptors:** Getters/setters for computed properties
- **Array slicing:** Subarray views without copying
- **String interning:** Reduce memory for duplicate strings
- **Inline caching:** Cache vtable lookups for method dispatch
- **Shapes/Hidden classes:** Optimize object layout for similar objects

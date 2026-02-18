//! Wrapper types for complex Raya values
//!
//! These types provide ergonomic, type-safe access to Raya arrays, objects,
//! classes, functions, methods, and tasks — all through the `NativeContext`
//! trait without depending on engine internals.

use std::collections::HashMap;

use crate::context::{ClassInfo, NativeContext};
use crate::error::{AbiResult, NativeError};
use crate::value::NativeValue;

// ============================================================================
// NativeArray
// ============================================================================

/// Wrapper for a Raya array value with typed element access.
///
/// All operations delegate through `&dyn NativeContext`.
pub struct NativeArray<'a> {
    value: NativeValue,
    ctx: &'a dyn NativeContext,
}

impl<'a> NativeArray<'a> {
    /// Wrap a NativeValue as an array. Returns error if not a pointer.
    pub fn wrap(ctx: &'a dyn NativeContext, val: NativeValue) -> AbiResult<Self> {
        if !val.is_ptr() {
            return Err(NativeError::TypeMismatch {
                expected: "Array".to_string(),
                got: val.type_name().to_string(),
            });
        }
        Ok(Self { value: val, ctx })
    }

    /// Get array length
    pub fn len(&self) -> AbiResult<usize> {
        self.ctx.array_len(self.value)
    }

    /// Check if array is empty
    pub fn is_empty(&self) -> AbiResult<bool> {
        Ok(self.len()? == 0)
    }

    /// Get element at index
    pub fn get(&self, index: usize) -> AbiResult<NativeValue> {
        self.ctx.array_get(self.value, index)
    }

    /// Get element as i32
    pub fn get_i32(&self, index: usize) -> AbiResult<i32> {
        self.get(index)?
            .as_i32()
            .ok_or_else(|| NativeError::TypeMismatch {
                expected: "i32".to_string(),
                got: "other".to_string(),
            })
    }

    /// Get element as f64
    pub fn get_f64(&self, index: usize) -> AbiResult<f64> {
        self.get(index)?
            .as_f64()
            .ok_or_else(|| NativeError::TypeMismatch {
                expected: "f64".to_string(),
                got: "other".to_string(),
            })
    }

    /// Get element as bool
    pub fn get_bool(&self, index: usize) -> AbiResult<bool> {
        self.get(index)?
            .as_bool()
            .ok_or_else(|| NativeError::TypeMismatch {
                expected: "bool".to_string(),
                got: "other".to_string(),
            })
    }

    /// Get element as string
    pub fn get_string(&self, index: usize) -> AbiResult<String> {
        self.ctx.read_string(self.get(index)?)
    }

    /// Collect all elements as NativeValues
    pub fn to_vec(&self) -> AbiResult<Vec<NativeValue>> {
        let len = self.len()?;
        let mut result = Vec::with_capacity(len);
        for i in 0..len {
            result.push(self.get(i)?);
        }
        Ok(result)
    }

    /// Collect all elements as i32
    pub fn to_vec_i32(&self) -> AbiResult<Vec<i32>> {
        let len = self.len()?;
        let mut result = Vec::with_capacity(len);
        for i in 0..len {
            result.push(self.get_i32(i)?);
        }
        Ok(result)
    }

    /// Collect all elements as f64
    pub fn to_vec_f64(&self) -> AbiResult<Vec<f64>> {
        let len = self.len()?;
        let mut result = Vec::with_capacity(len);
        for i in 0..len {
            result.push(self.get_f64(i)?);
        }
        Ok(result)
    }

    /// Collect all elements as strings
    pub fn to_vec_string(&self) -> AbiResult<Vec<String>> {
        let len = self.len()?;
        let mut result = Vec::with_capacity(len);
        for i in 0..len {
            result.push(self.get_string(i)?);
        }
        Ok(result)
    }

    /// Get the underlying NativeValue
    pub fn into_value(self) -> NativeValue {
        self.value
    }

    /// Get a reference to the context
    pub fn context(&self) -> &'a dyn NativeContext {
        self.ctx
    }
}

// ============================================================================
// ObjectSchema
// ============================================================================

/// Builder for constructing ObjectSchema instances manually.
pub struct ObjectSchemaBuilder {
    class_id: usize,
    class_name: String,
    field_lookup: HashMap<String, usize>,
    field_names: Vec<String>,
    method_lookup: HashMap<String, usize>,
    method_names: Vec<String>,
}

impl ObjectSchemaBuilder {
    /// Add a field with name and index
    pub fn field(mut self, name: &str, index: usize) -> Self {
        self.field_lookup.insert(name.to_string(), index);
        // Ensure field_names is large enough
        if index >= self.field_names.len() {
            self.field_names.resize(index + 1, String::new());
        }
        self.field_names[index] = name.to_string();
        self
    }

    /// Add a method with name and vtable index
    pub fn method(mut self, name: &str, vtable_index: usize) -> Self {
        self.method_lookup.insert(name.to_string(), vtable_index);
        if vtable_index >= self.method_names.len() {
            self.method_names.resize(vtable_index + 1, String::new());
        }
        self.method_names[vtable_index] = name.to_string();
        self
    }

    /// Build the ObjectSchema
    pub fn build(self) -> ObjectSchema {
        ObjectSchema {
            class_id: self.class_id,
            class_name: self.class_name,
            field_lookup: self.field_lookup,
            field_names: self.field_names,
            method_lookup: self.method_lookup,
            method_names: self.method_names,
        }
    }
}

/// Cached schema for named field/method access on objects of a given class.
///
/// Build once per class (via `from_context` or `builder`), reuse for all
/// instances. Field lookups are `HashMap<String, usize>` → O(1).
pub struct ObjectSchema {
    class_id: usize,
    class_name: String,
    field_lookup: HashMap<String, usize>,
    field_names: Vec<String>,
    method_lookup: HashMap<String, usize>,
    method_names: Vec<String>,
}

impl ObjectSchema {
    /// Build schema from VM class metadata
    pub fn from_context(ctx: &dyn NativeContext, class_id: usize) -> AbiResult<Self> {
        let info = ctx.class_info(class_id)?;
        let fields = ctx.class_field_names(class_id)?;
        let methods = ctx.class_method_entries(class_id)?;

        let mut field_lookup = HashMap::with_capacity(fields.len());
        let mut field_names = vec![String::new(); info.field_count];
        for (name, index) in &fields {
            field_lookup.insert(name.clone(), *index);
            if *index < field_names.len() {
                field_names[*index] = name.clone();
            }
        }

        let mut method_lookup = HashMap::with_capacity(methods.len());
        let mut method_names = Vec::new();
        for (name, vtable_index) in &methods {
            method_lookup.insert(name.clone(), *vtable_index);
            if *vtable_index >= method_names.len() {
                method_names.resize(*vtable_index + 1, String::new());
            }
            method_names[*vtable_index] = name.clone();
        }

        Ok(Self {
            class_id,
            class_name: info.name,
            field_lookup,
            field_names,
            method_lookup,
            method_names,
        })
    }

    /// Create a builder for manual schema construction
    pub fn builder(class_id: usize, class_name: &str) -> ObjectSchemaBuilder {
        ObjectSchemaBuilder {
            class_id,
            class_name: class_name.to_string(),
            field_lookup: HashMap::new(),
            field_names: Vec::new(),
            method_lookup: HashMap::new(),
            method_names: Vec::new(),
        }
    }

    /// Look up field index by name
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.field_lookup.get(name).copied()
    }

    /// Look up method vtable index by name
    pub fn method_index(&self, name: &str) -> Option<usize> {
        self.method_lookup.get(name).copied()
    }

    /// Get number of fields
    pub fn field_count(&self) -> usize {
        self.field_lookup.len()
    }

    /// Get number of methods
    pub fn method_count(&self) -> usize {
        self.method_lookup.len()
    }

    /// Get class name
    pub fn class_name(&self) -> &str {
        &self.class_name
    }

    /// Get class ID
    pub fn class_id(&self) -> usize {
        self.class_id
    }

    /// Get field names in order
    pub fn field_names(&self) -> &[String] {
        &self.field_names
    }

    /// Get method names in order
    pub fn method_names(&self) -> &[String] {
        &self.method_names
    }
}

// ============================================================================
// NativeObject
// ============================================================================

/// Wrapper for a Raya object with named field access via `ObjectSchema`.
pub struct NativeObject<'a> {
    value: NativeValue,
    schema: &'a ObjectSchema,
    ctx: &'a dyn NativeContext,
}

impl<'a> NativeObject<'a> {
    /// Wrap a NativeValue as an object with the given schema.
    pub fn wrap(
        ctx: &'a dyn NativeContext,
        val: NativeValue,
        schema: &'a ObjectSchema,
    ) -> AbiResult<Self> {
        if !val.is_ptr() {
            return Err(NativeError::TypeMismatch {
                expected: format!("Object({})", schema.class_name()),
                got: val.type_name().to_string(),
            });
        }
        Ok(Self {
            value: val,
            schema,
            ctx,
        })
    }

    /// Get field by name
    pub fn get(&self, name: &str) -> AbiResult<NativeValue> {
        let index = self
            .schema
            .field_index(name)
            .ok_or_else(|| NativeError::AbiError(format!(
                "Field '{}' not found in class '{}'",
                name,
                self.schema.class_name()
            )))?;
        self.ctx.object_get_field(self.value, index)
    }

    /// Get field as i32
    pub fn get_i32(&self, name: &str) -> AbiResult<i32> {
        self.get(name)?
            .as_i32()
            .ok_or_else(|| NativeError::TypeMismatch {
                expected: "i32".to_string(),
                got: "other".to_string(),
            })
    }

    /// Get field as f64
    pub fn get_f64(&self, name: &str) -> AbiResult<f64> {
        self.get(name)?
            .as_f64()
            .ok_or_else(|| NativeError::TypeMismatch {
                expected: "f64".to_string(),
                got: "other".to_string(),
            })
    }

    /// Get field as bool
    pub fn get_bool(&self, name: &str) -> AbiResult<bool> {
        self.get(name)?
            .as_bool()
            .ok_or_else(|| NativeError::TypeMismatch {
                expected: "bool".to_string(),
                got: "other".to_string(),
            })
    }

    /// Get field as string
    pub fn get_string(&self, name: &str) -> AbiResult<String> {
        self.ctx.read_string(self.get(name)?)
    }

    /// Set field by name
    pub fn set(&self, name: &str, value: NativeValue) -> AbiResult<()> {
        let index = self
            .schema
            .field_index(name)
            .ok_or_else(|| NativeError::AbiError(format!(
                "Field '{}' not found in class '{}'",
                name,
                self.schema.class_name()
            )))?;
        self.ctx.object_set_field(self.value, index, value)
    }

    /// Set field as i32
    pub fn set_i32(&self, name: &str, value: i32) -> AbiResult<()> {
        self.set(name, NativeValue::i32(value))
    }

    /// Set field as f64
    pub fn set_f64(&self, name: &str, value: f64) -> AbiResult<()> {
        self.set(name, NativeValue::f64(value))
    }

    /// Get object's class ID
    pub fn class_id(&self) -> AbiResult<usize> {
        self.ctx.object_class_id(self.value)
    }

    /// Get the schema
    pub fn schema(&self) -> &ObjectSchema {
        self.schema
    }

    /// Get the underlying NativeValue
    pub fn into_value(self) -> NativeValue {
        self.value
    }

    /// Get a reference to the context
    pub fn context(&self) -> &'a dyn NativeContext {
        self.ctx
    }
}

// ============================================================================
// NativeClass
// ============================================================================

/// Information about a Raya class with convenience methods.
#[derive(Debug, Clone)]
pub struct NativeClass {
    info: ClassInfo,
}

impl NativeClass {
    /// Look up class by ID
    pub fn from_id(ctx: &dyn NativeContext, class_id: usize) -> AbiResult<Self> {
        Ok(Self {
            info: ctx.class_info(class_id)?,
        })
    }

    /// Look up class by name (searches exported classes)
    pub fn from_name(ctx: &dyn NativeContext, name: &str) -> AbiResult<Self> {
        Ok(Self {
            info: ctx.class_by_name(name)?,
        })
    }

    /// Get class ID
    pub fn id(&self) -> usize {
        self.info.class_id
    }

    /// Get class name
    pub fn name(&self) -> &str {
        &self.info.name
    }

    /// Get field count
    pub fn field_count(&self) -> usize {
        self.info.field_count
    }

    /// Get parent class ID
    pub fn parent_id(&self) -> Option<usize> {
        self.info.parent_id
    }

    /// Get constructor function ID
    pub fn constructor_id(&self) -> Option<usize> {
        self.info.constructor_id
    }

    /// Get method count
    pub fn method_count(&self) -> usize {
        self.info.method_count
    }

    /// Build ObjectSchema for this class from VM metadata
    pub fn schema(&self, ctx: &dyn NativeContext) -> AbiResult<ObjectSchema> {
        ObjectSchema::from_context(ctx, self.info.class_id)
    }

    /// Allocate a new instance of this class
    pub fn instantiate(&self, ctx: &dyn NativeContext) -> AbiResult<NativeValue> {
        ctx.create_object_by_id(self.info.class_id)
    }
}

// ============================================================================
// NativeFunction
// ============================================================================

/// Wrapper for a Raya closure/function value.
pub struct NativeFunction<'a> {
    value: NativeValue,
    func_id: usize,
    ctx: &'a dyn NativeContext,
}

impl<'a> NativeFunction<'a> {
    /// Wrap a closure NativeValue
    pub fn new(ctx: &'a dyn NativeContext, val: NativeValue, func_id: usize) -> Self {
        Self {
            value: val,
            func_id,
            ctx,
        }
    }

    /// Get function ID
    pub fn func_id(&self) -> usize {
        self.func_id
    }

    /// Call this function synchronously (blocks until complete)
    pub fn call(&self, args: &[NativeValue]) -> AbiResult<NativeValue> {
        self.ctx.call_function(self.func_id, args)
    }

    /// Call as async task (returns immediately with task handle)
    pub fn call_async(&self, args: &[NativeValue]) -> AbiResult<NativeTask<'a>> {
        let task_id = self.ctx.spawn_function(self.func_id, args)?;
        Ok(NativeTask {
            task_id,
            ctx: self.ctx,
        })
    }

    /// Get the underlying NativeValue
    pub fn into_value(self) -> NativeValue {
        self.value
    }
}

// ============================================================================
// NativeMethod
// ============================================================================

/// Descriptor for a method resolved from a class vtable.
#[derive(Debug, Clone)]
pub struct NativeMethod {
    /// Class this method belongs to
    pub class_id: usize,
    /// Method name
    pub method_name: String,
    /// Vtable slot index
    pub vtable_index: usize,
    /// Underlying function ID
    pub function_id: usize,
}

impl NativeMethod {
    /// Resolve a method from class metadata
    pub fn resolve(
        ctx: &dyn NativeContext,
        class_id: usize,
        method_name: &str,
    ) -> AbiResult<Self> {
        let methods = ctx.class_method_entries(class_id)?;
        let (_, vtable_index) = methods
            .iter()
            .find(|(name, _)| name == method_name)
            .ok_or_else(|| NativeError::AbiError(format!(
                "Method '{}' not found in class {}",
                method_name, class_id
            )))?;

        // For now, function_id = vtable_index (resolved at call time by engine)
        Ok(Self {
            class_id,
            method_name: method_name.to_string(),
            vtable_index: *vtable_index,
            function_id: *vtable_index,
        })
    }

    /// Call this method on an object (synchronous)
    pub fn call(
        &self,
        ctx: &dyn NativeContext,
        receiver: NativeValue,
        args: &[NativeValue],
    ) -> AbiResult<NativeValue> {
        ctx.call_method(receiver, self.class_id, &self.method_name, args)
    }
}

// ============================================================================
// NativeTask
// ============================================================================

/// Handle for an async task with await/cancel capabilities.
pub struct NativeTask<'a> {
    task_id: u64,
    ctx: &'a dyn NativeContext,
}

impl<'a> NativeTask<'a> {
    /// Create from task ID and context
    pub fn new(ctx: &'a dyn NativeContext, task_id: u64) -> Self {
        Self { task_id, ctx }
    }

    /// Get task ID
    pub fn id(&self) -> u64 {
        self.task_id
    }

    /// Check if task is done (non-blocking)
    pub fn is_done(&self) -> bool {
        self.ctx.task_is_done(self.task_id)
    }

    /// Block until task completes and return its result
    pub fn await_result(&self) -> AbiResult<NativeValue> {
        self.ctx.await_task(self.task_id)
    }

    /// Cancel the task
    pub fn cancel(&self) {
        self.ctx.task_cancel(self.task_id)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_schema_builder() {
        let schema = ObjectSchema::builder(0, "Point")
            .field("x", 0)
            .field("y", 1)
            .method("toString", 0)
            .build();

        assert_eq!(schema.class_id(), 0);
        assert_eq!(schema.class_name(), "Point");
        assert_eq!(schema.field_count(), 2);
        assert_eq!(schema.field_index("x"), Some(0));
        assert_eq!(schema.field_index("y"), Some(1));
        assert_eq!(schema.field_index("z"), None);
        assert_eq!(schema.method_index("toString"), Some(0));
        assert_eq!(schema.method_index("missing"), None);
    }

    #[test]
    fn test_object_schema_field_names() {
        let schema = ObjectSchema::builder(1, "Vec2")
            .field("x", 0)
            .field("y", 1)
            .build();

        assert_eq!(schema.field_names(), &["x", "y"]);
    }

    #[test]
    fn test_native_class_accessors() {
        let class = NativeClass {
            info: ClassInfo {
                class_id: 5,
                field_count: 3,
                name: "MyClass".to_string(),
                parent_id: Some(1),
                constructor_id: Some(10),
                method_count: 2,
            },
        };

        assert_eq!(class.id(), 5);
        assert_eq!(class.name(), "MyClass");
        assert_eq!(class.field_count(), 3);
        assert_eq!(class.parent_id(), Some(1));
        assert_eq!(class.constructor_id(), Some(10));
        assert_eq!(class.method_count(), 2);
    }
}

//! VM Execution Context
//!
//! Each VmContext represents an isolated execution environment with:
//! - Its own heap and garbage collector
//! - Resource limits and accounting
//! - Global variables
//! - Independent GC policy

use crate::gc::{GarbageCollector, GcStats, HeapStats};
use crate::types::TypeRegistry;
use crate::value::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Unique identifier for a VmContext
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VmContextId(u64);

impl VmContextId {
    /// Create a new unique context ID
    pub fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        VmContextId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Get the raw ID value
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl Default for VmContextId {
    fn default() -> Self {
        Self::new()
    }
}

/// Resource limits for a VmContext
#[derive(Debug, Clone)]
pub struct ResourceLimits {
    /// Maximum heap size in bytes (None = unlimited)
    pub max_heap_bytes: Option<usize>,

    /// Maximum number of concurrent tasks (None = unlimited)
    pub max_tasks: Option<usize>,

    /// Maximum CPU step budget (None = unlimited)
    pub max_step_budget: Option<u64>,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_heap_bytes: None,
            max_tasks: None,
            max_step_budget: None,
        }
    }
}

impl ResourceLimits {
    /// Create unlimited resource limits
    pub fn unlimited() -> Self {
        Self::default()
    }

    /// Create resource limits with specific heap size
    pub fn with_heap_limit(max_heap_bytes: usize) -> Self {
        Self {
            max_heap_bytes: Some(max_heap_bytes),
            ..Default::default()
        }
    }

    /// Create resource limits with task limit
    pub fn with_task_limit(max_tasks: usize) -> Self {
        Self {
            max_tasks: Some(max_tasks),
            ..Default::default()
        }
    }

    /// Create resource limits with CPU step budget
    pub fn with_step_budget(max_step_budget: u64) -> Self {
        Self {
            max_step_budget: Some(max_step_budget),
            ..Default::default()
        }
    }
}

/// Resource usage counters for a VmContext
#[derive(Debug)]
pub struct ResourceCounters {
    /// Current number of active tasks
    active_tasks: AtomicUsize,

    /// Total CPU steps executed
    total_steps: AtomicU64,

    /// Peak number of tasks
    peak_tasks: AtomicUsize,
}

impl Default for ResourceCounters {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceCounters {
    /// Create new resource counters
    pub fn new() -> Self {
        Self {
            active_tasks: AtomicUsize::new(0),
            total_steps: AtomicU64::new(0),
            peak_tasks: AtomicUsize::new(0),
        }
    }

    /// Increment active task count
    pub fn increment_tasks(&self) -> usize {
        let count = self.active_tasks.fetch_add(1, Ordering::Relaxed) + 1;

        // Update peak
        let mut peak = self.peak_tasks.load(Ordering::Relaxed);
        while count > peak {
            match self.peak_tasks.compare_exchange_weak(
                peak,
                count,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(p) => peak = p,
            }
        }

        count
    }

    /// Decrement active task count
    pub fn decrement_tasks(&self) -> usize {
        self.active_tasks.fetch_sub(1, Ordering::Relaxed) - 1
    }

    /// Get current active task count
    pub fn active_tasks(&self) -> usize {
        self.active_tasks.load(Ordering::Relaxed)
    }

    /// Get peak task count
    pub fn peak_tasks(&self) -> usize {
        self.peak_tasks.load(Ordering::Relaxed)
    }

    /// Increment step counter
    pub fn increment_steps(&self, count: u64) {
        self.total_steps.fetch_add(count, Ordering::Relaxed);
    }

    /// Get total steps executed
    pub fn total_steps(&self) -> u64 {
        self.total_steps.load(Ordering::Relaxed)
    }

    /// Reset counters
    pub fn reset(&self) {
        self.active_tasks.store(0, Ordering::Relaxed);
        self.total_steps.store(0, Ordering::Relaxed);
        self.peak_tasks.store(0, Ordering::Relaxed);
    }
}

/// Options for creating a VmContext
#[derive(Debug, Clone)]
pub struct VmOptions {
    /// Resource limits
    pub limits: ResourceLimits,

    /// Initial GC threshold in bytes
    pub gc_threshold: usize,

    /// Type registry (shared across contexts)
    pub type_registry: Arc<TypeRegistry>,
}

impl Default for VmOptions {
    fn default() -> Self {
        Self {
            limits: ResourceLimits::default(),
            gc_threshold: 1024 * 1024, // 1 MB
            type_registry: Arc::new(crate::types::create_standard_registry()),
        }
    }
}

/// VM Execution Context
///
/// Each context has its own:
/// - Heap and garbage collector
/// - Global variables
/// - Resource limits and accounting
/// - GC policy
pub struct VmContext {
    /// Unique context ID
    id: VmContextId,

    /// Garbage collector (owns the heap)
    gc: GarbageCollector,

    /// Global variables
    globals: HashMap<String, Value>,

    /// Resource limits
    limits: ResourceLimits,

    /// Resource usage counters
    counters: ResourceCounters,

    /// Type registry (shared)
    type_registry: Arc<TypeRegistry>,
}

impl VmContext {
    /// Create a new VM context with default options
    pub fn new() -> Self {
        Self::with_options(VmOptions::default())
    }

    /// Create a new VM context with specific options
    pub fn with_options(options: VmOptions) -> Self {
        let id = VmContextId::new();
        let mut gc = GarbageCollector::new(id, options.type_registry.clone());

        // Set GC threshold
        gc.set_threshold(options.gc_threshold);

        // Set max heap size if specified
        if let Some(max_heap) = options.limits.max_heap_bytes {
            gc.set_max_heap_size(max_heap);
        }

        Self {
            id,
            gc,
            globals: HashMap::new(),
            limits: options.limits,
            counters: ResourceCounters::new(),
            type_registry: options.type_registry,
        }
    }

    /// Get the context ID
    pub fn id(&self) -> VmContextId {
        self.id
    }

    /// Get a reference to the garbage collector
    pub fn gc(&self) -> &GarbageCollector {
        &self.gc
    }

    /// Get a mutable reference to the garbage collector
    pub fn gc_mut(&mut self) -> &mut GarbageCollector {
        &mut self.gc
    }

    /// Get GC statistics
    pub fn gc_stats(&self) -> &GcStats {
        self.gc.stats()
    }

    /// Get heap statistics
    pub fn heap_stats(&self) -> HeapStats {
        self.gc.heap_stats()
    }

    /// Get a global variable
    pub fn get_global(&self, name: &str) -> Option<Value> {
        self.globals.get(name).copied()
    }

    /// Set a global variable
    pub fn set_global(&mut self, name: String, value: Value) {
        self.globals.insert(name, value);
    }

    /// Get resource limits
    pub fn limits(&self) -> &ResourceLimits {
        &self.limits
    }

    /// Get resource counters
    pub fn counters(&self) -> &ResourceCounters {
        &self.counters
    }

    /// Get type registry
    pub fn type_registry(&self) -> &Arc<TypeRegistry> {
        &self.type_registry
    }

    /// Check if task creation is allowed
    pub fn can_create_task(&self) -> bool {
        if let Some(max_tasks) = self.limits.max_tasks {
            self.counters.active_tasks() < max_tasks
        } else {
            true
        }
    }

    /// Check if step budget is exhausted
    pub fn is_step_budget_exhausted(&self) -> bool {
        if let Some(max_steps) = self.limits.max_step_budget {
            self.counters.total_steps() >= max_steps
        } else {
            false
        }
    }

    /// Run garbage collection
    pub fn collect_garbage(&mut self) {
        self.gc.collect();
    }
}

impl Default for VmContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Global registry of all VM contexts
pub struct ContextRegistry {
    contexts: Mutex<HashMap<VmContextId, Arc<Mutex<VmContext>>>>,
}

impl ContextRegistry {
    /// Create a new context registry
    pub fn new() -> Self {
        Self {
            contexts: Mutex::new(HashMap::new()),
        }
    }

    /// Register a new context
    pub fn register(&self, context: VmContext) -> Arc<Mutex<VmContext>> {
        let id = context.id();
        let context = Arc::new(Mutex::new(context));
        self.contexts.lock().unwrap().insert(id, context.clone());
        context
    }

    /// Get a context by ID
    pub fn get(&self, id: VmContextId) -> Option<Arc<Mutex<VmContext>>> {
        self.contexts.lock().unwrap().get(&id).cloned()
    }

    /// Remove a context
    pub fn remove(&self, id: VmContextId) -> Option<Arc<Mutex<VmContext>>> {
        self.contexts.lock().unwrap().remove(&id)
    }

    /// Get the number of registered contexts
    pub fn len(&self) -> usize {
        self.contexts.lock().unwrap().len()
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get all context IDs
    pub fn all_ids(&self) -> Vec<VmContextId> {
        self.contexts.lock().unwrap().keys().copied().collect()
    }
}

impl Default for ContextRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_id_uniqueness() {
        let id1 = VmContextId::new();
        let id2 = VmContextId::new();
        let id3 = VmContextId::new();

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_resource_limits_default() {
        let limits = ResourceLimits::default();

        assert!(limits.max_heap_bytes.is_none());
        assert!(limits.max_tasks.is_none());
        assert!(limits.max_step_budget.is_none());
    }

    #[test]
    fn test_resource_limits_builders() {
        let heap_limit = ResourceLimits::with_heap_limit(1024 * 1024);
        assert_eq!(heap_limit.max_heap_bytes, Some(1024 * 1024));

        let task_limit = ResourceLimits::with_task_limit(10);
        assert_eq!(task_limit.max_tasks, Some(10));

        let step_limit = ResourceLimits::with_step_budget(1000);
        assert_eq!(step_limit.max_step_budget, Some(1000));
    }

    #[test]
    fn test_resource_counters() {
        let counters = ResourceCounters::new();

        assert_eq!(counters.active_tasks(), 0);
        assert_eq!(counters.total_steps(), 0);
        assert_eq!(counters.peak_tasks(), 0);

        // Increment tasks
        assert_eq!(counters.increment_tasks(), 1);
        assert_eq!(counters.increment_tasks(), 2);
        assert_eq!(counters.active_tasks(), 2);
        assert_eq!(counters.peak_tasks(), 2);

        // Decrement tasks
        assert_eq!(counters.decrement_tasks(), 1);
        assert_eq!(counters.active_tasks(), 1);
        assert_eq!(counters.peak_tasks(), 2); // Peak remains

        // Increment steps
        counters.increment_steps(10);
        counters.increment_steps(5);
        assert_eq!(counters.total_steps(), 15);

        // Reset
        counters.reset();
        assert_eq!(counters.active_tasks(), 0);
        assert_eq!(counters.total_steps(), 0);
        assert_eq!(counters.peak_tasks(), 0);
    }

    #[test]
    fn test_vm_context_creation() {
        let ctx = VmContext::new();

        assert_eq!(ctx.counters().active_tasks(), 0);
        assert_eq!(ctx.counters().total_steps(), 0);

        let heap_stats = ctx.heap_stats();
        assert_eq!(heap_stats.allocated_bytes, 0);
        assert_eq!(heap_stats.allocation_count, 0);
    }

    #[test]
    fn test_vm_context_with_options() {
        let options = VmOptions {
            limits: ResourceLimits::with_heap_limit(2 * 1024 * 1024), // 2 MB
            gc_threshold: 512 * 1024,                                 // 512 KB
            type_registry: Arc::new(crate::types::create_standard_registry()),
        };

        let ctx = VmContext::with_options(options);

        assert_eq!(ctx.limits().max_heap_bytes, Some(2 * 1024 * 1024));
        assert_eq!(ctx.heap_stats().threshold, 512 * 1024);
    }

    #[test]
    fn test_vm_context_globals() {
        let mut ctx = VmContext::new();

        assert!(ctx.get_global("test").is_none());

        ctx.set_global("test".to_string(), Value::i32(42));
        assert_eq!(ctx.get_global("test"), Some(Value::i32(42)));

        ctx.set_global("test".to_string(), Value::i32(100));
        assert_eq!(ctx.get_global("test"), Some(Value::i32(100)));
    }

    #[test]
    fn test_vm_context_task_limits() {
        let options = VmOptions {
            limits: ResourceLimits::with_task_limit(2),
            ..Default::default()
        };

        let ctx = VmContext::with_options(options);

        assert!(ctx.can_create_task());

        ctx.counters().increment_tasks();
        assert!(ctx.can_create_task());

        ctx.counters().increment_tasks();
        assert!(!ctx.can_create_task()); // At limit

        ctx.counters().decrement_tasks();
        assert!(ctx.can_create_task());
    }

    #[test]
    fn test_vm_context_step_budget() {
        let options = VmOptions {
            limits: ResourceLimits::with_step_budget(100),
            ..Default::default()
        };

        let ctx = VmContext::with_options(options);

        assert!(!ctx.is_step_budget_exhausted());

        ctx.counters().increment_steps(50);
        assert!(!ctx.is_step_budget_exhausted());

        ctx.counters().increment_steps(49);
        assert!(!ctx.is_step_budget_exhausted());

        ctx.counters().increment_steps(1);
        assert!(ctx.is_step_budget_exhausted());
    }

    #[test]
    fn test_context_registry() {
        let registry = ContextRegistry::new();

        assert_eq!(registry.len(), 0);
        assert!(registry.is_empty());

        let ctx1 = VmContext::new();
        let id1 = ctx1.id();
        let _arc1 = registry.register(ctx1);

        assert_eq!(registry.len(), 1);
        assert!(!registry.is_empty());

        let ctx2 = VmContext::new();
        let id2 = ctx2.id();
        let _arc2 = registry.register(ctx2);

        assert_eq!(registry.len(), 2);

        // Retrieve contexts
        assert!(registry.get(id1).is_some());
        assert!(registry.get(id2).is_some());

        // Check all IDs
        let ids = registry.all_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));

        // Remove a context
        registry.remove(id1);
        assert_eq!(registry.len(), 1);
        assert!(registry.get(id1).is_none());
        assert!(registry.get(id2).is_some());
    }

    #[test]
    fn test_context_registry_multiple() {
        let registry = ContextRegistry::new();

        // Create 10 contexts
        let mut ids = vec![];
        for _ in 0..10 {
            let ctx = VmContext::new();
            let id = ctx.id();
            registry.register(ctx);
            ids.push(id);
        }

        assert_eq!(registry.len(), 10);

        // Verify all contexts are retrievable
        for id in ids {
            assert!(registry.get(id).is_some());
        }
    }
}

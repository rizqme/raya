//! Reflect registries
//!
//! Global registries for bytecode builders, class builders, and dynamic modules.
//! These are used by the reflect method handlers in the interpreter.

use std::sync::LazyLock;
use parking_lot::Mutex;
use crate::vm::reflect::{BytecodeBuilderRegistry, ClassBuilderRegistry, DynamicModuleRegistry};

pub(crate) static BYTECODE_BUILDER_REGISTRY: LazyLock<Mutex<BytecodeBuilderRegistry>> =
    LazyLock::new(|| Mutex::new(BytecodeBuilderRegistry::new()));

pub(crate) static CLASS_BUILDER_REGISTRY: LazyLock<Mutex<ClassBuilderRegistry>> =
    LazyLock::new(|| Mutex::new(ClassBuilderRegistry::new()));

pub(crate) static DYNAMIC_MODULE_REGISTRY: LazyLock<Mutex<DynamicModuleRegistry>> =
    LazyLock::new(|| Mutex::new(DynamicModuleRegistry::new()));

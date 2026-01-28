//! Type metadata system for precise garbage collection
//!
//! This module provides the type registry and pointer maps that enable
//! precise GC by describing the exact layout of pointers in objects.

mod pointer_map;
mod registry;

pub use pointer_map::PointerMap;
pub use registry::{create_standard_registry, DropFn, TypeInfo, TypeRegistry, TypeRegistryBuilder};

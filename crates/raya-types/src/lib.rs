//! Raya Type System
//!
//! Type representation, inference, and checking for Raya.

#![warn(missing_docs)]

pub mod ty;
pub mod context;
pub mod error;
pub mod subtyping;
pub mod assignability;
pub mod generics;
pub mod normalize;

pub use ty::{Type, PrimitiveType, TypeId};
pub use context::TypeContext;
pub use error::TypeError;
pub use subtyping::SubtypingContext;
pub use assignability::{AssignabilityContext, CoercionKind};
pub use generics::GenericContext;

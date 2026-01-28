//! Raya Type System
//!
//! Type representation, inference, and checking for Raya.

pub mod ty;
pub mod context;
pub mod error;
pub mod subtyping;
pub mod assignability;
pub mod generics;
pub mod normalize;
pub mod discriminant;
pub mod bare_union;

pub use ty::{Type, PrimitiveType, TypeId};
pub use context::TypeContext;
pub use error::TypeError;
pub use subtyping::SubtypingContext;
pub use assignability::{AssignabilityContext, CoercionKind};
pub use generics::GenericContext;
pub use discriminant::{Discriminant, DiscriminantInference, DiscriminantError};
pub use bare_union::{BareUnionDetector, BareUnionError, BareUnionInfo, BareUnionTransform};

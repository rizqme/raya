//! Raya Type System
//!
//! Type representation, inference, and checking for Raya.

pub mod assignability;
pub mod bare_union;
pub mod context;
pub mod discriminant;
pub mod error;
pub mod generics;
pub mod normalize;
pub mod signature;
pub mod subtyping;
pub mod ty;

pub use assignability::{AssignabilityContext, CoercionKind};
pub use bare_union::{BareUnionDetector, BareUnionError, BareUnionInfo, BareUnionTransform};
pub use context::TypeContext;
pub use discriminant::{Discriminant, DiscriminantError, DiscriminantInference};
pub use error::TypeError;
pub use generics::GenericContext;
pub use signature::{
    canonical_type_signature, signature_hash, type_signature_hash, type_signature_string,
    hydrate_type_from_canonical_signature, structural_signature_is_assignable,
    try_hydrate_type_from_canonical_signature, CanonicalTypeSignature,
};
pub use subtyping::SubtypingContext;
pub use ty::{PrimitiveType, Type, TypeId};

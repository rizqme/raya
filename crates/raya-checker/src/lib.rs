//! Raya Type Checker
//!
//! Type checking and control flow analysis for Raya.
//!
//! This crate provides:
//! - Symbol tables with scope management
//! - Name binding (AST → Symbol Table)
//! - Type checking for expressions and statements
//! - Control flow-based type narrowing
//! - Exhaustiveness checking for discriminated unions
//!
//! # Usage
//!
//! ```ignore
//! use raya_checker::{Binder, TypeChecker};
//! use raya_types::TypeContext;
//! use raya_parser::Parser;
//!
//! // Parse source code
//! let ast = Parser::parse(source)?;
//!
//! // Create type context
//! let mut type_ctx = TypeContext::new();
//!
//! // Bind names to create symbol table
//! let binder = Binder::new(&mut type_ctx);
//! let symbols = binder.bind_module(&ast)?;
//!
//! // Type check the module
//! let checker = TypeChecker::new(&mut type_ctx, &symbols);
//! checker.check_module(&ast)?;
//! ```

#![warn(missing_docs)]

pub mod symbols;
pub mod binder;
pub mod checker;
pub mod error;

// Re-export main types
pub use symbols::{Symbol, SymbolKind, SymbolTable, SymbolFlags, Scope, ScopeId, ScopeKind};
pub use binder::Binder;
pub use checker::TypeChecker;
pub use error::{BindError, CheckError};

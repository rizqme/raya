//! Raya parser crate - Lexer and parser for the Raya programming language.
//!
//! This crate provides lexical analysis (tokenization) and syntactic analysis
//! (parsing) for Raya source code.
//!
//! # Example
//!
//! ```ignore
//! use raya_engine::parser::Lexer;
//!
//! let source = r#"
//!     function add(a: number, b: number): number {
//!         return a + b;
//!     }
//! "#;
//!
//! let lexer = Lexer::new(source);
//! match lexer.tokenize() {
//!     Ok((tokens, _interner)) => {
//!         for (token, span) in tokens {
//!             println!("{:?} at {}:{}", token, span.line, span.column);
//!         }
//!     }
//!     Err(errors) => {
//!         for err in errors {
//!             eprintln!("{}", err);
//!         }
//!     }
//! }
//! ```

pub mod token;
pub mod lexer;
pub mod ast;
pub mod parser;
pub mod interner;

// Type system modules (merged from raya-types)
pub mod types;

// Type checker modules (merged from raya-checker)
pub mod checker;

// Re-exports for convenience
pub use token::{Token, Span, TemplatePart};
pub use lexer::{Lexer, LexError};
pub use parser::{Parser, ParseError};
pub use interner::{Interner, Symbol};

// Type system re-exports
pub use types::{Type, TypeId, TypeContext};

// Checker re-exports
pub use checker::{TypeChecker, CheckError, SymbolTable};

//! Raya parser crate - Lexer and parser for the Raya programming language.
//!
//! This crate provides lexical analysis (tokenization) and syntactic analysis
//! (parsing) for Raya source code.
//!
//! # Example
//!
//! ```rust
//! use raya_parser::Lexer;
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

// Re-exports for convenience
pub use token::{Token, Span, TemplatePart};
pub use lexer::{Lexer, LexError};
pub use parser::{Parser, ParseError};
pub use interner::{Interner, Symbol};

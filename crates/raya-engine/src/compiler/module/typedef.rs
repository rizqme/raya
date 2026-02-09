//! Type definition file (.d.raya) parsing
//!
//! Parses TypeScript-like declaration files that contain type information
//! without implementation details. Used for:
//! - Providing types for pre-compiled bytecode packages
//! - IDE autocomplete and type checking
//! - Documenting public APIs

use std::path::Path;

use crate::parser::ast::*;
use crate::parser::{Interner, ParseError, Parser, Span};

/// A parsed type definition file
#[derive(Debug, Clone)]
pub struct TypeDefFile {
    /// Path to the .d.raya file
    pub path: std::path::PathBuf,
    /// Exported declarations
    pub exports: Vec<TypeDefExport>,
}

/// An exported declaration from a type definition file
#[derive(Debug, Clone)]
pub enum TypeDefExport {
    /// Exported function signature
    Function(FunctionSignature),
    /// Exported class definition
    Class(ClassSignature),
    /// Exported type alias
    TypeAlias(TypeAliasSignature),
    /// Exported variable/constant type
    Variable(VariableSignature),
}

/// Function signature (no body)
#[derive(Debug, Clone)]
pub struct FunctionSignature {
    pub name: String,
    pub type_params: Option<Vec<TypeParameter>>,
    pub params: Vec<Parameter>,
    pub return_type: Option<TypeAnnotation>,
    pub is_async: bool,
    pub span: Span,
}

/// Class signature with member signatures
#[derive(Debug, Clone)]
pub struct ClassSignature {
    pub name: String,
    pub type_params: Option<Vec<TypeParameter>>,
    pub extends: Option<TypeAnnotation>,
    pub implements: Vec<TypeAnnotation>,
    pub members: Vec<ClassMemberSignature>,
    pub is_abstract: bool,
    pub span: Span,
}

/// Class member signature
#[derive(Debug, Clone)]
pub enum ClassMemberSignature {
    /// Field declaration
    Field {
        name: String,
        type_annotation: Option<TypeAnnotation>,
        visibility: Visibility,
        is_static: bool,
        span: Span,
    },
    /// Method signature (no body)
    Method {
        name: String,
        type_params: Option<Vec<TypeParameter>>,
        params: Vec<Parameter>,
        return_type: Option<TypeAnnotation>,
        visibility: Visibility,
        is_static: bool,
        is_async: bool,
        is_abstract: bool,
        span: Span,
    },
    /// Constructor signature
    Constructor {
        params: Vec<Parameter>,
        span: Span,
    },
}

/// Type alias signature
#[derive(Debug, Clone)]
pub struct TypeAliasSignature {
    pub name: String,
    pub type_params: Option<Vec<TypeParameter>>,
    pub type_annotation: TypeAnnotation,
    pub span: Span,
}

/// Variable/constant type signature
#[derive(Debug, Clone)]
pub struct VariableSignature {
    pub name: String,
    pub type_annotation: TypeAnnotation,
    pub is_const: bool,
    pub span: Span,
}

/// Error during type definition parsing
#[derive(Debug, Clone)]
pub enum TypeDefError {
    /// IO error reading file
    IoError(String),
    /// Lexer error
    LexError(String),
    /// Parse error
    ParseError(ParseError),
    /// Invalid declaration (has body when it shouldn't)
    InvalidDeclaration { message: String, span: Span },
    /// Missing type annotation
    MissingTypeAnnotation { message: String, span: Span },
}

impl std::fmt::Display for TypeDefError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeDefError::IoError(msg) => write!(f, "IO error: {}", msg),
            TypeDefError::LexError(msg) => write!(f, "Lexer error: {}", msg),
            TypeDefError::ParseError(e) => write!(f, "Parse error: {}", e),
            TypeDefError::InvalidDeclaration { message, .. } => {
                write!(f, "Invalid declaration: {}", message)
            }
            TypeDefError::MissingTypeAnnotation { message, .. } => {
                write!(f, "Missing type annotation: {}", message)
            }
        }
    }
}

impl std::error::Error for TypeDefError {}

/// Parser for .d.raya type definition files
pub struct TypeDefParser {
    interner: Interner,
}

impl TypeDefParser {
    /// Create a new type definition parser
    pub fn new() -> Self {
        Self {
            interner: Interner::new(),
        }
    }

    /// Parse a type definition file from path
    pub fn parse_file(&mut self, path: &Path) -> Result<TypeDefFile, TypeDefError> {
        let source = std::fs::read_to_string(path)
            .map_err(|e| TypeDefError::IoError(e.to_string()))?;

        self.parse_source(&source, path)
    }

    /// Parse type definitions from source string
    pub fn parse_source(&mut self, source: &str, path: &Path) -> Result<TypeDefFile, TypeDefError> {
        // Create parser from source
        let parser = Parser::new(source).map_err(|errors| {
            TypeDefError::LexError(
                errors
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        })?;

        // Parse the module
        let (module, interner) = parser.parse().map_err(|errors| {
            TypeDefError::ParseError(errors.into_iter().next().unwrap())
        })?;

        self.interner = interner;

        // Extract type definitions from the parsed module
        let exports = self.extract_exports(&module)?;

        Ok(TypeDefFile {
            path: path.to_path_buf(),
            exports,
        })
    }

    /// Extract exported declarations from a parsed program
    fn extract_exports(&self, program: &Module) -> Result<Vec<TypeDefExport>, TypeDefError> {
        let mut exports = Vec::new();

        for stmt in &program.statements {
            match stmt {
                Statement::ExportDecl(export) => {
                    match export {
                        ExportDecl::Declaration(inner) => {
                            if let Some(export) = self.convert_declaration(inner)? {
                                exports.push(export);
                            }
                        }
                        ExportDecl::Named { specifiers, .. } => {
                            // Named exports reference other declarations
                            // For now, we skip these - they should reference declared items
                            let _ = specifiers;
                        }
                        ExportDecl::All { .. } => {
                            // Re-exports from other modules
                        }
                        ExportDecl::Default { .. } => {
                            // Default exports — type information comes from the expression
                            // For .d.raya files, default exports are handled via named "default" symbol
                        }
                    }
                }
                // Non-exported declarations are ignored in .d.raya files
                _ => {}
            }
        }

        Ok(exports)
    }

    /// Convert a declaration statement to a type definition export
    fn convert_declaration(
        &self,
        stmt: &Statement,
    ) -> Result<Option<TypeDefExport>, TypeDefError> {
        match stmt {
            Statement::FunctionDecl(func) => {
                // In .d.raya files, function bodies should be empty or contain only a semicolon
                // For now, we allow bodies but extract only the signature
                let sig = FunctionSignature {
                    name: self.interner.resolve(func.name.name).to_string(),
                    type_params: func.type_params.clone(),
                    params: func.params.clone(),
                    return_type: func.return_type.clone(),
                    is_async: func.is_async,
                    span: func.span.clone(),
                };
                Ok(Some(TypeDefExport::Function(sig)))
            }

            Statement::ClassDecl(class) => {
                let members = self.convert_class_members(&class.members)?;
                let sig = ClassSignature {
                    name: self.interner.resolve(class.name.name).to_string(),
                    type_params: class.type_params.clone(),
                    extends: class.extends.clone(),
                    implements: class.implements.clone(),
                    members,
                    is_abstract: class.is_abstract,
                    span: class.span.clone(),
                };
                Ok(Some(TypeDefExport::Class(sig)))
            }

            Statement::TypeAliasDecl(alias) => {
                let sig = TypeAliasSignature {
                    name: self.interner.resolve(alias.name.name).to_string(),
                    type_params: alias.type_params.clone(),
                    type_annotation: alias.type_annotation.clone(),
                    span: alias.span.clone(),
                };
                Ok(Some(TypeDefExport::TypeAlias(sig)))
            }

            Statement::VariableDecl(var) => {
                // Variable declarations in .d.raya must have type annotations
                let type_annotation = var.type_annotation.clone().ok_or_else(|| {
                    TypeDefError::MissingTypeAnnotation {
                        message: "Variable declarations in .d.raya files must have type annotations"
                            .to_string(),
                        span: var.span.clone(),
                    }
                })?;

                // Get the name from the pattern
                let name = match &var.pattern {
                    Pattern::Identifier(id) => self.interner.resolve(id.name).to_string(),
                    _ => {
                        return Err(TypeDefError::InvalidDeclaration {
                            message: "Destructuring patterns not supported in .d.raya files"
                                .to_string(),
                            span: var.span.clone(),
                        })
                    }
                };

                let sig = VariableSignature {
                    name,
                    type_annotation,
                    is_const: var.kind == VariableKind::Const,
                    span: var.span.clone(),
                };
                Ok(Some(TypeDefExport::Variable(sig)))
            }

            _ => Ok(None),
        }
    }

    /// Convert class members to signatures
    fn convert_class_members(
        &self,
        members: &[ClassMember],
    ) -> Result<Vec<ClassMemberSignature>, TypeDefError> {
        let mut sigs = Vec::new();

        for member in members {
            match member {
                ClassMember::Field(field) => {
                    sigs.push(ClassMemberSignature::Field {
                        name: self.interner.resolve(field.name.name).to_string(),
                        type_annotation: field.type_annotation.clone(),
                        visibility: field.visibility,
                        is_static: field.is_static,
                        span: field.span.clone(),
                    });
                }

                ClassMember::Method(method) => {
                    sigs.push(ClassMemberSignature::Method {
                        name: self.interner.resolve(method.name.name).to_string(),
                        type_params: method.type_params.clone(),
                        params: method.params.clone(),
                        return_type: method.return_type.clone(),
                        visibility: method.visibility,
                        is_static: method.is_static,
                        is_async: method.is_async,
                        is_abstract: method.is_abstract,
                        span: method.span.clone(),
                    });
                }

                ClassMember::Constructor(ctor) => {
                    sigs.push(ClassMemberSignature::Constructor {
                        params: ctor.params.clone(),
                        span: ctor.span.clone(),
                    });
                }
            }
        }

        Ok(sigs)
    }
}

impl Default for TypeDefParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Load a type definition file
pub fn load_typedef(path: &Path) -> Result<TypeDefFile, TypeDefError> {
    let mut parser = TypeDefParser::new();
    parser.parse_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn parse_source(source: &str) -> Result<TypeDefFile, TypeDefError> {
        let mut parser = TypeDefParser::new();
        parser.parse_source(source, &PathBuf::from("test.d.raya"))
    }

    #[test]
    fn test_parse_function_signature() {
        let source = r#"
            export function add(a: number, b: number): number { return 0; }
        "#;

        let typedef = parse_source(source).unwrap();
        assert_eq!(typedef.exports.len(), 1);

        match &typedef.exports[0] {
            TypeDefExport::Function(sig) => {
                assert_eq!(sig.name, "add");
                assert_eq!(sig.params.len(), 2);
                assert!(sig.return_type.is_some());
            }
            _ => panic!("Expected function export"),
        }
    }

    #[test]
    fn test_parse_async_function() {
        let source = r#"
            export async function fetch(url: string): Task<string> { return ""; }
        "#;

        let typedef = parse_source(source).unwrap();
        assert_eq!(typedef.exports.len(), 1);

        match &typedef.exports[0] {
            TypeDefExport::Function(sig) => {
                assert_eq!(sig.name, "fetch");
                assert!(sig.is_async);
            }
            _ => panic!("Expected function export"),
        }
    }

    #[test]
    fn test_parse_generic_function() {
        let source = r#"
            export function identity<T>(value: T): T { return value; }
        "#;

        let typedef = parse_source(source).unwrap();
        assert_eq!(typedef.exports.len(), 1);

        match &typedef.exports[0] {
            TypeDefExport::Function(sig) => {
                assert_eq!(sig.name, "identity");
                assert!(sig.type_params.is_some());
                assert_eq!(sig.type_params.as_ref().unwrap().len(), 1);
            }
            _ => panic!("Expected function export"),
        }
    }

    #[test]
    fn test_parse_class_signature() {
        let source = r#"
            export class Logger {
                constructor(name: string) {}
                info(message: string): void {}
                error(message: string): void {}
            }
        "#;

        let typedef = parse_source(source).unwrap();
        assert_eq!(typedef.exports.len(), 1);

        match &typedef.exports[0] {
            TypeDefExport::Class(sig) => {
                assert_eq!(sig.name, "Logger");
                assert_eq!(sig.members.len(), 3);

                // Check constructor
                match &sig.members[0] {
                    ClassMemberSignature::Constructor { params, .. } => {
                        assert_eq!(params.len(), 1);
                    }
                    _ => panic!("Expected constructor"),
                }

                // Check methods
                match &sig.members[1] {
                    ClassMemberSignature::Method { name, .. } => {
                        assert_eq!(name, "info");
                    }
                    _ => panic!("Expected method"),
                }
            }
            _ => panic!("Expected class export"),
        }
    }

    #[test]
    fn test_parse_abstract_class() {
        let source = r#"
            export abstract class Shape {
                abstract area(): number;
            }
        "#;

        let typedef = parse_source(source).unwrap();
        assert_eq!(typedef.exports.len(), 1);

        match &typedef.exports[0] {
            TypeDefExport::Class(sig) => {
                assert_eq!(sig.name, "Shape");
                assert!(sig.is_abstract);

                match &sig.members[0] {
                    ClassMemberSignature::Method { is_abstract, .. } => {
                        assert!(*is_abstract);
                    }
                    _ => panic!("Expected abstract method"),
                }
            }
            _ => panic!("Expected class export"),
        }
    }

    #[test]
    fn test_parse_class_with_extends() {
        let source = r#"
            export class Circle extends Shape {
                constructor(radius: number) { }
                area(): number { return 0; }
            }
        "#;

        let typedef = parse_source(source).unwrap();

        match &typedef.exports[0] {
            TypeDefExport::Class(sig) => {
                assert_eq!(sig.name, "Circle");
                assert!(sig.extends.is_some());
            }
            _ => panic!("Expected class export"),
        }
    }

    #[test]
    fn test_parse_type_alias() {
        let source = r#"
            export type Point = { x: number; y: number; };
        "#;

        let typedef = parse_source(source).unwrap();
        assert_eq!(typedef.exports.len(), 1);

        match &typedef.exports[0] {
            TypeDefExport::TypeAlias(sig) => {
                assert_eq!(sig.name, "Point");
            }
            _ => panic!("Expected type alias export"),
        }
    }

    #[test]
    fn test_parse_generic_type_alias() {
        let source = r#"
            export type Result<T, E> = { status: "ok"; value: T; } | { status: "error"; error: E; };
        "#;

        let typedef = parse_source(source).unwrap();

        match &typedef.exports[0] {
            TypeDefExport::TypeAlias(sig) => {
                assert_eq!(sig.name, "Result");
                assert!(sig.type_params.is_some());
                assert_eq!(sig.type_params.as_ref().unwrap().len(), 2);
            }
            _ => panic!("Expected type alias export"),
        }
    }

    #[test]
    fn test_parse_variable_signature() {
        let source = r#"
            export const VERSION: string = "";
        "#;

        let typedef = parse_source(source).unwrap();
        assert_eq!(typedef.exports.len(), 1);

        match &typedef.exports[0] {
            TypeDefExport::Variable(sig) => {
                assert_eq!(sig.name, "VERSION");
                assert!(sig.is_const);
            }
            _ => panic!("Expected variable export"),
        }
    }

    #[test]
    fn test_variable_requires_type_annotation() {
        let source = r#"
            export let value = 42;
        "#;

        let result = parse_source(source);
        assert!(matches!(result, Err(TypeDefError::MissingTypeAnnotation { .. })));
    }

    #[test]
    fn test_parse_multiple_exports() {
        let source = r#"
            export function createLogger(name: string): Logger { return null as any; }
            export class Logger {
                constructor(name: string) {}
                log(message: string): void {}
            }
            export type LogLevel = "debug" | "info" | "warn" | "error";
            export const DEFAULT_LEVEL: LogLevel = "info";
        "#;

        let typedef = parse_source(source).unwrap();
        assert_eq!(typedef.exports.len(), 4);

        assert!(matches!(typedef.exports[0], TypeDefExport::Function(_)));
        assert!(matches!(typedef.exports[1], TypeDefExport::Class(_)));
        assert!(matches!(typedef.exports[2], TypeDefExport::TypeAlias(_)));
        assert!(matches!(typedef.exports[3], TypeDefExport::Variable(_)));
    }

    #[test]
    fn test_parse_class_with_fields() {
        let source = r#"
            export class Config {
                public host: string;
                private port: number;
                static DEFAULT_PORT: number;
            }
        "#;

        let typedef = parse_source(source).unwrap();

        match &typedef.exports[0] {
            TypeDefExport::Class(sig) => {
                assert_eq!(sig.members.len(), 3);

                match &sig.members[0] {
                    ClassMemberSignature::Field { name, visibility, .. } => {
                        assert_eq!(name, "host");
                        assert_eq!(*visibility, Visibility::Public);
                    }
                    _ => panic!("Expected field"),
                }

                match &sig.members[1] {
                    ClassMemberSignature::Field { name, visibility, .. } => {
                        assert_eq!(name, "port");
                        assert_eq!(*visibility, Visibility::Private);
                    }
                    _ => panic!("Expected field"),
                }

                match &sig.members[2] {
                    ClassMemberSignature::Field { name, is_static, .. } => {
                        assert_eq!(name, "DEFAULT_PORT");
                        assert!(*is_static);
                    }
                    _ => panic!("Expected static field"),
                }
            }
            _ => panic!("Expected class export"),
        }
    }

    #[test]
    fn test_non_exported_declarations_ignored() {
        let source = r#"
            function helper(): void {}
            export function main(): void {}
        "#;

        let typedef = parse_source(source).unwrap();
        // Only the exported function should be captured
        assert_eq!(typedef.exports.len(), 1);

        match &typedef.exports[0] {
            TypeDefExport::Function(sig) => {
                assert_eq!(sig.name, "main");
            }
            _ => panic!("Expected function export"),
        }
    }
}

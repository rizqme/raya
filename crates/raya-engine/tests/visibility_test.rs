//! Tests for visibility modifiers (private/protected/public)

use raya_engine::parser::ast::{ClassMember, Statement, Visibility};
use raya_engine::parser::Parser;

// ============================================================================
// Field Visibility Tests
// ============================================================================

#[test]
fn test_parse_private_field() {
    let source = "class Foo { private x: number; }";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(interner.resolve(decl.name.name), "Foo");
            assert_eq!(decl.members.len(), 1);
            match &decl.members[0] {
                ClassMember::Field(field) => {
                    assert_eq!(field.visibility, Visibility::Private);
                    assert_eq!(interner.resolve(field.name.name), "x");
                }
                _ => panic!("Expected field member"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

#[test]
fn test_parse_protected_field() {
    let source = "class Foo { protected name: string; }";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(decl.members.len(), 1);
            match &decl.members[0] {
                ClassMember::Field(field) => {
                    assert_eq!(field.visibility, Visibility::Protected);
                    assert_eq!(interner.resolve(field.name.name), "name");
                }
                _ => panic!("Expected field member"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

#[test]
fn test_parse_public_field_explicit() {
    let source = "class Foo { public value: number; }";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(decl.members.len(), 1);
            match &decl.members[0] {
                ClassMember::Field(field) => {
                    assert_eq!(field.visibility, Visibility::Public);
                    assert_eq!(interner.resolve(field.name.name), "value");
                }
                _ => panic!("Expected field member"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

#[test]
fn test_parse_default_visibility_is_public() {
    let source = "class Foo { count: number; }";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(decl.members.len(), 1);
            match &decl.members[0] {
                ClassMember::Field(field) => {
                    // Default visibility should be Public
                    assert_eq!(field.visibility, Visibility::Public);
                }
                _ => panic!("Expected field member"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

// ============================================================================
// Method Visibility Tests
// ============================================================================

#[test]
fn test_parse_private_method() {
    let source = "class Foo { private calculate(): number { return 42; } }";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(decl.members.len(), 1);
            match &decl.members[0] {
                ClassMember::Method(method) => {
                    assert_eq!(method.visibility, Visibility::Private);
                    assert_eq!(interner.resolve(method.name.name), "calculate");
                }
                _ => panic!("Expected method member"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

#[test]
fn test_parse_protected_method() {
    let source = "class Foo { protected process(): void {} }";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(decl.members.len(), 1);
            match &decl.members[0] {
                ClassMember::Method(method) => {
                    assert_eq!(method.visibility, Visibility::Protected);
                    assert_eq!(interner.resolve(method.name.name), "process");
                }
                _ => panic!("Expected method member"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

#[test]
fn test_parse_public_method_explicit() {
    let source = "class Foo { public greet(): string { return \"hello\"; } }";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(decl.members.len(), 1);
            match &decl.members[0] {
                ClassMember::Method(method) => {
                    assert_eq!(method.visibility, Visibility::Public);
                    assert_eq!(interner.resolve(method.name.name), "greet");
                }
                _ => panic!("Expected method member"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

// ============================================================================
// Combined Modifiers Tests
// ============================================================================

#[test]
fn test_parse_private_static_field() {
    let source = "class Singleton { private static instance: Singleton; }";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(decl.members.len(), 1);
            match &decl.members[0] {
                ClassMember::Field(field) => {
                    assert_eq!(field.visibility, Visibility::Private);
                    assert!(field.is_static);
                    assert_eq!(interner.resolve(field.name.name), "instance");
                }
                _ => panic!("Expected field member"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

#[test]
fn test_parse_protected_static_method() {
    let source = "class Base { protected static create(): Base { return new Base(); } }";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(decl.members.len(), 1);
            match &decl.members[0] {
                ClassMember::Method(method) => {
                    assert_eq!(method.visibility, Visibility::Protected);
                    assert!(method.is_static);
                    assert_eq!(interner.resolve(method.name.name), "create");
                }
                _ => panic!("Expected method member"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

#[test]
fn test_parse_public_static_field() {
    let source = "class Config { public static version: string = \"1.0\"; }";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(decl.members.len(), 1);
            match &decl.members[0] {
                ClassMember::Field(field) => {
                    assert_eq!(field.visibility, Visibility::Public);
                    assert!(field.is_static);
                    assert_eq!(interner.resolve(field.name.name), "version");
                    assert!(field.initializer.is_some());
                }
                _ => panic!("Expected field member"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

#[test]
fn test_parse_protected_abstract_method() {
    let source = "abstract class Shape { protected abstract area(): number; }";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert!(decl.is_abstract);
            assert_eq!(decl.members.len(), 1);
            match &decl.members[0] {
                ClassMember::Method(method) => {
                    assert_eq!(method.visibility, Visibility::Protected);
                    assert!(method.is_abstract);
                    assert!(method.body.is_none());
                    assert_eq!(interner.resolve(method.name.name), "area");
                }
                _ => panic!("Expected method member"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

#[test]
fn test_parse_private_async_method() {
    let source = "class Api { private async fetch(): Task<string> {} }";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(decl.members.len(), 1);
            match &decl.members[0] {
                ClassMember::Method(method) => {
                    assert_eq!(method.visibility, Visibility::Private);
                    assert!(method.is_async);
                    assert_eq!(interner.resolve(method.name.name), "fetch");
                }
                _ => panic!("Expected method member"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

// ============================================================================
// Mixed Members Tests
// ============================================================================

#[test]
fn test_parse_class_with_mixed_visibility() {
    let source = r#"
        class BankAccount {
            private balance: number;
            protected accountNumber: string;
            public owner: string;

            public deposit(amount: number): void {}
            private validateAmount(amount: number): boolean { return true; }
            protected logTransaction(txType: string): void {}
        }
    "#;
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(interner.resolve(decl.name.name), "BankAccount");
            assert_eq!(decl.members.len(), 6);

            // Check field visibilities
            match &decl.members[0] {
                ClassMember::Field(field) => {
                    assert_eq!(field.visibility, Visibility::Private);
                    assert_eq!(interner.resolve(field.name.name), "balance");
                }
                _ => panic!("Expected field"),
            }
            match &decl.members[1] {
                ClassMember::Field(field) => {
                    assert_eq!(field.visibility, Visibility::Protected);
                    assert_eq!(interner.resolve(field.name.name), "accountNumber");
                }
                _ => panic!("Expected field"),
            }
            match &decl.members[2] {
                ClassMember::Field(field) => {
                    assert_eq!(field.visibility, Visibility::Public);
                    assert_eq!(interner.resolve(field.name.name), "owner");
                }
                _ => panic!("Expected field"),
            }

            // Check method visibilities
            match &decl.members[3] {
                ClassMember::Method(method) => {
                    assert_eq!(method.visibility, Visibility::Public);
                    assert_eq!(interner.resolve(method.name.name), "deposit");
                }
                _ => panic!("Expected method"),
            }
            match &decl.members[4] {
                ClassMember::Method(method) => {
                    assert_eq!(method.visibility, Visibility::Private);
                    assert_eq!(interner.resolve(method.name.name), "validateAmount");
                }
                _ => panic!("Expected method"),
            }
            match &decl.members[5] {
                ClassMember::Method(method) => {
                    assert_eq!(method.visibility, Visibility::Protected);
                    assert_eq!(interner.resolve(method.name.name), "logTransaction");
                }
                _ => panic!("Expected method"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

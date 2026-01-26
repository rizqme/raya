//! Tests for decorator parsing

use raya_parser::ast::{ClassMember, Expression, Statement};
use raya_parser::Parser;

// ============================================================================
// Class Decorator Tests
// ============================================================================

#[test]
fn test_parse_simple_class_decorator() {
    let source = "@sealed class Foo {}";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(interner.resolve(decl.name.name), "Foo");
            assert_eq!(decl.decorators.len(), 1);
            match &decl.decorators[0].expression {
                Expression::Identifier(id) => {
                    assert_eq!(interner.resolve(id.name), "sealed");
                }
                _ => panic!("Expected identifier decorator"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

#[test]
fn test_parse_class_decorator_with_args() {
    let source = r#"@logged("debug") class Foo {}"#;
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(interner.resolve(decl.name.name), "Foo");
            assert_eq!(decl.decorators.len(), 1);
            match &decl.decorators[0].expression {
                Expression::Call(call) => {
                    match call.callee.as_ref() {
                        Expression::Identifier(id) => {
                            assert_eq!(interner.resolve(id.name), "logged");
                        }
                        _ => panic!("Expected identifier callee"),
                    }
                    assert_eq!(call.arguments.len(), 1);
                }
                _ => panic!("Expected call expression decorator"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

#[test]
fn test_parse_multiple_class_decorators() {
    let source = "@sealed @logged class Foo {}";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(interner.resolve(decl.name.name), "Foo");
            assert_eq!(decl.decorators.len(), 2);
            match &decl.decorators[0].expression {
                Expression::Identifier(id) => {
                    assert_eq!(interner.resolve(id.name), "sealed");
                }
                _ => panic!("Expected identifier"),
            }
            match &decl.decorators[1].expression {
                Expression::Identifier(id) => {
                    assert_eq!(interner.resolve(id.name), "logged");
                }
                _ => panic!("Expected identifier"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

#[test]
fn test_parse_decorator_with_member_access() {
    let source = "@core.component class Widget {}";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(interner.resolve(decl.name.name), "Widget");
            assert_eq!(decl.decorators.len(), 1);
            match &decl.decorators[0].expression {
                Expression::Member(member) => {
                    match member.object.as_ref() {
                        Expression::Identifier(id) => {
                            assert_eq!(interner.resolve(id.name), "core");
                        }
                        _ => panic!("Expected identifier object"),
                    }
                    assert_eq!(interner.resolve(member.property.name), "component");
                }
                _ => panic!("Expected member expression decorator"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

#[test]
fn test_parse_abstract_class_with_decorator() {
    let source = "@sealed abstract class Shape {}";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(interner.resolve(decl.name.name), "Shape");
            assert!(decl.is_abstract);
            assert_eq!(decl.decorators.len(), 1);
        }
        _ => panic!("Expected class declaration"),
    }
}

// ============================================================================
// Method Decorator Tests
// ============================================================================

#[test]
fn test_parse_method_decorator() {
    let source = r#"
        class Foo {
            @memoized
            calculate(): number { return 42; }
        }
    "#;
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(decl.members.len(), 1);
            match &decl.members[0] {
                ClassMember::Method(method) => {
                    assert_eq!(interner.resolve(method.name.name), "calculate");
                    assert_eq!(method.decorators.len(), 1);
                    match &method.decorators[0].expression {
                        Expression::Identifier(id) => {
                            assert_eq!(interner.resolve(id.name), "memoized");
                        }
                        _ => panic!("Expected identifier"),
                    }
                }
                _ => panic!("Expected method"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

#[test]
fn test_parse_method_decorator_with_args() {
    let source = r#"
        class Foo {
            @debounce(300)
            handleClick(): void {}
        }
    "#;
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(decl.members.len(), 1);
            match &decl.members[0] {
                ClassMember::Method(method) => {
                    assert_eq!(interner.resolve(method.name.name), "handleClick");
                    assert_eq!(method.decorators.len(), 1);
                    match &method.decorators[0].expression {
                        Expression::Call(call) => {
                            assert_eq!(call.arguments.len(), 1);
                        }
                        _ => panic!("Expected call expression"),
                    }
                }
                _ => panic!("Expected method"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

// ============================================================================
// Field Decorator Tests
// ============================================================================

#[test]
fn test_parse_field_decorator() {
    let source = r#"
        class Foo {
            @validate
            name: string;
        }
    "#;
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(decl.members.len(), 1);
            match &decl.members[0] {
                ClassMember::Field(field) => {
                    assert_eq!(interner.resolve(field.name.name), "name");
                    assert_eq!(field.decorators.len(), 1);
                    match &field.decorators[0].expression {
                        Expression::Identifier(id) => {
                            assert_eq!(interner.resolve(id.name), "validate");
                        }
                        _ => panic!("Expected identifier"),
                    }
                }
                _ => panic!("Expected field"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

#[test]
fn test_parse_field_decorator_with_args() {
    let source = r#"
        class Foo {
            @minLength(3)
            username: string;
        }
    "#;
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(decl.members.len(), 1);
            match &decl.members[0] {
                ClassMember::Field(field) => {
                    assert_eq!(interner.resolve(field.name.name), "username");
                    assert_eq!(field.decorators.len(), 1);
                    match &field.decorators[0].expression {
                        Expression::Call(call) => {
                            assert_eq!(call.arguments.len(), 1);
                        }
                        _ => panic!("Expected call expression"),
                    }
                }
                _ => panic!("Expected field"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

// ============================================================================
// Combined Decorators Tests
// ============================================================================

#[test]
fn test_parse_class_with_decorated_members() {
    let source = r#"
        @sealed
        class Service {
            @inject
            private client: HttpClient;

            @cached(60)
            getData(): string { return "data"; }
        }
    "#;
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(interner.resolve(decl.name.name), "Service");
            assert_eq!(decl.decorators.len(), 1); // @sealed

            assert_eq!(decl.members.len(), 2);

            // Check field decorator
            match &decl.members[0] {
                ClassMember::Field(field) => {
                    assert_eq!(interner.resolve(field.name.name), "client");
                    assert_eq!(field.decorators.len(), 1); // @inject
                }
                _ => panic!("Expected field"),
            }

            // Check method decorator
            match &decl.members[1] {
                ClassMember::Method(method) => {
                    assert_eq!(interner.resolve(method.name.name), "getData");
                    assert_eq!(method.decorators.len(), 1); // @cached(60)
                }
                _ => panic!("Expected method"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

#[test]
fn test_parse_multiple_decorators_on_member() {
    let source = r#"
        class Foo {
            @readonly
            @validate
            name: string;
        }
    "#;
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(decl.members.len(), 1);
            match &decl.members[0] {
                ClassMember::Field(field) => {
                    assert_eq!(interner.resolve(field.name.name), "name");
                    assert_eq!(field.decorators.len(), 2);
                }
                _ => panic!("Expected field"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

#[test]
fn test_parse_decorator_with_complex_member_access() {
    let source = "@angular.core.Component class MyComponent {}";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(interner.resolve(decl.name.name), "MyComponent");
            assert_eq!(decl.decorators.len(), 1);
            // angular.core.Component
            match &decl.decorators[0].expression {
                Expression::Member(outer) => {
                    assert_eq!(interner.resolve(outer.property.name), "Component");
                    match outer.object.as_ref() {
                        Expression::Member(inner) => {
                            assert_eq!(interner.resolve(inner.property.name), "core");
                            match inner.object.as_ref() {
                                Expression::Identifier(id) => {
                                    assert_eq!(interner.resolve(id.name), "angular");
                                }
                                _ => panic!("Expected identifier"),
                            }
                        }
                        _ => panic!("Expected member"),
                    }
                }
                _ => panic!("Expected member expression"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

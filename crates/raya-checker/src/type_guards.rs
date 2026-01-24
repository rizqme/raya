//! Type guard detection for control flow-based type narrowing
//!
//! This module provides utilities for detecting type guards in expressions,
//! which are used to narrow types in conditional branches.

use raya_parser::ast::{BinaryOperator, Expression};

/// A type guard extracted from a conditional expression
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeGuard {
    /// `typeof x === "type"` or `typeof x !== "type"`
    TypeOf {
        /// Variable name being tested
        var: String,
        /// Type name ("string", "number", "boolean", "function", "object")
        type_name: String,
        /// Whether this is a negated check (!==)
        negated: bool,
    },

    /// `x.discriminant === "variant"` or `x.discriminant !== "variant"`
    Discriminant {
        /// Variable name (base of member expression)
        var: String,
        /// Discriminant field name
        field: String,
        /// Variant value
        variant: String,
        /// Whether this is a negated check (!==)
        negated: bool,
    },

    /// `x !== null` or `x === null`
    Nullish {
        /// Variable name being tested
        var: String,
        /// Whether this is a negated check (testing for non-null)
        negated: bool,
    },
}

/// Extract a type guard from a conditional expression
///
/// Returns `Some(TypeGuard)` if the expression represents a type guard,
/// or `None` if it's a regular boolean expression.
///
/// # Supported Patterns
///
/// - `typeof x === "string"` → TypeOf guard
/// - `typeof x !== "number"` → Negated TypeOf guard
/// - `x.kind === "ok"` → Discriminant guard
/// - `x.status !== "error"` → Negated Discriminant guard
/// - `x !== null` → Nullish guard (negated)
/// - `x === null` → Nullish guard
pub fn extract_type_guard(expr: &Expression) -> Option<TypeGuard> {
    // Type guards are binary expressions with === or !==
    let bin = match expr {
        Expression::Binary(b) => b,
        _ => return None,
    };

    // Must be equality or inequality
    let (negated, is_equality) = match bin.operator {
        BinaryOperator::StrictEqual => (false, true),
        BinaryOperator::StrictNotEqual => (true, true),
        BinaryOperator::Equal => (false, true),
        BinaryOperator::NotEqual => (true, true),
        _ => return None,
    };

    if !is_equality {
        return None;
    }

    // Try different patterns

    // Pattern 1: typeof x === "type"
    if let Some(guard) = try_extract_typeof_guard(&bin.left, &bin.right, negated) {
        return Some(guard);
    }
    if let Some(guard) = try_extract_typeof_guard(&bin.right, &bin.left, negated) {
        return Some(guard);
    }

    // Pattern 2: x.field === "variant"
    if let Some(guard) = try_extract_discriminant_guard(&bin.left, &bin.right, negated) {
        return Some(guard);
    }
    if let Some(guard) = try_extract_discriminant_guard(&bin.right, &bin.left, negated) {
        return Some(guard);
    }

    // Pattern 3: x === null or x !== null
    if let Some(guard) = try_extract_nullish_guard(&bin.left, &bin.right, negated) {
        return Some(guard);
    }
    if let Some(guard) = try_extract_nullish_guard(&bin.right, &bin.left, negated) {
        return Some(guard);
    }

    None
}

/// Try to extract a typeof guard: `typeof x === "type"`
fn try_extract_typeof_guard(
    left: &Expression,
    right: &Expression,
    negated: bool,
) -> Option<TypeGuard> {
    // Left must be typeof expression
    let typeof_expr = match left {
        Expression::Typeof(t) => t,
        _ => return None,
    };

    // Argument must be an identifier
    let var_name = match &*typeof_expr.argument {
        Expression::Identifier(ident) => ident.name.clone(),
        _ => return None,
    };

    // Right must be a string literal
    let type_name = match right {
        Expression::StringLiteral(s) => s.value.clone(),
        _ => return None,
    };

    Some(TypeGuard::TypeOf {
        var: var_name,
        type_name,
        negated,
    })
}

/// Try to extract a discriminant guard: `x.field === "variant"`
fn try_extract_discriminant_guard(
    left: &Expression,
    right: &Expression,
    negated: bool,
) -> Option<TypeGuard> {
    // Left must be a member expression
    let member = match left {
        Expression::Member(m) => m,
        _ => return None,
    };

    // Object must be an identifier
    let var_name = match &*member.object {
        Expression::Identifier(ident) => ident.name.clone(),
        _ => return None,
    };

    // Property is always an identifier in MemberExpression
    let field_name = member.property.name.clone();

    // Right must be a string literal
    let variant = match right {
        Expression::StringLiteral(s) => s.value.clone(),
        _ => return None,
    };

    Some(TypeGuard::Discriminant {
        var: var_name,
        field: field_name,
        variant,
        negated,
    })
}

/// Try to extract a nullish guard: `x === null` or `x !== null`
fn try_extract_nullish_guard(
    left: &Expression,
    right: &Expression,
    negated: bool,
) -> Option<TypeGuard> {
    // Left must be an identifier
    let var_name = match left {
        Expression::Identifier(ident) => ident.name.clone(),
        _ => return None,
    };

    // Right must be null literal
    match right {
        Expression::NullLiteral(_) => Some(TypeGuard::Nullish {
            var: var_name,
            negated,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use raya_parser::Parser;

    fn parse_expr(source: &str) -> Expression {
        let parser = Parser::new(source).unwrap();
        let module = parser.parse().unwrap();
        // Extract the first expression statement
        match &module.statements[0] {
            raya_parser::ast::Statement::Expression(expr_stmt) => expr_stmt.expression.clone(),
            _ => panic!("Expected expression statement"),
        }
    }

    #[test]
    fn test_extract_typeof_guard() {
        let expr = parse_expr(r#"typeof x === "string""#);
        let guard = extract_type_guard(&expr).unwrap();

        assert_eq!(
            guard,
            TypeGuard::TypeOf {
                var: "x".to_string(),
                type_name: "string".to_string(),
                negated: false,
            }
        );
    }

    #[test]
    fn test_extract_typeof_guard_negated() {
        let expr = parse_expr(r#"typeof x !== "number""#);
        let guard = extract_type_guard(&expr).unwrap();

        assert_eq!(
            guard,
            TypeGuard::TypeOf {
                var: "x".to_string(),
                type_name: "number".to_string(),
                negated: true,
            }
        );
    }

    #[test]
    fn test_extract_discriminant_guard() {
        let expr = parse_expr(r#"x.kind === "ok""#);
        let guard = extract_type_guard(&expr).unwrap();

        assert_eq!(
            guard,
            TypeGuard::Discriminant {
                var: "x".to_string(),
                field: "kind".to_string(),
                variant: "ok".to_string(),
                negated: false,
            }
        );
    }

    #[test]
    fn test_extract_discriminant_guard_negated() {
        let expr = parse_expr(r#"result.status !== "error""#);
        let guard = extract_type_guard(&expr).unwrap();

        assert_eq!(
            guard,
            TypeGuard::Discriminant {
                var: "result".to_string(),
                field: "status".to_string(),
                variant: "error".to_string(),
                negated: true,
            }
        );
    }

    #[test]
    fn test_extract_nullish_guard() {
        let expr = parse_expr("x !== null");
        let guard = extract_type_guard(&expr).unwrap();

        assert_eq!(
            guard,
            TypeGuard::Nullish {
                var: "x".to_string(),
                negated: true,
            }
        );
    }

    #[test]
    fn test_extract_nullish_guard_non_negated() {
        let expr = parse_expr("x === null");
        let guard = extract_type_guard(&expr).unwrap();

        assert_eq!(
            guard,
            TypeGuard::Nullish {
                var: "x".to_string(),
                negated: false,
            }
        );
    }

    #[test]
    fn test_non_guard_expression() {
        let expr = parse_expr("x > 5");
        let guard = extract_type_guard(&expr);
        assert!(guard.is_none());
    }

    #[test]
    fn test_non_guard_equality() {
        let expr = parse_expr("x === y");
        let guard = extract_type_guard(&expr);
        assert!(guard.is_none());
    }
}

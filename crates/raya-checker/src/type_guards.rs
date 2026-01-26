//! Type guard detection for control flow-based type narrowing
//!
//! This module provides utilities for detecting type guards in expressions,
//! which are used to narrow types in conditional branches.

use raya_parser::ast::{BinaryOperator, Expression};
use raya_parser::Interner;

/// A type guard extracted from a conditional expression
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeGuard {
    /// `typeof x === "type"` or `typeof x !== "type"`
    /// Supports: "string", "number", "boolean", "function", "object", "undefined"
    TypeOf {
        /// Variable name being tested
        var: String,
        /// Type name ("string", "number", "boolean", "function", "object", "undefined")
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

    /// `Array.isArray(x)` - narrows to array type
    IsArray {
        /// Variable name being tested
        var: String,
        /// Whether this is a negated check (!Array.isArray(x))
        negated: bool,
    },

    /// `Number.isInteger(x)` - narrows number to integer
    IsInteger {
        /// Variable name being tested
        var: String,
        /// Whether this is a negated check
        negated: bool,
    },

    /// `Number.isNaN(x)` - checks for NaN value
    IsNaN {
        /// Variable name being tested
        var: String,
        /// Whether this is a negated check
        negated: bool,
    },

    /// `Number.isFinite(x)` - checks for finite number (not Infinity or NaN)
    IsFinite {
        /// Variable name being tested
        var: String,
        /// Whether this is a negated check
        negated: bool,
    },

    /// Custom type predicate functions like `isString(x)`, `isObject(x)`, etc.
    /// Pattern: function_name(x) where function is known type guard
    TypePredicate {
        /// Variable name being tested
        var: String,
        /// Predicate function name (e.g., "isString", "isObject")
        predicate: String,
        /// Whether this is a negated check
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
pub fn extract_type_guard(expr: &Expression, interner: &Interner) -> Option<TypeGuard> {
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
    if let Some(guard) = try_extract_typeof_guard(&bin.left, &bin.right, negated, interner) {
        return Some(guard);
    }
    if let Some(guard) = try_extract_typeof_guard(&bin.right, &bin.left, negated, interner) {
        return Some(guard);
    }

    // Pattern 2: x.field === "variant"
    if let Some(guard) = try_extract_discriminant_guard(&bin.left, &bin.right, negated, interner) {
        return Some(guard);
    }
    if let Some(guard) = try_extract_discriminant_guard(&bin.right, &bin.left, negated, interner) {
        return Some(guard);
    }

    // Pattern 3: x === null or x !== null
    if let Some(guard) = try_extract_nullish_guard(&bin.left, &bin.right, negated, interner) {
        return Some(guard);
    }
    if let Some(guard) = try_extract_nullish_guard(&bin.right, &bin.left, negated, interner) {
        return Some(guard);
    }

    None
}

/// Extract type guard from a call expression (e.g., Array.isArray(x), Number.isInteger(x))
///
/// Handles patterns like:
/// - `Array.isArray(x)`
/// - `Number.isInteger(x)`
/// - `Number.isNaN(x)`
/// - `Number.isFinite(x)`
/// - `!Array.isArray(x)` (negated)
pub fn extract_call_type_guard(expr: &Expression, interner: &Interner) -> Option<TypeGuard> {
    // Handle negation: !Array.isArray(x)
    let (call_expr, negated) = match expr {
        Expression::Unary(unary) if matches!(unary.operator, raya_parser::ast::UnaryOperator::Not) => {
            match &*unary.operand {
                Expression::Call(call) => (call, true),
                _ => return None,
            }
        }
        Expression::Call(call) => (call, false),
        _ => return None,
    };

    // Extract function being called
    let (object, method) = match &*call_expr.callee {
        // Pattern: Array.isArray, Number.isInteger, etc.
        Expression::Member(member) => {
            let obj_name = match &*member.object {
                Expression::Identifier(ident) => interner.resolve(ident.name),
                _ => return None,
            };
            (obj_name, interner.resolve(member.property.name))
        }
        // Pattern: isArray, isString (standalone predicates)
        Expression::Identifier(ident) => ("", interner.resolve(ident.name)),
        _ => return None,
    };

    // Must have exactly one argument
    if call_expr.arguments.len() != 1 {
        return None;
    }

    // Extract variable name from argument
    let var_name = match &call_expr.arguments[0] {
        Expression::Identifier(ident) => interner.resolve(ident.name).to_string(),
        _ => return None,
    };

    // Match known type guard patterns
    match (object, method) {
        ("Array", "isArray") => Some(TypeGuard::IsArray {
            var: var_name,
            negated,
        }),
        ("Number", "isInteger") => Some(TypeGuard::IsInteger {
            var: var_name,
            negated,
        }),
        ("Number", "isNaN") => Some(TypeGuard::IsNaN {
            var: var_name,
            negated,
        }),
        ("Number", "isFinite") => Some(TypeGuard::IsFinite {
            var: var_name,
            negated,
        }),
        // Standalone predicates: isString, isObject, isNumber, etc.
        ("", predicate) if predicate.starts_with("is") => Some(TypeGuard::TypePredicate {
            var: var_name,
            predicate: predicate.to_string(),
            negated,
        }),
        _ => None,
    }
}

/// Try to extract a typeof guard: `typeof x === "type"`
fn try_extract_typeof_guard(
    left: &Expression,
    right: &Expression,
    negated: bool,
    interner: &Interner,
) -> Option<TypeGuard> {
    // Left must be typeof expression
    let typeof_expr = match left {
        Expression::Typeof(t) => t,
        _ => return None,
    };

    // Argument must be an identifier
    let var_name = match &*typeof_expr.argument {
        Expression::Identifier(ident) => interner.resolve(ident.name).to_string(),
        _ => return None,
    };

    // Right must be a string literal
    let type_name = match right {
        Expression::StringLiteral(s) => interner.resolve(s.value).to_string(),
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
    interner: &Interner,
) -> Option<TypeGuard> {
    // Left must be a member expression
    let member = match left {
        Expression::Member(m) => m,
        _ => return None,
    };

    // Object must be an identifier
    let var_name = match &*member.object {
        Expression::Identifier(ident) => interner.resolve(ident.name).to_string(),
        _ => return None,
    };

    // Property is always an identifier in MemberExpression
    let field_name = interner.resolve(member.property.name).to_string();

    // Right must be a string literal
    let variant = match right {
        Expression::StringLiteral(s) => interner.resolve(s.value).to_string(),
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
    interner: &Interner,
) -> Option<TypeGuard> {
    // Left must be an identifier
    let var_name = match left {
        Expression::Identifier(ident) => interner.resolve(ident.name).to_string(),
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

    fn parse_expr(source: &str) -> (Expression, Interner) {
        let parser = Parser::new(source).unwrap();
        let (module, interner) = parser.parse().unwrap();
        // Extract the first expression statement
        let expr = match &module.statements[0] {
            raya_parser::ast::Statement::Expression(expr_stmt) => expr_stmt.expression.clone(),
            _ => panic!("Expected expression statement"),
        };
        (expr, interner)
    }

    #[test]
    fn test_extract_typeof_guard() {
        let (expr, interner) = parse_expr(r#"typeof x === "string""#);
        let guard = extract_type_guard(&expr, &interner).unwrap();

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
        let (expr, interner) = parse_expr(r#"typeof x !== "number""#);
        let guard = extract_type_guard(&expr, &interner).unwrap();

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
        let (expr, interner) = parse_expr(r#"x.kind === "ok""#);
        let guard = extract_type_guard(&expr, &interner).unwrap();

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
        let (expr, interner) = parse_expr(r#"result.status !== "error""#);
        let guard = extract_type_guard(&expr, &interner).unwrap();

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
        let (expr, interner) = parse_expr("x !== null");
        let guard = extract_type_guard(&expr, &interner).unwrap();

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
        let (expr, interner) = parse_expr("x === null");
        let guard = extract_type_guard(&expr, &interner).unwrap();

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
        let (expr, interner) = parse_expr("x > 5");
        let guard = extract_type_guard(&expr, &interner);
        assert!(guard.is_none());
    }

    #[test]
    fn test_non_guard_equality() {
        let (expr, interner) = parse_expr("x === y");
        let guard = extract_type_guard(&expr, &interner);
        assert!(guard.is_none());
    }

    #[test]
    fn test_extract_array_is_array_guard() {
        let (expr, interner) = parse_expr("Array.isArray(x)");
        let guard = extract_call_type_guard(&expr, &interner).unwrap();

        assert_eq!(
            guard,
            TypeGuard::IsArray {
                var: "x".to_string(),
                negated: false,
            }
        );
    }

    #[test]
    fn test_extract_array_is_array_negated() {
        let (expr, interner) = parse_expr("!Array.isArray(x)");
        let guard = extract_call_type_guard(&expr, &interner).unwrap();

        assert_eq!(
            guard,
            TypeGuard::IsArray {
                var: "x".to_string(),
                negated: true,
            }
        );
    }

    #[test]
    fn test_extract_number_is_integer() {
        let (expr, interner) = parse_expr("Number.isInteger(x)");
        let guard = extract_call_type_guard(&expr, &interner).unwrap();

        assert_eq!(
            guard,
            TypeGuard::IsInteger {
                var: "x".to_string(),
                negated: false,
            }
        );
    }

    #[test]
    fn test_extract_number_is_nan() {
        let (expr, interner) = parse_expr("Number.isNaN(value)");
        let guard = extract_call_type_guard(&expr, &interner).unwrap();

        assert_eq!(
            guard,
            TypeGuard::IsNaN {
                var: "value".to_string(),
                negated: false,
            }
        );
    }

    #[test]
    fn test_extract_number_is_finite() {
        let (expr, interner) = parse_expr("Number.isFinite(num)");
        let guard = extract_call_type_guard(&expr, &interner).unwrap();

        assert_eq!(
            guard,
            TypeGuard::IsFinite {
                var: "num".to_string(),
                negated: false,
            }
        );
    }

    #[test]
    fn test_extract_custom_type_predicate() {
        let (expr, interner) = parse_expr("isString(x)");
        let guard = extract_call_type_guard(&expr, &interner).unwrap();

        assert_eq!(
            guard,
            TypeGuard::TypePredicate {
                var: "x".to_string(),
                predicate: "isString".to_string(),
                negated: false,
            }
        );
    }

    #[test]
    fn test_non_predicate_call() {
        let (expr, interner) = parse_expr("doSomething(x)");
        let guard = extract_call_type_guard(&expr, &interner);
        assert!(guard.is_none());
    }
}

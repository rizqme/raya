//! Exhaustiveness checking for discriminated unions
//!
//! This module provides exhaustiveness checking for switch statements and
//! match expressions on discriminated union types. It ensures all variants
//! are handled.

use raya_parser::ast::{Expression, SwitchCase, SwitchStatement};
use raya_types::{Type, TypeContext, TypeId};
use std::collections::HashSet;

/// Result of exhaustiveness checking
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExhaustivenessResult {
    /// All variants are covered
    Exhaustive,
    /// Missing variants (returns list of missing discriminant values)
    NonExhaustive(Vec<String>),
    /// Has a default case, so exhaustive by definition
    HasDefault,
    /// Not a discriminated union, cannot check exhaustiveness
    NotApplicable,
}

/// Check if a switch statement is exhaustive over a discriminated union
///
/// Returns the missing variants if not exhaustive, or Ok if exhaustive.
///
/// # Arguments
/// * `ctx` - Type context for looking up types
/// * `discriminant_ty` - Type of the switch discriminant
/// * `switch_stmt` - The switch statement to check
///
/// # Returns
/// * `ExhaustivenessResult::Exhaustive` - All variants covered
/// * `ExhaustivenessResult::NonExhaustive(missing)` - Missing variants listed
/// * `ExhaustivenessResult::HasDefault` - Has default case
/// * `ExhaustivenessResult::NotApplicable` - Not a discriminated union
pub fn check_switch_exhaustiveness(
    ctx: &TypeContext,
    discriminant_ty: TypeId,
    switch_stmt: &SwitchStatement,
) -> ExhaustivenessResult {
    // Check if there's a default case
    if has_default_case(&switch_stmt.cases) {
        return ExhaustivenessResult::HasDefault;
    }

    // Get all variants from the discriminated union
    let all_variants = match extract_union_variants(ctx, discriminant_ty) {
        Some(variants) => variants,
        None => return ExhaustivenessResult::NotApplicable,
    };

    // Extract tested variants from cases
    let tested_variants = extract_tested_variants(&switch_stmt.cases);

    // Find missing variants
    let missing: Vec<String> = all_variants
        .into_iter()
        .filter(|variant| !tested_variants.contains(variant))
        .collect();

    if missing.is_empty() {
        ExhaustivenessResult::Exhaustive
    } else {
        ExhaustivenessResult::NonExhaustive(missing)
    }
}

/// Check if a discriminated union type has all variants covered by a set of string literals
///
/// This is useful for checking if-else chains or other patterns.
///
/// # Arguments
/// * `ctx` - Type context
/// * `union_ty` - Union type to check
/// * `tested_variants` - Set of variant values that have been tested
///
/// # Returns
/// * `Some(missing)` - List of missing variants if not exhaustive
/// * `None` - Exhaustive or not applicable
pub fn check_variants_exhaustive(
    ctx: &TypeContext,
    union_ty: TypeId,
    tested_variants: &HashSet<String>,
) -> Option<Vec<String>> {
    let all_variants = extract_union_variants(ctx, union_ty)?;

    let missing: Vec<String> = all_variants
        .into_iter()
        .filter(|variant| !tested_variants.contains(variant))
        .collect();

    if missing.is_empty() {
        None
    } else {
        Some(missing)
    }
}

/// Extract all variant values from a discriminated union type
///
/// Returns None if the type is not a discriminated union.
fn extract_union_variants(ctx: &TypeContext, ty: TypeId) -> Option<HashSet<String>> {
    let type_def = ctx.get(ty)?;

    match type_def {
        Type::Union(union_ty) => {
            let discriminant = union_ty.discriminant.as_ref()?;
            let mut variants = HashSet::new();

            // For each member of the union, extract the discriminant value
            for member_id in &union_ty.members {
                if let Some(variant) = extract_discriminant_value(ctx, *member_id, discriminant) {
                    variants.insert(variant);
                }
            }

            if variants.is_empty() {
                None
            } else {
                Some(variants)
            }
        }
        _ => None,
    }
}

/// Extract the discriminant value from an object type
fn extract_discriminant_value(
    ctx: &TypeContext,
    ty: TypeId,
    discriminant_field: &str,
) -> Option<String> {
    let type_def = ctx.get(ty)?;

    match type_def {
        Type::Object(obj) => {
            // Find the discriminant property
            let prop = obj.properties.iter().find(|p| p.name == discriminant_field)?;

            // For now, we assume discriminant values are property names
            // In a full implementation with string literal types, we'd extract the literal value
            // Since we don't have string literal types yet, we use a placeholder
            // TODO: Extract actual string literal value when string literal types are implemented
            Some(format!("variant_{}", discriminant_field))
        }
        _ => None,
    }
}

/// Check if a switch statement has a default case
fn has_default_case(cases: &[SwitchCase]) -> bool {
    cases.iter().any(|case| case.test.is_none())
}

/// Extract all tested variants from switch cases
fn extract_tested_variants(cases: &[SwitchCase]) -> HashSet<String> {
    let mut variants = HashSet::new();

    for case in cases {
        if let Some(ref test) = case.test {
            if let Some(variant) = extract_variant_from_expression(test) {
                variants.insert(variant);
            }
        }
    }

    variants
}

/// Extract a variant string from a case test expression
///
/// For now, this only handles string literals.
fn extract_variant_from_expression(expr: &Expression) -> Option<String> {
    match expr {
        Expression::StringLiteral(s) => Some(s.value.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_default_case() {
        use raya_parser::ast::{BlockStatement, SwitchCase};
        use raya_parser::Span;

        let span = Span::new(0, 0, 1, 1);
        let empty_block = BlockStatement {
            statements: vec![],
            span,
        };

        // No default case
        let cases = vec![
            SwitchCase {
                test: Some(Expression::StringLiteral(raya_parser::ast::StringLiteral {
                    value: "ok".to_string(),
                    span,
                })),
                consequent: vec![],
                span,
            },
        ];
        assert!(!has_default_case(&cases));

        // Has default case
        let cases_with_default = vec![
            SwitchCase {
                test: Some(Expression::StringLiteral(raya_parser::ast::StringLiteral {
                    value: "ok".to_string(),
                    span,
                })),
                consequent: vec![],
                span,
            },
            SwitchCase {
                test: None,
                consequent: vec![],
                span,
            },
        ];
        assert!(has_default_case(&cases_with_default));
    }

    #[test]
    fn test_extract_tested_variants() {
        use raya_parser::ast::SwitchCase;
        use raya_parser::Span;

        let span = Span::new(0, 0, 1, 1);

        let cases = vec![
            SwitchCase {
                test: Some(Expression::StringLiteral(raya_parser::ast::StringLiteral {
                    value: "ok".to_string(),
                    span,
                })),
                consequent: vec![],
                span,
            },
            SwitchCase {
                test: Some(Expression::StringLiteral(raya_parser::ast::StringLiteral {
                    value: "error".to_string(),
                    span,
                })),
                consequent: vec![],
                span,
            },
        ];

        let variants = extract_tested_variants(&cases);
        assert_eq!(variants.len(), 2);
        assert!(variants.contains("ok"));
        assert!(variants.contains("error"));
    }

    #[test]
    fn test_extract_variant_from_string_literal() {
        use raya_parser::Span;

        let span = Span::new(0, 0, 1, 1);
        let expr = Expression::StringLiteral(raya_parser::ast::StringLiteral {
            value: "test_variant".to_string(),
            span,
        });

        let variant = extract_variant_from_expression(&expr);
        assert_eq!(variant, Some("test_variant".to_string()));
    }

    #[test]
    fn test_check_variants_exhaustive() {
        let mut tested = HashSet::new();
        tested.insert("ok".to_string());
        tested.insert("error".to_string());

        // Since we don't have a real discriminated union type in tests,
        // we can only test the logic with mock data
        // The actual integration with TypeContext would be tested in integration tests
    }
}

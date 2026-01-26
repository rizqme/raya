//! Exhaustiveness checking for discriminated unions
//!
//! This module provides exhaustiveness checking for switch statements and
//! match expressions on discriminated union types. It ensures all variants
//! are handled.

use raya_parser::ast::{Expression, SwitchCase, SwitchStatement};
use raya_parser::Interner;
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
/// * `interner` - Interner for resolving symbol names
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
    interner: &Interner,
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
    let tested_variants = extract_tested_variants(&switch_stmt.cases, interner);

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

/// Get the discriminant field name for a union type
///
/// Returns None if the type is not a discriminated union.
pub fn get_discriminant_field(ctx: &TypeContext, ty: TypeId) -> Option<String> {
    let type_def = ctx.get(ty)?;

    match type_def {
        Type::Union(union_ty) => {
            let discriminant = union_ty.discriminant.as_ref()?;
            Some(discriminant.field_name.clone())
        }
        _ => None,
    }
}

/// Check exhaustiveness for typeof-based switch on bare union
///
/// This checks that all primitive types in a bare union are covered
/// by typeof checks in a switch statement.
pub fn check_typeof_exhaustiveness(
    ctx: &TypeContext,
    bare_union: TypeId,
    tested_types: &HashSet<String>,
) -> Option<Vec<String>> {
    use raya_types::PrimitiveType;

    let type_def = ctx.get(bare_union)?;

    match type_def {
        Type::Union(union_ty) if union_ty.is_bare => {
            // Extract all primitive type names from the bare union
            let all_types: HashSet<String> = union_ty.members.iter()
                .filter_map(|&member| {
                    if let Some(Type::Primitive(prim)) = ctx.get(member) {
                        Some(prim.type_name().to_string())
                    } else {
                        None
                    }
                })
                .collect();

            // Find missing types
            let missing: Vec<String> = all_types.iter()
                .filter(|t| !tested_types.contains(*t))
                .cloned()
                .collect();

            if missing.is_empty() {
                None
            } else {
                Some(missing)
            }
        }
        _ => None, // Not a bare union
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

            // Use the value_map from discriminant inference
            // The value_map contains all discriminant values that exist in the union
            let variants: HashSet<String> = discriminant.value_map.keys().cloned().collect();

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

            // Extract the literal value from the discriminant field's type
            let field_type = ctx.get(prop.ty)?;
            match field_type {
                Type::StringLiteral(s) => Some(s.clone()),
                Type::NumberLiteral(n) => Some(n.to_string()),
                Type::BooleanLiteral(b) => Some(b.to_string()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Check if a switch statement has a default case
fn has_default_case(cases: &[SwitchCase]) -> bool {
    cases.iter().any(|case| case.test.is_none())
}

/// Extract all tested variants from switch cases
fn extract_tested_variants(cases: &[SwitchCase], interner: &Interner) -> HashSet<String> {
    let mut variants = HashSet::new();

    for case in cases {
        if let Some(ref test) = case.test {
            if let Some(variant) = extract_variant_from_expression(test, interner) {
                variants.insert(variant);
            }
        }
    }

    variants
}

/// Extract a variant string from a case test expression
///
/// For now, this only handles string literals.
fn extract_variant_from_expression(expr: &Expression, interner: &Interner) -> Option<String> {
    match expr {
        Expression::StringLiteral(s) => Some(interner.resolve(s.value).to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use raya_parser::Parser;
    use raya_parser::ast::Statement;

    fn parse_switch(source: &str) -> (SwitchStatement, Interner) {
        let parser = Parser::new(source).unwrap();
        let (module, interner) = parser.parse().unwrap();
        match &module.statements[0] {
            Statement::Switch(switch_stmt) => (switch_stmt.clone(), interner),
            _ => panic!("Expected switch statement"),
        }
    }

    #[test]
    fn test_has_default_case() {
        // No default case
        let (switch_stmt, _interner) = parse_switch(r#"
            switch (x) {
                case "ok": break;
            }
        "#);
        assert!(!has_default_case(&switch_stmt.cases));

        // Has default case
        let (switch_stmt, _interner) = parse_switch(r#"
            switch (x) {
                case "ok": break;
                default: break;
            }
        "#);
        assert!(has_default_case(&switch_stmt.cases));
    }

    #[test]
    fn test_extract_tested_variants() {
        let (switch_stmt, interner) = parse_switch(r#"
            switch (x) {
                case "ok": break;
                case "error": break;
            }
        "#);

        let variants = extract_tested_variants(&switch_stmt.cases, &interner);
        assert_eq!(variants.len(), 2);
        assert!(variants.contains("ok"));
        assert!(variants.contains("error"));
    }

    #[test]
    fn test_extract_variant_from_string_literal() {
        let parser = Parser::new(r#""test_variant""#).unwrap();
        let (module, interner) = parser.parse().unwrap();
        let expr = match &module.statements[0] {
            Statement::Expression(expr_stmt) => &expr_stmt.expression,
            _ => panic!("Expected expression statement"),
        };

        let variant = extract_variant_from_expression(expr, &interner);
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

    #[test]
    fn test_typeof_exhaustiveness_bare_union() {
        let mut ctx = TypeContext::new();

        // Create bare union: string | number | boolean
        let string = ctx.string_type();
        let number = ctx.number_type();
        let boolean = ctx.boolean_type();
        let bare_union = ctx.union_type(vec![string, number, boolean]);

        // Test exhaustive case - all types covered
        let mut tested = HashSet::new();
        tested.insert("string".to_string());
        tested.insert("number".to_string());
        tested.insert("boolean".to_string());

        let result = check_typeof_exhaustiveness(&ctx, bare_union, &tested);
        assert!(result.is_none(), "Should be exhaustive");

        // Test non-exhaustive case - missing boolean
        let mut tested_partial = HashSet::new();
        tested_partial.insert("string".to_string());
        tested_partial.insert("number".to_string());

        let result = check_typeof_exhaustiveness(&ctx, bare_union, &tested_partial);
        assert!(result.is_some(), "Should be non-exhaustive");
        let missing = result.unwrap();
        assert_eq!(missing.len(), 1);
        assert!(missing.contains(&"boolean".to_string()));
    }
}

//! Rule registry — all available lint rules.

pub mod naming_convention;
pub mod no_async_without_await;
pub mod no_constant_condition;
pub mod no_duplicate_imports;
pub mod no_empty_block;
pub mod no_self_assign;
pub mod no_throw_literal;
pub mod prefer_const;

use super::rule::LintRule;

/// Returns all available lint rules with their default configuration.
pub fn all_rules() -> Vec<Box<dyn LintRule>> {
    vec![
        // Correctness
        Box::new(no_self_assign::NoSelfAssign),
        Box::new(no_constant_condition::NoConstantCondition),
        Box::new(no_duplicate_imports::NoDuplicateImports),
        // Style
        Box::new(no_empty_block::NoEmptyBlock),
        Box::new(prefer_const::PreferConst),
        Box::new(naming_convention::NamingConvention),
        // Best Practice
        Box::new(no_throw_literal::NoThrowLiteral),
        Box::new(no_async_without_await::NoAsyncWithoutAwait),
    ]
}

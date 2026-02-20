//! Rule registry — all available lint rules.

pub mod await_in_loop;
pub mod explicit_return_type;
pub mod explicit_visibility;
pub mod naming_convention;
pub mod no_async_without_await;
pub mod no_constant_condition;
pub mod no_duplicate_case;
pub mod no_duplicate_imports;
pub mod no_empty_block;
pub mod no_fallthrough;
pub mod no_floating_task;
pub mod no_invalid_typeof;
pub mod no_self_assign;
pub mod no_throw_literal;
pub mod no_typeof_class;
pub mod prefer_const;

use super::rule::LintRule;

/// Returns all available lint rules with their default configuration.
pub fn all_rules() -> Vec<Box<dyn LintRule>> {
    vec![
        // Correctness
        Box::new(no_self_assign::NoSelfAssign),
        Box::new(no_constant_condition::NoConstantCondition),
        Box::new(no_duplicate_imports::NoDuplicateImports),
        Box::new(no_duplicate_case::NoDuplicateCase),
        Box::new(no_fallthrough::NoFallthrough),
        Box::new(await_in_loop::AwaitInLoop),
        Box::new(no_floating_task::NoFloatingTask),
        Box::new(no_invalid_typeof::NoInvalidTypeof),
        Box::new(no_typeof_class::NoTypeofClass),
        // Style
        Box::new(no_empty_block::NoEmptyBlock),
        Box::new(prefer_const::PreferConst),
        Box::new(naming_convention::NamingConvention),
        Box::new(explicit_return_type::ExplicitReturnType),
        Box::new(explicit_visibility::ExplicitVisibility),
        // Best Practice
        Box::new(no_throw_literal::NoThrowLiteral),
        Box::new(no_async_without_await::NoAsyncWithoutAwait),
    ]
}

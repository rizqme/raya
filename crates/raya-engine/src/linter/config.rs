//! Lint configuration: per-rule severity overrides.

use std::collections::HashMap;

use super::rule::Severity;

/// Configuration for the linter, loaded from `[lint]` in `raya.toml`.
#[derive(Debug, Clone, Default)]
pub struct LintConfig {
    /// Per-rule severity overrides. Key = rule name (e.g. "no-empty-block").
    overrides: HashMap<String, Severity>,
}

impl LintConfig {
    /// Create a new empty config (all rules use their default severity).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the severity for a specific rule.
    pub fn set_severity(&mut self, rule_name: &str, severity: Severity) {
        self.overrides.insert(rule_name.to_string(), severity);
    }

    /// Get the effective severity for a rule, falling back to its default.
    pub fn effective_severity(&self, rule_name: &str, default: Severity) -> Severity {
        self.overrides.get(rule_name).copied().unwrap_or(default)
    }

    /// Check if a rule is explicitly disabled.
    pub fn is_disabled(&self, rule_name: &str) -> bool {
        self.overrides.get(rule_name) == Some(&Severity::Off)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = LintConfig::new();
        // No overrides → falls back to the provided default
        assert_eq!(config.effective_severity("no-empty-block", Severity::Warn), Severity::Warn);
        assert_eq!(config.effective_severity("no-empty-block", Severity::Error), Severity::Error);
    }

    #[test]
    fn test_override_severity() {
        let mut config = LintConfig::new();
        config.set_severity("prefer-const", Severity::Error);

        assert_eq!(config.effective_severity("prefer-const", Severity::Warn), Severity::Error);
        // Other rules unaffected
        assert_eq!(config.effective_severity("no-empty-block", Severity::Warn), Severity::Warn);
    }

    #[test]
    fn test_disable_rule() {
        let mut config = LintConfig::new();
        config.set_severity("no-empty-block", Severity::Off);

        assert!(config.is_disabled("no-empty-block"));
        assert!(!config.is_disabled("prefer-const"));
        assert_eq!(config.effective_severity("no-empty-block", Severity::Warn), Severity::Off);
    }
}

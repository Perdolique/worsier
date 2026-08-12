use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::FormatError;

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct FormatConfig {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    #[schemars(range(min = 1))]
    pub line_width: u32,
    pub verify_ast: bool,
    pub rules: RulesConfig,
    pub ignore_patterns: Vec<String>,
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self {
            schema: None,
            line_width: 120,
            verify_ast: true,
            rules: RulesConfig::default(),
            ignore_patterns: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct RulesConfig {
    pub imports: bool,
    pub variables: bool,
}

impl Default for RulesConfig {
    fn default() -> Self {
        Self {
            imports: true,
            variables: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ResolvedConfig {
    value: FormatConfig,
}

impl ResolvedConfig {
    #[must_use]
    pub const fn line_width(&self) -> u32 {
        self.value.line_width
    }

    #[must_use]
    pub const fn verify_ast(&self) -> bool {
        self.value.verify_ast
    }

    #[must_use]
    pub const fn imports_enabled(&self) -> bool {
        self.value.rules.imports
    }

    #[must_use]
    pub const fn variables_enabled(&self) -> bool {
        self.value.rules.variables
    }

    #[must_use]
    pub fn ignore_patterns(&self) -> &[String] {
        &self.value.ignore_patterns
    }
}

/// Resolves and validates a raw formatter configuration.
///
/// # Errors
///
/// Returns [`FormatError::InvalidConfig`] when `lineWidth` is zero.
pub fn resolve_config(config: FormatConfig) -> Result<ResolvedConfig, FormatError> {
    if config.line_width == 0 {
        return Err(FormatError::invalid_config(
            "lineWidth must be greater than zero",
        ));
    }

    Ok(ResolvedConfig { value: config })
}

#[cfg(test)]
mod tests {
    use super::{FormatConfig, resolve_config};

    #[test]
    fn resolves_documented_defaults() {
        let config = resolve_config(FormatConfig::default()).unwrap();
        assert_eq!(config.line_width(), 120);
        assert!(config.verify_ast());
        assert!(config.imports_enabled());
        assert!(config.variables_enabled());
    }

    #[test]
    fn rejects_zero_line_width() {
        let config = FormatConfig {
            line_width: 0,
            ..FormatConfig::default()
        };
        assert!(resolve_config(config).is_err());
    }

    #[test]
    fn rejects_removed_and_unknown_keys() {
        for source in [
            r#"{"quoteStyle":"single"}"#,
            r#"{"imports":{"specifierLayout":"auto"}}"#,
            r#"{"statementSpacing":[]}"#,
            r#"{"rules":{"objects":true}}"#,
        ] {
            let error = serde_json::from_str::<FormatConfig>(source).unwrap_err();
            assert!(error.to_string().contains("unknown field"));
        }
    }

    #[test]
    fn generated_schema_uses_runtime_numeric_range() {
        let schema = serde_json::to_value(schemars::schema_for!(FormatConfig)).unwrap();
        assert_eq!(schema["properties"]["lineWidth"]["minimum"], 1);
    }
}

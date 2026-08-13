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
    pub import_layout: bool,
    pub statement_spacing: StatementSpacingConfig,
    pub trailing_commas: TrailingCommaMode,
}

impl Default for RulesConfig {
    fn default() -> Self {
        Self {
            import_layout: true,
            statement_spacing: StatementSpacingConfig::default(),
            trailing_commas: TrailingCommaMode::Never,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct StatementSpacingConfig {
    pub imports: StatementSpacingMode,
    pub variable_declarations: StatementSpacingMode,
}

impl Default for StatementSpacingConfig {
    fn default() -> Self {
        Self {
            imports: StatementSpacingMode::Separate,
            variable_declarations: StatementSpacingMode::Separate,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StatementSpacingMode {
    #[default]
    Separate,
    Compact,
    Off,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TrailingCommaMode {
    Always,
    #[default]
    Never,
    Off,
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
    pub const fn import_layout_enabled(&self) -> bool {
        self.value.rules.import_layout
    }

    #[must_use]
    pub const fn import_spacing(&self) -> StatementSpacingMode {
        self.value.rules.statement_spacing.imports
    }

    #[must_use]
    pub const fn variable_declaration_spacing(&self) -> StatementSpacingMode {
        self.value.rules.statement_spacing.variable_declarations
    }

    #[must_use]
    pub const fn trailing_commas(&self) -> TrailingCommaMode {
        self.value.rules.trailing_commas
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
    use super::{FormatConfig, StatementSpacingMode, TrailingCommaMode, resolve_config};

    #[test]
    fn resolves_documented_defaults() {
        let config = resolve_config(FormatConfig::default()).unwrap();
        assert_eq!(config.line_width(), 120);
        assert!(config.verify_ast());
        assert!(config.import_layout_enabled());
        assert_eq!(config.import_spacing(), StatementSpacingMode::Separate);
        assert_eq!(
            config.variable_declaration_spacing(),
            StatementSpacingMode::Separate
        );
        assert_eq!(config.trailing_commas(), TrailingCommaMode::Never);
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
            r#"{"rules":{"imports":true}}"#,
            r#"{"rules":{"variables":true}}"#,
            r#"{"trailingCommas":"always"}"#,
            r#"{"rules":{"trailingCommas":"multiline"}}"#,
            r#"{"rules":{"statementSpacing":{"imports":"preserve"}}}"#,
            r#"{"rules":{"statementSpacing":{"variables":"compact"}}}"#,
        ] {
            let error = serde_json::from_str::<FormatConfig>(source).unwrap_err();
            assert!(
                error.to_string().contains("unknown field")
                    || error.to_string().contains("unknown variant")
            );
        }
    }

    #[test]
    fn partial_nested_configs_keep_their_own_defaults() {
        let config: FormatConfig = serde_json::from_str(
            r#"{"rules":{"importLayout":false,"statementSpacing":{"imports":"compact"}}}"#,
        )
        .unwrap();
        let config = resolve_config(config).unwrap();

        assert!(!config.import_layout_enabled());
        assert_eq!(config.import_spacing(), StatementSpacingMode::Compact);
        assert_eq!(
            config.variable_declaration_spacing(),
            StatementSpacingMode::Separate
        );
        assert_eq!(config.trailing_commas(), TrailingCommaMode::Never);
    }

    #[test]
    fn accepts_all_trailing_comma_modes() {
        for (value, expected) in [
            ("always", TrailingCommaMode::Always),
            ("never", TrailingCommaMode::Never),
            ("off", TrailingCommaMode::Off),
        ] {
            let source = format!(r#"{{"rules":{{"trailingCommas":"{value}"}}}}"#);
            let config: FormatConfig = serde_json::from_str(&source).unwrap();
            assert_eq!(resolve_config(config).unwrap().trailing_commas(), expected);
        }
    }

    #[test]
    fn generated_schema_uses_runtime_numeric_range() {
        let schema = serde_json::to_value(schemars::schema_for!(FormatConfig)).unwrap();
        assert_eq!(schema["properties"]["lineWidth"]["minimum"], 1);
    }
}

use schemars::JsonSchema;
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, Unexpected, Visitor},
};

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
    pub interface_layout: InterfaceLayoutRule,
    pub statement_spacing: StatementSpacingConfig,
    pub semicolons: SemicolonConfig,
    pub trailing_commas: TrailingCommaMode,
}

impl Default for RulesConfig {
    fn default() -> Self {
        Self {
            import_layout: true,
            interface_layout: InterfaceLayoutRule::default(),
            statement_spacing: StatementSpacingConfig::default(),
            semicolons: SemicolonConfig::default(),
            trailing_commas: TrailingCommaMode::Never,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(untagged)]
pub enum InterfaceLayoutRule {
    Threshold(#[schemars(range(max = 4_294_967_295_u32))] u32),
    Mode(InterfaceLayoutMode),
}

impl<'de> Deserialize<'de> for InterfaceLayoutRule {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct InterfaceLayoutVisitor;

        impl Visitor<'_> for InterfaceLayoutVisitor {
            type Value = InterfaceLayoutRule;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(r#""off" or an integer from 0 to 4294967295"#)
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                u32::try_from(value)
                    .map(InterfaceLayoutRule::Threshold)
                    .map_err(|_| E::invalid_value(Unexpected::Unsigned(value), &self))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                u64::try_from(value)
                    .map_err(|_| E::invalid_value(Unexpected::Signed(value), &self))
                    .and_then(|value| self.visit_u64(value))
            }

            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "the finite integer and u32 range checks make this conversion exact"
            )]
            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if value.is_finite()
                    && value.fract() == 0.0
                    && value >= 0.0
                    && value <= f64::from(u32::MAX)
                {
                    return Ok(InterfaceLayoutRule::Threshold(value as u32));
                }
                Err(E::invalid_value(Unexpected::Float(value), &self))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if value == "off" {
                    Ok(InterfaceLayoutRule::Mode(InterfaceLayoutMode::Off))
                } else {
                    Err(E::invalid_value(Unexpected::Str(value), &self))
                }
            }
        }

        deserializer.deserialize_any(InterfaceLayoutVisitor)
    }
}

impl Default for InterfaceLayoutRule {
    fn default() -> Self {
        Self::Threshold(0)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InterfaceLayoutMode {
    Off,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct SemicolonConfig {
    pub statements: SemicolonMode,
    pub class_members: SemicolonMode,
    pub type_members: SemicolonMode,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SemicolonMode {
    Always,
    #[default]
    AsNeeded,
    Off,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct StatementSpacingConfig {
    pub imports: StatementSpacingMode,
    pub type_aliases: StatementSpacingMode,
    pub variable_declarations: StatementSpacingMode,
}

impl Default for StatementSpacingConfig {
    fn default() -> Self {
        Self {
            imports: StatementSpacingMode::Separate,
            type_aliases: StatementSpacingMode::Separate,
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
    pub const fn interface_layout_threshold(&self) -> Option<u32> {
        match self.value.rules.interface_layout {
            InterfaceLayoutRule::Threshold(threshold) => Some(threshold),
            InterfaceLayoutRule::Mode(InterfaceLayoutMode::Off) => None,
        }
    }

    #[must_use]
    pub const fn import_spacing(&self) -> StatementSpacingMode {
        self.value.rules.statement_spacing.imports
    }

    #[must_use]
    pub const fn type_alias_spacing(&self) -> StatementSpacingMode {
        self.value.rules.statement_spacing.type_aliases
    }

    #[must_use]
    pub const fn variable_declaration_spacing(&self) -> StatementSpacingMode {
        self.value.rules.statement_spacing.variable_declarations
    }

    #[must_use]
    pub const fn statement_semicolons(&self) -> SemicolonMode {
        self.value.rules.semicolons.statements
    }

    #[must_use]
    pub const fn class_member_semicolons(&self) -> SemicolonMode {
        self.value.rules.semicolons.class_members
    }

    #[must_use]
    pub const fn type_member_semicolons(&self) -> SemicolonMode {
        self.value.rules.semicolons.type_members
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
    use super::{
        FormatConfig, InterfaceLayoutMode, InterfaceLayoutRule, SemicolonMode,
        StatementSpacingMode, TrailingCommaMode, resolve_config,
    };

    #[test]
    fn resolves_documented_defaults() {
        let config = resolve_config(FormatConfig::default()).unwrap();
        assert_eq!(config.line_width(), 120);
        assert!(config.verify_ast());
        assert!(config.import_layout_enabled());
        assert_eq!(config.interface_layout_threshold(), Some(0));
        assert_eq!(config.import_spacing(), StatementSpacingMode::Separate);
        assert_eq!(config.type_alias_spacing(), StatementSpacingMode::Separate);
        assert_eq!(
            config.variable_declaration_spacing(),
            StatementSpacingMode::Separate
        );
        assert_eq!(config.statement_semicolons(), SemicolonMode::AsNeeded);
        assert_eq!(config.class_member_semicolons(), SemicolonMode::AsNeeded);
        assert_eq!(config.type_member_semicolons(), SemicolonMode::AsNeeded);
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
            r#"{"semicolons":"always"}"#,
            r#"{"rules":{"trailingCommas":"multiline"}}"#,
            r#"{"rules":{"semicolons":"always"}}"#,
            r#"{"rules":{"semicolons":{"statements":"never"}}}"#,
            r#"{"rules":{"semicolons":{"extra":"off"}}}"#,
            r#"{"rules":{"statementSpacing":{"imports":"preserve"}}}"#,
            r#"{"rules":{"statementSpacing":{"typeAliases":"preserve"}}}"#,
            r#"{"rules":{"statementSpacing":{"variables":"compact"}}}"#,
        ] {
            let error = serde_json::from_str::<FormatConfig>(source).unwrap_err();
            assert!(
                error.to_string().contains("unknown field")
                    || error.to_string().contains("unknown variant")
                    || error.to_string().contains("invalid type")
            );
        }
    }

    #[test]
    fn partial_nested_configs_keep_their_own_defaults() {
        let config: FormatConfig = serde_json::from_str(
            r#"{"rules":{"importLayout":false,"statementSpacing":{"imports":"compact","typeAliases":"off"},"semicolons":{"typeMembers":"always"}}}"#,
        )
        .unwrap();
        let config = resolve_config(config).unwrap();

        assert!(!config.import_layout_enabled());
        assert_eq!(config.interface_layout_threshold(), Some(0));
        assert_eq!(config.import_spacing(), StatementSpacingMode::Compact);
        assert_eq!(config.type_alias_spacing(), StatementSpacingMode::Off);
        assert_eq!(
            config.variable_declaration_spacing(),
            StatementSpacingMode::Separate
        );
        assert_eq!(config.statement_semicolons(), SemicolonMode::AsNeeded);
        assert_eq!(config.class_member_semicolons(), SemicolonMode::AsNeeded);
        assert_eq!(config.type_member_semicolons(), SemicolonMode::Always);
        assert_eq!(config.trailing_commas(), TrailingCommaMode::Never);
    }

    #[test]
    fn accepts_all_semicolon_modes_for_each_group() {
        for (value, expected) in [
            ("always", SemicolonMode::Always),
            ("asNeeded", SemicolonMode::AsNeeded),
            ("off", SemicolonMode::Off),
        ] {
            let source = format!(
                r#"{{"rules":{{"semicolons":{{"statements":"{value}","classMembers":"{value}","typeMembers":"{value}"}}}}}}"#
            );
            let config: FormatConfig = serde_json::from_str(&source).unwrap();
            let config = resolve_config(config).unwrap();
            assert_eq!(config.statement_semicolons(), expected);
            assert_eq!(config.class_member_semicolons(), expected);
            assert_eq!(config.type_member_semicolons(), expected);
        }
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
        assert_eq!(
            schema["$defs"]["InterfaceLayoutRule"]["anyOf"][0]["minimum"],
            0
        );
        assert_eq!(
            schema["$defs"]["InterfaceLayoutRule"]["anyOf"][0]["maximum"],
            u32::MAX
        );
    }

    #[test]
    fn accepts_interface_layout_thresholds_and_off() {
        for (value, expected) in [
            ("0", InterfaceLayoutRule::Threshold(0)),
            ("1.0", InterfaceLayoutRule::Threshold(1)),
            ("1e0", InterfaceLayoutRule::Threshold(1)),
            ("3", InterfaceLayoutRule::Threshold(3)),
            (
                r#""off""#,
                InterfaceLayoutRule::Mode(InterfaceLayoutMode::Off),
            ),
        ] {
            let source = format!(r#"{{"rules":{{"interfaceLayout":{value}}}}}"#);
            let config: FormatConfig = serde_json::from_str(&source).unwrap();
            assert_eq!(config.rules.interface_layout, expected);
        }
    }

    #[test]
    fn rejects_invalid_interface_layout_values() {
        for value in ["-1", "1.5", "4294967296", r#""always""#, "true", "null"] {
            let source = format!(r#"{{"rules":{{"interfaceLayout":{value}}}}}"#);
            assert!(serde_json::from_str::<FormatConfig>(&source).is_err());
        }
    }
}

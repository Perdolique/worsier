use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::FormatError;

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct FormatConfig {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    #[schemars(range(min = 1, max = 320))]
    pub line_width: u16,
    pub indent_style: IndentStyle,
    #[schemars(range(min = 0, max = 24))]
    pub indent_width: u8,
    pub line_ending: LineEnding,
    pub quote_style: QuoteStyle,
    pub semicolons: Semicolons,
    pub trailing_commas: TrailingCommas,
    pub bracket_spacing: bool,
    pub arrow_parentheses: ArrowParentheses,
    pub final_newline: bool,
    pub verify_ast: bool,
    pub objects: ObjectConfig,
    pub arrays: ArrayConfig,
    pub imports: ImportConfig,
    pub statement_spacing: Vec<StatementSpacingRule>,
    pub ignore_patterns: Vec<String>,
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self {
            schema: None,
            line_width: 100,
            indent_style: IndentStyle::Space,
            indent_width: 2,
            line_ending: LineEnding::Preserve,
            quote_style: QuoteStyle::Single,
            semicolons: Semicolons::Always,
            trailing_commas: TrailingCommas::Multiline,
            bracket_spacing: true,
            arrow_parentheses: ArrowParentheses::Always,
            final_newline: true,
            verify_ast: true,
            objects: ObjectConfig::default(),
            arrays: ArrayConfig::default(),
            imports: ImportConfig::default(),
            statement_spacing: Vec::new(),
            ignore_patterns: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum IndentStyle {
    #[default]
    Space,
    Tab,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LineEnding {
    #[default]
    Preserve,
    Lf,
    Crlf,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum QuoteStyle {
    #[default]
    Single,
    Double,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Semicolons {
    #[default]
    Always,
    AsNeeded,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TrailingCommas {
    None,
    #[default]
    Multiline,
    All,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ArrowParentheses {
    #[default]
    Always,
    AsNeeded,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CollectionLayout {
    #[default]
    Auto,
    Preserve,
    SingleLine,
    MultiLine,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CollectionItemLayout {
    #[default]
    Auto,
    Preserve,
    OnePerLine,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct ObjectConfig {
    pub layout: CollectionLayout,
    pub property_layout: CollectionItemLayout,
    pub when_array_element: ObjectArrayLayout,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ObjectArrayLayout {
    #[default]
    Inherit,
    MultiLine,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct ArrayConfig {
    pub layout: CollectionLayout,
    pub element_layout: CollectionItemLayout,
    pub object_elements: ArrayObjectLayout,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ArrayObjectLayout {
    #[default]
    Inherit,
    OnePerLine,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct ImportConfig {
    pub specifier_layout: CollectionItemLayout,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StatementSpacingRule {
    pub previous: StatementSelector,
    pub next: StatementSelector,
    #[serde(default)]
    pub scope: StatementScope,
    pub blank_lines: u8,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct StatementSelector {
    pub kind: StatementKind,
    pub line_shape: LineShape,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StatementKind {
    #[default]
    Any,
    Import,
    Export,
    Const,
    Let,
    Var,
    Function,
    Class,
    Type,
    Interface,
    Enum,
    Namespace,
    Other,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LineShape {
    #[default]
    Any,
    SingleLine,
    MultiLine,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StatementScope {
    #[default]
    Any,
    TopLevel,
    Block,
}

#[derive(Clone, Debug)]
pub struct ResolvedConfig {
    value: FormatConfig,
}

impl ResolvedConfig {
    #[must_use]
    pub const fn line_width(&self) -> u16 {
        self.value.line_width
    }

    #[must_use]
    pub const fn indent_style(&self) -> IndentStyle {
        self.value.indent_style
    }

    #[must_use]
    pub const fn indent_width(&self) -> u8 {
        self.value.indent_width
    }

    #[must_use]
    pub const fn line_ending(&self) -> LineEnding {
        self.value.line_ending
    }

    #[must_use]
    pub const fn quote_style(&self) -> QuoteStyle {
        self.value.quote_style
    }

    #[must_use]
    pub const fn semicolons(&self) -> Semicolons {
        self.value.semicolons
    }

    #[must_use]
    pub const fn trailing_commas(&self) -> TrailingCommas {
        self.value.trailing_commas
    }

    #[must_use]
    pub const fn bracket_spacing(&self) -> bool {
        self.value.bracket_spacing
    }

    #[must_use]
    pub const fn arrow_parentheses(&self) -> ArrowParentheses {
        self.value.arrow_parentheses
    }

    #[must_use]
    pub const fn final_newline(&self) -> bool {
        self.value.final_newline
    }

    #[must_use]
    pub const fn verify_ast(&self) -> bool {
        self.value.verify_ast
    }

    #[must_use]
    pub const fn objects(&self) -> &ObjectConfig {
        &self.value.objects
    }

    #[must_use]
    pub const fn arrays(&self) -> &ArrayConfig {
        &self.value.arrays
    }

    #[must_use]
    pub const fn imports(&self) -> &ImportConfig {
        &self.value.imports
    }

    #[must_use]
    pub fn statement_spacing(&self) -> &[StatementSpacingRule] {
        &self.value.statement_spacing
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
/// Returns [`FormatError::InvalidConfig`] when a numeric setting is outside its
/// documented range.
pub fn resolve_config(config: FormatConfig) -> Result<ResolvedConfig, FormatError> {
    if !(1..=320).contains(&config.line_width) {
        return Err(FormatError::invalid_config(
            "lineWidth must be between 1 and 320",
        ));
    }

    if config.indent_width > 24 {
        return Err(FormatError::invalid_config(
            "indentWidth must be between 0 and 24",
        ));
    }

    Ok(ResolvedConfig { value: config })
}

#[cfg(test)]
mod tests {
    use super::{FormatConfig, LineEnding, QuoteStyle, resolve_config};

    #[test]
    fn resolves_documented_defaults() {
        let config = resolve_config(FormatConfig::default()).unwrap();
        assert_eq!(config.line_width(), 100);
        assert_eq!(config.indent_width(), 2);
        assert!(matches!(config.line_ending(), LineEnding::Preserve));
        assert!(matches!(config.quote_style(), QuoteStyle::Single));
        assert!(config.verify_ast());
    }

    #[test]
    fn rejects_out_of_range_line_width() {
        let config = FormatConfig {
            line_width: 0,
            ..FormatConfig::default()
        };
        assert!(resolve_config(config).is_err());
    }

    #[test]
    fn rejects_out_of_range_indent_width() {
        let config = FormatConfig {
            indent_width: 25,
            ..FormatConfig::default()
        };
        assert!(resolve_config(config).is_err());
    }

    #[test]
    fn rejects_unknown_keys() {
        let error = serde_json::from_str::<FormatConfig>(r#"{"linewidth": 80}"#).unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn generated_schema_uses_runtime_numeric_ranges() {
        let schema = serde_json::to_value(schemars::schema_for!(FormatConfig)).unwrap();
        assert_eq!(schema["properties"]["lineWidth"]["minimum"], 1);
        assert_eq!(schema["properties"]["lineWidth"]["maximum"], 320);
        assert_eq!(schema["properties"]["indentWidth"]["minimum"], 0);
        assert_eq!(schema["properties"]["indentWidth"]["maximum"], 24);
    }
}

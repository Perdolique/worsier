use std::path::Path;

use worsier_formatter::{
    FormatConfig, InterfaceLayoutMode, InterfaceLayoutRule, QuoteStyle, SemicolonMode,
    StatementSpacingMode, TrailingCommaMode, format_text, resolve_config,
};

fn quote_config(style: QuoteStyle) -> FormatConfig {
    let mut config = FormatConfig::default();
    config.rules.comment_spacing = false;
    config.rules.import_layout = false;
    config.rules.interface_layout = InterfaceLayoutRule::Mode(InterfaceLayoutMode::Off);
    config.rules.object_property_spacing = false;
    config.rules.quote_style = style;
    let spacing = &mut config.rules.statement_spacing;
    spacing.control_flow_statements = StatementSpacingMode::Off;
    spacing.imports = StatementSpacingMode::Off;
    spacing.multiline_call_statements = StatementSpacingMode::Off;
    spacing.single_line_call_statements = StatementSpacingMode::Off.into();
    spacing.return_statements = StatementSpacingMode::Off;
    spacing.type_aliases = StatementSpacingMode::Off;
    spacing.variable_declarations = StatementSpacingMode::Off;
    config.rules.semicolons.statements = SemicolonMode::Off;
    config.rules.semicolons.class_members = SemicolonMode::Off;
    config.rules.semicolons.type_members = SemicolonMode::Off.into();
    config.rules.trailing_commas = TrailingCommaMode::Off;
    config
}

fn format(file: &str, source: &str, config: FormatConfig) -> String {
    let config = resolve_config(config).unwrap();
    format_text(Path::new(file), source, &config)
        .unwrap_or_else(|error| panic!("{error}\nsource:\n{source}"))
        .unwrap_or_else(|| source.to_owned())
}

fn assert_format(file: &str, source: &str, expected: &str, config: FormatConfig) {
    let output = format(file, source, config.clone());
    assert_eq!(output, expected);
    assert_eq!(format(file, &output, config), output);
}

#[test]
fn defaults_to_strict_single_quotes_and_normalizes_quote_escapes() {
    assert_format(
        "sample.ts",
        r#"const empty = ""; const apostrophe = "don't"; const double = 'say \"hi\"'; const slashes = "a\\\"b"; const unicode = "Привет"; const escapes = "\x41\u0042\uD800";"#,
        r#"const empty = ''; const apostrophe = 'don\'t'; const double = 'say "hi"'; const slashes = 'a\\"b'; const unicode = 'Привет'; const escapes = '\x41\u0042\uD800';"#,
        quote_config(QuoteStyle::Single),
    );
}

#[test]
fn supports_strict_double_quotes() {
    assert_format(
        "sample.ts",
        r#"const plain = 'text'; const double = 'say "hi"'; const apostrophe = "don\'t"; const slashes = 'a\\\'b';"#,
        r#"const plain = "text"; const double = "say \"hi\""; const apostrophe = "don't"; const slashes = "a\\'b";"#,
        quote_config(QuoteStyle::Double),
    );
}

#[test]
fn off_preserves_quotes_and_escapes_byte_for_byte() {
    let source = r#"const first="double";const second='single';const escaped="don\'t";"#;
    assert_format("sample.ts", source, source, quote_config(QuoteStyle::Off));
}

#[test]
fn covers_types_modules_directives_templates_and_jsx_exclusions() {
    let source = r#""use 'client'";
import { "external" as local } from "pkg" with { type: "json" };
export { local as "public" } from "other";
const object = { "key": "value" };
type Name = "type";
const template = `raw " '${"inside"}`;
const element = <div title="attribute" data-other='preserved'>{"child"}</div>;"#;
    let expected = r#"'use \'client\'';
import { 'external' as local } from 'pkg' with { type: 'json' };
export { local as 'public' } from 'other';
const object = { 'key': 'value' };
type Name = 'type';
const template = `raw " '${'inside'}`;
const element = <div title="attribute" data-other='preserved'>{'child'}</div>;"#;
    assert_format(
        "sample.tsx",
        source,
        expected,
        quote_config(QuoteStyle::Single),
    );
}

#[test]
fn formats_declaration_files_and_vue_scripts_only() {
    assert_format(
        "sample.d.ts",
        r#"declare module "pkg" { export type Value = "literal"; }"#,
        r"declare module 'pkg' { export type Value = 'literal'; }",
        quote_config(QuoteStyle::Single),
    );
    assert_format(
        "sample.vue",
        r#"<template><div title="template">"text"</div></template>
<script setup lang="ts">
const value = "script"
</script>
<style>.label::after { content: "style"; }</style>"#,
        r#"<template><div title="template">"text"</div></template>
<script setup lang="ts">
const value = 'script'
</script>
<style>.label::after { content: "style"; }</style>"#,
        quote_config(QuoteStyle::Single),
    );
}

#[test]
fn preserves_bom_newline_style_line_continuations_and_final_newline() {
    assert_format(
        "sample.js",
        "\u{feff}const value = \"first\\\r\nsecond\";\r\n",
        "\u{feff}const value = 'first\\\r\nsecond';\r\n",
        quote_config(QuoteStyle::Single),
    );
}

#[test]
fn quote_length_drives_import_layout_on_the_first_pass() {
    let mut single = quote_config(QuoteStyle::Single);
    single.line_width = 29;
    single.rules.import_layout = true;
    assert_format(
        "sample.ts",
        r#"import{value}from"don't""#,
        "import {\n  value\n} from 'don\\'t'",
        single,
    );

    let mut double = quote_config(QuoteStyle::Double);
    double.line_width = 29;
    double.rules.import_layout = true;
    assert_format(
        "sample.ts",
        "import {\n  value\n} from 'don\\'t'",
        r#"import { value } from "don't""#,
        double,
    );
}

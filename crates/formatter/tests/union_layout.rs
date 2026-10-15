use std::path::Path;

use worsier_formatter::{
    ClassMemberSpacingMode, FormatConfig, QuoteStyle, SemicolonMode, TypeLiteralLayoutRule,
    UnionLayoutMode, format_text, resolve_config,
};

fn isolated(mode: UnionLayoutMode) -> FormatConfig {
    let mut config: FormatConfig = serde_json::from_value(serde_json::json!({
        "rules": {
            "bracketSpacing": { "curly": "off", "square": "off" },
            "commentSpacing": false, "importLayout": false, "exportLayout": false,
            "classMemberSpacing": "off", "interfaceLayout": "off", "typeLiteralLayout": "off",
            "objectPropertySpacing": false, "quoteStyle": "off",
            "statementSpacing": {
                "controlFlowStatements": "off", "imports": "off", "multilineCallStatements": "off",
                "singleLineCallStatements": "off", "returnStatements": "off",
                "typeAliases": "off", "variableDeclarations": "off"
            },
            "semicolons": { "statements": "off", "classMembers": "off", "typeMembers": "off" },
            "trailingCommas": "off"
        }
    }))
    .unwrap();
    config.rules.union_layout = mode;
    config
}

fn assert_format(file: &str, source: &str, expected: &str, config: &FormatConfig) {
    let config = resolve_config(config.clone()).unwrap();
    assert!(config.verify_ast());
    let newlines: &[&str] = if source.contains('\n') {
        &["\n", "\r\n"]
    } else {
        &["\n"]
    };
    for newline in newlines {
        let source = source.replace('\n', newline);
        let expected = expected.replace('\n', newline);
        let output = format_text(Path::new(file), &source, &config)
            .unwrap_or_else(|error| panic!("{error}\n{file}: {source}"));
        let output = output.as_deref().unwrap_or(&source);
        assert_eq!(output, expected, "source: {source:?}");
        assert!(
            format_text(Path::new(file), output, &config)
                .unwrap()
                .is_none(),
            "second pass changed: {output:?}"
        );
    }
}

#[test]
fn supports_modes_and_the_inclusive_width_boundary() {
    let flat = "type Identifier = string | number";
    let expanded = "type Identifier =\n  | string\n  | number";
    let mut config = isolated(UnionLayoutMode::Auto);
    config.line_width = u32::try_from(flat.chars().count()).unwrap();
    for source in [
        flat,
        expanded,
        "type Identifier = string |\n  number",
        "type Identifier = | string | number",
    ] {
        assert_format("sample.ts", source, flat, &config);
    }
    config.line_width -= 1;
    assert_format("sample.ts", flat, expanded, &config);
    assert_format("sample.ts", expanded, expanded, &config);
    config.rules.union_layout = UnionLayoutMode::Always;
    config.line_width = 1_000;
    assert_format("sample.ts", flat, expanded, &config);
    config.rules.union_layout = UnionLayoutMode::Off;
    for source in [flat, expanded, "type Identifier = string |\n  number"] {
        assert_format("sample.ts", source, source, &config);
    }
    assert_format(
        "sample.ts",
        "type A = | string",
        "type A = | string",
        &isolated(UnionLayoutMode::Always),
    );
}

#[test]
fn uses_default_auto_and_preserves_other_rules_when_off() {
    let mut config = FormatConfig::default();
    assert_format(
        "sample.ts",
        "type A =\n | \"one\"\n | \"two\";",
        "type A = 'one' | 'two'",
        &config,
    );
    config.rules.union_layout = UnionLayoutMode::Off;
    assert_format(
        "sample.ts",
        "type A =\n | \"one\"\n | \"two\";",
        "type A =\n | 'one'\n | 'two'",
        &config,
    );
}

#[test]
fn covers_type_positions_and_preserves_precedence_and_parentheses() {
    let config = isolated(UnionLayoutMode::Always);
    for (source, expected) in [
        ("let value: A | B", "let value:\n  | A\n  | B"),
        (
            "declare function f(value: A | B): C | D;",
            "declare function f(value:\n  | A\n  | B):\n  | C\n  | D;",
        ),
        ("type U = (A | B)[]", "type U = (\n  | A\n  | B)[]"),
        ("type U = (A | B) & C", "type U = (\n  | A\n  | B) & C"),
        ("type U = (() => A) | B", "type U =\n  | (() => A)\n  | B"),
        ("type U = A | (() => B)", "type U =\n  | A\n  | (() => B)"),
        (
            "type U = ((A | B)) | ((C | D))",
            "type U =\n  | ((\n    | A\n    | B))\n  | ((\n    | C\n    | D))",
        ),
        (
            "type U = T extends A | B ? C | D : E",
            "type U = T extends\n  | A\n  | B ?\n  | C\n  | D : E",
        ),
        (
            "type U = Outer<A | B> | C",
            "type U =\n  | Outer<\n    | A\n    | B>\n  | C",
        ),
        ("type U = [A | B, C]", "type U = [\n  | A\n  | B, C]"),
    ] {
        for file in [
            "sample.ts",
            "sample.mts",
            "sample.cts",
            "sample.d.ts",
            "sample.tsx",
        ] {
            assert_format(file, source, expected, &config);
        }
    }
}

#[test]
fn measures_the_complete_final_line_including_quotes_semicolons_and_neighbors() {
    let source = "type U = \"a'b\" | \"c\";";
    let flat = "type U = 'a\\'b' | 'c'";
    let mut config = isolated(UnionLayoutMode::Auto);
    config.rules.quote_style = QuoteStyle::Single;
    config.rules.semicolons.statements = SemicolonMode::AsNeeded;
    config.line_width = u32::try_from(flat.chars().count()).unwrap();
    assert_format("sample.ts", source, flat, &config);
    config.rules.semicolons.statements = SemicolonMode::Always;
    assert_format(
        "sample.ts",
        source,
        "type U =\n  | 'a\\'b'\n  | 'c';",
        &config,
    );
    config.rules.quote_style = QuoteStyle::Double;
    config.line_width = u32::try_from(source.chars().count()).unwrap();
    assert_format("sample.ts", source, source, &config);
    let mut config = isolated(UnionLayoutMode::Auto);
    let source = "let first: A | B; let second: C | D";
    config.line_width = 25;
    assert_format(
        "sample.ts",
        source,
        "let first: A | B;\nlet second: C | D",
        &config,
    );
}

#[test]
fn accounts_for_parent_indentation_and_cascades_to_class_fields() {
    let mut config = isolated(UnionLayoutMode::Auto);
    config.rules.class_member_spacing = ClassMemberSpacingMode::Smart;
    config.line_width = 16;
    assert_format(
        "sample.ts",
        "class C { a: A | B; b: C | D; }",
        "class C {\n  a: A | B;\n  b: C | D;\n}",
        &config,
    );
    config.line_width = 10;
    assert_format(
        "sample.ts",
        "class C { a: A | B; b: C | D; }",
        "class C {\n  a:\n    | A\n    | B;\n\n  b:\n    | C\n    | D;\n}",
        &config,
    );
    config.line_width = 120;
    assert_format(
        "sample.ts",
        "class C {\n  a:\n    | A\n    | B;\n\n  b: C;\n}",
        "class C {\n  a: A | B;\n  b: C;\n}",
        &config,
    );
}

#[test]
fn preserves_containing_line_indentation_when_parent_layout_is_disabled() {
    let config = isolated(UnionLayoutMode::Always);
    for (source, expected) in [
        (
            "function f(\n    a: A | B\n) {}",
            "function f(\n    a:\n      | A\n      | B\n) {}",
        ),
        (
            "type U = {\n    value: A | B\n}",
            "type U = {\n    value:\n      | A\n      | B\n}",
        ),
        (
            "class C {\n    value: A | B\n}",
            "class C {\n    value:\n      | A\n      | B\n}",
        ),
        (
            "type U = {\n    value: A | B\n} | C",
            "type U =\n  | {\n    value:\n      | A\n      | B\n}\n  | C",
        ),
        (
            "type U = {\n\tvalue: A | B\n}",
            "type U = {\n\tvalue:\n\t  | A\n\t  | B\n}",
        ),
    ] {
        assert_format("sample.ts", source, expected, &config);
    }
    let mut config = FormatConfig::default();
    config.rules.union_layout = UnionLayoutMode::Always;
    assert_format(
        "sample.ts",
        "function f(\n    a: A | B\n) {}",
        "function f(\n    a:\n      | A\n      | B\n) {}",
        &config,
    );
    assert_format(
        "component.vue",
        "<script setup lang=\"ts\">\n  function f(\n      a: A | B\n  ) {}\n</script>",
        "<script setup lang=\"ts\">\n  function f(\n      a:\n        | A\n        | B\n  ) {}\n</script>",
        &config,
    );
}

#[test]
fn cascades_type_layouts_calls_objects_and_statement_boundaries() {
    let mut config = FormatConfig::default();
    config.rules.union_layout = UnionLayoutMode::Always;
    assert_format(
        "sample.ts",
        "type U = { a: A | B } | C",
        "type U =\n  | {\n    a:\n      | A\n      | B;\n  }\n  | C",
        &config,
    );
    config.rules.type_literal_layout = TypeLiteralLayoutRule::Threshold(1);
    let source = "function f(){before();use<A | B>();after();}";
    assert_format(
        "sample.ts",
        source,
        "function f(){\n  before()\n\n  use<\n    | A\n    | B>()\n\n  after()\n}",
        &config,
    );
    let source = "const x = { a: value as A | B, b: 1 }";
    assert_format(
        "sample.ts",
        source,
        "const x = {\n  a: value as\n    | A\n    | B,\n\n  b: 1\n}",
        &config,
    );
}

#[test]
fn preserves_multiline_variants_in_auto_mode() {
    let source = "type U = {\n    value: A\n} | B";
    assert_format(
        "sample.ts",
        source,
        "type U =\n  | {\n    value: A\n}\n  | B",
        &isolated(UnionLayoutMode::Auto),
    );
}

#[test]
fn preserves_comment_connections_text_and_protected_gaps() {
    let config = isolated(UnionLayoutMode::Always);
    for (source, expected) in [
        (
            "type U = A | /* next | variant */ B",
            "type U =\n  | A\n  | /* next | variant */ B",
        ),
        (
            "type U = A |\n // next\n B",
            "type U =\n  | A\n // next\n  | B",
        ),
        (
            "type U = /* next */ A | B",
            "type U =\n  | /* next */ A\n  | B",
        ),
        (
            "type U = A /* previous */ | B",
            "type U =\n  | A /* previous */\n  | B",
        ),
        (
            "type U = A // previous\n | B",
            "type U =\n  | A // previous\n  | B",
        ),
        (
            "type U = A\n\n\n // detached\n\n\n | B",
            "type U =\n  | A\n\n\n // detached\n\n\n  | B",
        ),
    ] {
        assert_format("sample.ts", source, expected, &config);
    }
}

#[test]
fn preserves_typescript_directive_target_lines_with_comment_spacing_on_and_off() {
    for enabled in [false, true] {
        let mut config = isolated(UnionLayoutMode::Always);
        config.rules.comment_spacing = enabled;
        for directive in ["@ts-ignore", "@ts-expect-error"] {
            for source in [
                format!("// {directive}\ntype U = A | B"),
                format!("/* {directive} */\ntype U = A | B"),
                format!("type U = A |\n// {directive}\n B | C"),
                format!("/* {directive} */ type U = A | B\nuse()"),
            ] {
                let expected = if enabled {
                    source.replace("|\n//", "|\n\n//")
                } else {
                    source.clone()
                };
                assert_format("sample.ts", &source, &expected, &config);
            }
        }
    }
}

#[test]
fn preserves_bom_eof_and_non_script_vue_content() {
    let mut config = isolated(UnionLayoutMode::Auto);
    config.line_width = 20;
    for final_newline in ["", "\n"] {
        let source = format!(
            "\u{feff}<template> intact </template>\n<script setup lang=\"ts\">\n    type U = A | B | C\n</script>\n<style> untouched </style>{final_newline}"
        );
        let expected = format!(
            "\u{feff}<template> intact </template>\n<script setup lang=\"ts\">\n    type U =\n      | A\n      | B\n      | C\n</script>\n<style> untouched </style>{final_newline}"
        );
        assert_format("component.vue", &source, &expected, &config);
    }
}

use std::path::Path;

use worsier_formatter::{
    FormatConfig, TypeLiteralLayoutMode, TypeLiteralLayoutRule, format_text, resolve_config,
};

fn isolated(threshold: u32) -> FormatConfig {
    serde_json::from_value(serde_json::json!({
        "rules": {
            "bracketSpacing": { "curly": "off", "square": "off" },
            "commentSpacing": false,
            "importLayout": false,
            "exportLayout": false,
            "classMemberSpacing": "off",
            "unionLayout": "off",
            "interfaceLayout": "off",
            "typeLiteralLayout": threshold,
            "objectPropertySpacing": false,
            "quoteStyle": "off",
            "statementSpacing": {
                "controlFlowStatements": "off", "imports": "off",
                "multilineCallStatements": "off", "singleLineCallStatements": "off",
                "returnStatements": "off", "typeAliases": "off", "variableDeclarations": "off"
            },
            "semicolons": { "statements": "off", "classMembers": "off", "typeMembers": "off" },
            "trailingCommas": "off"
        }
    }))
    .unwrap()
}

fn assert_format(file: &str, source: &str, expected: &str, config: &FormatConfig) {
    let config = resolve_config(config.clone()).unwrap();
    let newlines: &[&str] = if source.contains('\n') {
        &["\n", "\r\n"]
    } else {
        &["\n"]
    };
    for newline in newlines {
        let source = source.replace('\n', newline);
        let expected = expected.replace('\n', newline);
        let output = format_text(Path::new(file), &source, &config).unwrap();
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
fn expands_only_above_the_direct_member_threshold() {
    let source = "type Shape = { value: string; run(): void, }";
    let expanded = "type Shape = {\n  value: string;\n  run(): void,\n}";
    for threshold in [0, 1, 2, u32::MAX] {
        assert_format(
            "sample.ts",
            source,
            if threshold < 2 { expanded } else { source },
            &isolated(threshold),
        );
    }
    for source in [
        "type Empty = {}",
        "type Empty = { /* empty */ }",
        "type Small = {\n    value: string;\n}",
    ] {
        assert_format("sample.ts", source, source, &isolated(1));
    }
    let mut config = isolated(0);
    config.rules.type_literal_layout = TypeLiteralLayoutRule::Mode(TypeLiteralLayoutMode::Off);
    assert_format("sample.ts", source, source, &config);
}

#[test]
fn counts_every_type_member_kind_and_ignores_line_width() {
    let source = "type Shape = { value?: string; readonly count: number; run(): void; [key: string]: unknown; (): string; new (): Shape; }";
    let expected = "type Shape = {\n  value?: string;\n  readonly count: number;\n  run(): void;\n  [key: string]: unknown;\n  (): string;\n  new (): Shape;\n}";
    for line_width in [1, 1_000] {
        let mut config = isolated(5);
        config.line_width = line_width;
        assert_format("sample.ts", source, expected, &config);
        config.rules.type_literal_layout = TypeLiteralLayoutRule::Threshold(6);
        assert_format("sample.ts", source, source, &config);
    }
}

#[test]
fn preserves_existing_indentation_and_uses_neighboring_member_indents() {
    for (source, expected) in [
        (
            "type Shape = {\n    /** value */ value: string;\n}",
            "type Shape = {\n    /** value */ value: string;\n}",
        ),
        (
            "type Shape = {\n    first: string; second: number;\n  }",
            "type Shape = {\n    first: string;\n    second: number;\n  }",
        ),
        (
            "type Shape = { first: string;\n\tsecond: number; }",
            "type Shape = {\n\tfirst: string;\n\tsecond: number;\n}",
        ),
        (
            "  type Shape = { first: string; second: number; }",
            "  type Shape = {\n    first: string;\n    second: number;\n  }",
        ),
    ] {
        assert_format("sample.ts", source, expected, &isolated(0));
    }
}

#[test]
fn nests_type_bodies_without_forcing_a_below_threshold_parent() {
    assert_format(
        "sample.ts",
        "type Shape = { nested: { first: string; second: number; }; }",
        "type Shape = {\n  nested: {\n    first: string;\n    second: number;\n  };\n}",
        &isolated(0),
    );
    assert_format(
        "sample.ts",
        "type Shape = { nested: { first: string; second: number; }; }",
        "type Shape = { nested: {\n  first: string;\n  second: number;\n}; }",
        &isolated(1),
    );
}

#[test]
fn supports_annotations_parameters_returns_generics_and_definition_files() {
    for file in [
        "sample.ts",
        "sample.tsx",
        "sample.d.ts",
        "sample.mts",
        "sample.cts",
    ] {
        for (source, expected) in [
            (
                "declare const value: { id: string };",
                "declare const value: {\n  id: string\n};",
            ),
            (
                "declare function read(value: { id: string }): { name: string };",
                "declare function read(value: {\n  id: string\n}): {\n  name: string\n};",
            ),
            (
                "type Box = Array<{ id: string }>;",
                "type Box = Array<{\n  id: string\n}>;",
            ),
            (
                "type Result = { ok: true } | { error: string };",
                "type Result = {\n  ok: true\n} | {\n  error: string\n};",
            ),
            (
                "type Values<T> = { [K in keyof T]: { value: T[K] } };",
                "type Values<T> = { [K in keyof T]: {\n  value: T[K]\n} };",
            ),
        ] {
            assert_format(file, source, expected, &isolated(0));
        }
    }
}

#[test]
fn keeps_interfaces_and_runtime_objects_independent() {
    let source = "interface Shape { value: string; }\nconst value = { first: 1, second: 2 };\ntype Values<T> = { [K in keyof T]: T[K] };";
    assert_format("sample.ts", source, source, &isolated(0));
    let source = "interface Shape { nested: { value: string }; }";
    assert_format(
        "sample.ts",
        source,
        "interface Shape { nested: {\n  value: string\n}; }",
        &isolated(0),
    );
    let mut config = isolated(0);
    config.rules.interface_layout = worsier_formatter::InterfaceLayoutRule::Threshold(0);
    assert_format(
        "sample.ts",
        source,
        "interface Shape {\n  nested: {\n    value: string\n  };\n}",
        &config,
    );
}

#[test]
fn preserves_directive_scope_for_whole_aliases_members_and_nested_types() {
    for directive in ["@ts-ignore", "@ts-expect-error"] {
        for source in [
            format!(
                "// {directive}\ntype Shape = {{ first: Missing; nested: {{ second: Missing }} }};"
            ),
            format!(
                "type Shape = {{\n  // {directive}\n  first: Missing; nested: {{ second: Missing }};\n}}"
            ),
        ] {
            assert_format("sample.ts", &source, &source, &isolated(0));
        }
        let source = format!(
            "type Shape = {{ first: string;\n  // {directive}\n  second: Missing; third: Missing;\n  last: number; }}"
        );
        let expected = format!(
            "type Shape = {{\n  first: string;\n  // {directive}\n  second: Missing; third: Missing;\n  last: number;\n}}"
        );
        assert_format("sample.ts", &source, &expected, &isolated(0));
    }
}

#[test]
fn preserves_directives_at_multiline_type_boundaries() {
    for directive in ["@ts-ignore", "@ts-expect-error"] {
        for comment in [format!("// {directive}"), format!("/* {directive} */")] {
            for source in [
                format!(
                    "type Shape = {{\n  nested: {{\n    value: string\n  {comment}\n  }}; wrong: Missing;\n}}"
                ),
                format!("type Shape = {{\n  nested: {{\n    value: string\n  {comment}\n  }}; }}"),
                format!(
                    "type Shape = {{ nested: {{ value: string }}\n{comment}\n}}; const wrong: number = 'value';"
                ),
            ] {
                let expected = if source.starts_with("type Shape = { nested:") {
                    format!(
                        "type Shape = {{\n  nested: {{\n    value: string\n  }}\n{comment}\n}}; const wrong: number = 'value';"
                    )
                } else {
                    source.clone()
                };
                assert_format("sample.ts", &source, &expected, &isolated(0));
            }
        }
    }
}

#[test]
fn preserves_block_directives_on_their_final_comment_line() {
    for directive in ["@ts-ignore", "@ts-expect-error"] {
        for comment in [
            format!("/* {directive} */"),
            format!("/* {directive}*/"),
            format!("/** {directive} */"),
            format!("/* details\n * {directive} */"),
        ] {
            let source = format!(
                "{comment}\ntype Shape = {{ first: Missing; nested: {{ second: Missing }} }};"
            );
            assert_format("sample.ts", &source, &source, &isolated(0));
            let source =
                format!("type Shape = {{\n  {comment}\n  first: Missing; second: Missing;\n}}");
            assert_format("sample.ts", &source, &source, &isolated(0));
        }
        for comment in [
            format!("/* {directive}\n */"),
            format!("/* details {directive} */"),
        ] {
            let source = format!("{comment}\ntype Shape = {{ value: string }};");
            let expected = format!("{comment}\ntype Shape = {{\n  value: string\n}};");
            assert_format("sample.ts", &source, &expected, &isolated(0));
        }
    }
}

#[test]
fn preserves_attached_detached_and_dangling_comments() {
    let mut config = isolated(0);
    config.rules.comment_spacing = true;
    assert_format(
        "sample.ts",
        "type Shape = { first: string; // trailing\n  // second\n  second: number;\n\n  /* detached */\n\n\n  third: boolean;\n\n  // end\n}",
        "type Shape = {\n  first: string; // trailing\n\n  // second\n  second: number;\n\n  /* detached */\n\n\n  third: boolean;\n\n  // end\n}",
        &config,
    );
}

#[test]
fn applies_semicolons_to_the_final_layout_and_preserves_commas() {
    for (mode, expected) in [
        (
            "always",
            "type Shape = {\n  first: string;\n  second: number;\n}",
        ),
        (
            "asNeeded",
            "type Shape = {\n  first: string\n  second: number\n}",
        ),
        (
            "off",
            "type Shape = {\n  first: string;\n  second: number\n}",
        ),
    ] {
        let mut config = isolated(0);
        config.rules.semicolons.type_members =
            serde_json::from_value(serde_json::json!({"singleLine": "off", "multiline": mode}))
                .unwrap();
        assert_format(
            "sample.ts",
            "type Shape = { first: string; second: number }",
            expected,
            &config,
        );
    }
    assert_format(
        "sample.ts",
        "type Shape = { first: string, second: number, }",
        "type Shape = {\n  first: string,\n  second: number,\n}",
        &FormatConfig::default(),
    );
}

#[test]
fn cascades_into_object_properties_calls_and_inline_statement_lists() {
    let config = FormatConfig::default();
    assert_format(
        "sample.ts",
        "before()\nuse<{ id: string }>()\nafter()",
        "before()\n\nuse<{\n  id: string;\n}>()\n\nafter()",
        &config,
    );
    assert_format(
        "sample.ts",
        "const options = { first: 1, nested: value as { id: string }, last: 2 }",
        "const options = {\n  first: 1,\n\n  nested: value as {\n    id: string;\n  },\n\n  last: 2\n}",
        &config,
    );
    assert_format(
        "sample.ts",
        "function run() { type Shape = { value: string }; done(); }",
        "function run() {\n  type Shape = {\n    value: string\n  };\n  done();\n}",
        &isolated(0),
    );
}

#[test]
fn resolves_type_indents_from_final_parent_positions() {
    assert_format(
        "sample.ts",
        "function run() {\n  const options = { first: value as { id: string }, second: 2 }\n}",
        "function run() {\n  const options = {\n    first: value as {\n      id: string;\n    },\n\n    second: 2\n  }\n}",
        &FormatConfig::default(),
    );
    assert_format(
        "sample.ts",
        "  function run() { type Shape = { value: string }; done(); }",
        "  function run() {\n    type Shape = {\n      value: string\n    };\n    done();\n  }",
        &isolated(0),
    );
    assert_format(
        "sample.ts",
        "declare function run(\n    value: { id: string }\n): void;",
        "declare function run(\n    value: {\n      id: string\n    }\n): void;",
        &isolated(0),
    );
}

#[test]
fn preserves_bom_newlines_and_vue_content() {
    for suffix in ["", "\n"] {
        assert_format(
            "sample.ts",
            &format!("\u{feff}type Shape = {{ value: string }}{suffix}"),
            &format!("\u{feff}type Shape = {{\n  value: string;\n}}{suffix}"),
            &FormatConfig::default(),
        );
    }
    assert_format(
        "sample.vue",
        "<template><p>{{ value }}</p></template>\n<script setup lang=\"ts\">\n  type Shape = { value: string }\n</script>\n<style>p { color: red }</style>",
        "<template><p>{{ value }}</p></template>\n<script setup lang=\"ts\">\n  type Shape = {\n    value: string;\n  }\n</script>\n<style>p { color: red }</style>",
        &FormatConfig::default(),
    );
}

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
        ("let value: A | B", "let value: A | B"),
        (
            "declare function f(value: A | B): C | D;",
            "declare function f(value: A | B): C | D;",
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
    let source = "type First = A | B; type Second = C | D";
    config.line_width = 25;
    assert_format(
        "sample.ts",
        source,
        "type First = A | B;\ntype Second = C | D",
        &config,
    );
}

#[test]
fn keeps_class_layout_independent_from_union_layout() {
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
        "class C {\n  a: A | B;\n  b: C | D;\n}",
        &config,
    );
    config.line_width = 120;
    assert_format(
        "sample.ts",
        "class C {\n  a:\n    | A\n    | B;\n\n  b: C;\n}",
        "class C {\n  a:\n    | A\n    | B;\n\n  b: C;\n}",
        &config,
    );
}

#[test]
fn preserves_containing_line_indentation_when_parent_layout_is_disabled() {
    let config = isolated(UnionLayoutMode::Always);
    for (source, expected) in [
        (
            "function f(\n    a: A | B\n) {}",
            "function f(\n    a: A | B\n) {}",
        ),
        (
            "type U = {\n    value: A | B\n}",
            "type U = {\n    value:\n      | A\n      | B\n}",
        ),
        (
            "class C {\n    value: A | B\n}",
            "class C {\n    value: A | B\n}",
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
        "function f(\n    a: A | B\n) {}",
        &config,
    );
    assert_format(
        "component.vue",
        "<script setup lang=\"ts\">\n  function f(\n      a: A | B\n  ) {}\n</script>",
        "<script setup lang=\"ts\">\n  function f(\n      a: A | B\n  ) {}\n</script>",
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
        "function f(){\n  before()\n  use<A | B>()\n  after()\n}",
        &config,
    );
    let source = "const x = { a: value as A | B, b: 1 }";
    assert_format(
        "sample.ts",
        source,
        "const x = {\n  a: value as A | B,\n  b: 1\n}",
        &config,
    );
}

#[test]
fn preserves_union_layout_outside_type_and_interface_declarations() {
    for mode in [UnionLayoutMode::Auto, UnionLayoutMode::Always] {
        let mut config = isolated(mode);
        config.line_width = 1;
        for source in [
            "let value: A | B",
            "let value:\n  | A\n  | B",
            "declare function f(value: A | B): C | D;",
            "const f = (value: A | B): C | D => value",
            "function applyEquipmentPropertyAssignments(sources: EquipmentPropertySource[], targets: MappingProperty[], assignments: EquipmentPropertyAssignments): Record<number, boolean | string> | null { return null }",
            "function f():\n  | A\n  | B { throw 0 }",
            "use<A | B>()",
            "use<\n  | A\n  | B>()",
            "class C { value: A | B; run(value: A | B): C | D {} }",
            "const value = input as A | B",
            "const value = input satisfies A | B",
        ] {
            assert_format("sample.ts", source, source, &config);
        }
    }
}

#[test]
fn keeps_object_union_boundaries_inline_in_auto_mode() {
    let mut config = FormatConfig::default();
    let source =
        "type PackingListEntryCreateBody = { customName: string; } | { inventoryId: string; }";
    let expected = "type PackingListEntryCreateBody = {\n  customName: string;\n} | {\n  inventoryId: string;\n}";
    for width in [1, 120, 1_000] {
        config.line_width = width;
        assert_format("sample.ts", source, expected, &config);
        assert_format("sample.ts", expected, expected, &config);
    }
    assert_format(
        "sample.ts",
        "interface Request { body: { decision: 'publish'; makePrimary: boolean; } | { decision: 'reject'; rejectionReason: string; } }",
        "interface Request {\n  body: {\n    decision: 'publish';\n    makePrimary: boolean;\n  } | {\n    decision: 'reject';\n    rejectionReason: string;\n  };\n}",
        &config,
    );
    assert_format(
        "sample.ts",
        "type U =\n  | {\n    first: {\n      value: string;\n    };\n  }\n  | {\n    second: number;\n  }",
        "type U = {\n  first: {\n    value: string;\n  };\n} | {\n  second: number;\n}",
        &config,
    );
    let flat = "type U = ({ a: string }) | ({ b: number })";
    let mut config = isolated(UnionLayoutMode::Auto);
    config.line_width = 1;
    assert_format("sample.ts", flat, flat, &config);
    assert_format(
        "sample.ts",
        "namespace N { type U = { a: A } | { b: B }; }",
        "namespace N { type U = { a: A } | { b: B }; }",
        &config,
    );
    config.rules.union_layout = UnionLayoutMode::Always;
    assert_format(
        "sample.ts",
        "type U = { a: A } | { b: B }",
        "type U =\n  | { a: A }\n  | { b: B }",
        &config,
    );
    config.rules.union_layout = UnionLayoutMode::Off;
    let source = "type U =\n  | { a: A }\n  | { b: B }";
    assert_format("sample.ts", source, source, &config);
}

#[test]
fn object_chain_rebase_respects_comment_retained_boundaries() {
    let mut config = FormatConfig::default();
    config.rules.comment_spacing = false;
    for (source, expected) in [
        (
            "type U =\n  // first variant\n  | {\n    a: A;\n  }\n  | {\n    b: B;\n  }",
            "type U =\n  // first variant\n  | {\n    a: A;\n  } | {\n    b: B;\n  }",
        ),
        (
            "type U =\n  | {\n    a: A;\n  }\n  // second variant\n  | {\n    b: B;\n  }",
            "type U = {\n  a: A;\n}\n  // second variant\n  | {\n    b: B;\n  }",
        ),
    ] {
        assert_format("sample.ts", source, expected, &config);
    }
}

#[test]
fn object_chain_rebase_is_independent_from_type_body_expansion() {
    let source = "type U =\n  | {\n    a: A;\n  }\n  | {\n    b: B;\n  }";
    let expected = "type U = {\n  a: A;\n} | {\n  b: B;\n}";
    for rule in [
        TypeLiteralLayoutRule::Threshold(1),
        TypeLiteralLayoutRule::Threshold(u32::MAX),
        TypeLiteralLayoutRule::Mode(worsier_formatter::TypeLiteralLayoutMode::Off),
    ] {
        let mut config = isolated(UnionLayoutMode::Auto);
        config.rules.type_literal_layout = rule;
        assert_format("sample.ts", source, expected, &config);
    }
    let config = isolated(UnionLayoutMode::Auto);
    assert_format(
        "sample.ts",
        "type U =\n  | {\n    nested: {\n      value: A;\n    };\n  }\n  | {\n    b: B;\n  }",
        "type U = {\n  nested: {\n    value: A;\n  };\n} | {\n  b: B;\n}",
        &config,
    );
    assert_format(
        "sample.ts",
        "type U =\n  | {\n    text: `first\n    keep text\n  last`;\n    /* first\n     * keep comment\n     */\n    a: A;\n  }\n  | {\n    b: B;\n  }",
        "type U = {\n  text: `first\n    keep text\n  last`;\n  /* first\n     * keep comment\n     */\n  a: A;\n} | {\n  b: B;\n}",
        &config,
    );
    assert_format(
        "sample.ts",
        "type U =\n  | {\n    nested:\n      | {\n        a: A;\n      }\n      | {\n        b: B;\n      };\n  }\n  | {\n    c: C;\n  }",
        "type U = {\n  nested: {\n    a: A;\n  } | {\n    b: B;\n  };\n} | {\n  c: C;\n}",
        &config,
    );
    assert_format(
        "sample.ts",
        "type U =\n  | {\n    nested:\n      // first inner variant\n      | {\n        a: A;\n      }\n      | {\n        b: B;\n      };\n  }\n  | {\n    c: C;\n  }",
        "type U = {\n  nested:\n    // first inner variant\n    | {\n      a: A;\n    } | {\n      b: B;\n    };\n} | {\n  c: C;\n}",
        &config,
    );
    assert_format(
        "component.vue",
        "<template>keep</template>\n<script setup lang=\"ts\">\n    type U =\n      | {\n        a: A;\n      }\n      | {\n        b: B;\n      }\n</script>\n<style>keep</style>",
        "<template>keep</template>\n<script setup lang=\"ts\">\n    type U = {\n      a: A;\n    } | {\n      b: B;\n    }\n</script>\n<style>keep</style>",
        &config,
    );
}

#[test]
fn formats_named_declarations_inside_functions_and_namespaces() {
    let mut config = isolated(UnionLayoutMode::Always);
    for (source, expected) in [
        (
            "function f(value: A | B) { type U = A | B; }",
            "function f(value: A | B) {\n  type U =\n    | A\n    | B;\n}",
        ),
        (
            "declare namespace N { export type U = A | B; }",
            "declare namespace N {\n  export type U =\n    | A\n    | B;\n}",
        ),
        (
            "interface I { run(value: A | B): C | D; }",
            "interface I { run(value:\n  | A\n  | B):\n  | C\n  | D; }",
        ),
    ] {
        assert_format("sample.ts", source, expected, &config);
    }
    config.rules.union_layout = UnionLayoutMode::Auto;
    assert_format(
        "sample.ts",
        "type U = { value: A } | null",
        "type U = { value: A } | null",
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
fn preserves_object_union_comments_and_directive_targets() {
    let mut config = isolated(UnionLayoutMode::Auto);
    for source in [
        "type U = /* first | object */ { a: A } | { b: B }",
        "type U = { a: A } /* previous */ | /* next */ { b: B }",
        "type U = { a: A } // previous\n  | { b: B }",
        "type U = { a: A }\n\n\n  // next\n\n\n  | { b: B }",
    ] {
        assert_format("sample.ts", source, source, &config);
    }
    for comment_spacing in [false, true] {
        config.rules.comment_spacing = comment_spacing;
        for directive in ["@ts-ignore", "@ts-expect-error"] {
            for source in [
                format!("// {directive}\ntype U =\n  | {{ a: A }}\n  | {{ b: B }}"),
                format!("type U = {{ a: A }} |\n// {directive}\n  {{ b: B }}"),
                format!("/* {directive} */ type U = {{ a: A }} | {{ b: B }}"),
            ] {
                let expected = if comment_spacing {
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
fn preserves_typescript_directive_target_lines_with_comment_spacing_on_and_off() {
    for enabled in [false, true] {
        let mut config = isolated(UnionLayoutMode::Always);
        config.rules.comment_spacing = enabled;
        for directive in ["@ts-ignore", "@ts-expect-error"] {
            for source in [
                format!("// {directive}\ntype U = A | B"),
                format!("// {directive}\ntype U =\n  | A\n  | B"),
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
    for (script, formatted_script) in [
        (
            "    type U = A | B | C",
            "    type U =\n      | A\n      | B\n      | C",
        ),
        (
            "    type U =\n      | { a: A }\n      | { b: B }\n    use<A | B>()",
            "    type U = { a: A } | { b: B }\n    use<A | B>()",
        ),
    ] {
        for final_newline in ["", "\n"] {
            let source = format!(
                "\u{feff}<template> intact </template>\n<script setup lang=\"ts\">\n{script}\n</script>\n<style> untouched </style>{final_newline}"
            );
            let expected = format!(
                "\u{feff}<template> intact </template>\n<script setup lang=\"ts\">\n{formatted_script}\n</script>\n<style> untouched </style>{final_newline}"
            );
            assert_format("component.vue", &source, &expected, &config);
        }
    }
}

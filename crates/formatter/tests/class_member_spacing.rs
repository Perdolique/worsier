use std::path::Path;

use worsier_formatter::{ClassMemberSpacingMode, FormatConfig, format_text, resolve_config};

fn isolated(mode: ClassMemberSpacingMode) -> FormatConfig {
    let mut config: FormatConfig = serde_json::from_value(serde_json::json!({
        "rules": {
            "bracketSpacing": { "curly": "off", "square": "off" },
            "commentSpacing": false, "importLayout": false, "exportLayout": false,
            "interfaceLayout": "off", "typeLiteralLayout": "off", "unionLayout": "off",
            "objectPropertySpacing": false, "quoteStyle": "off",
            "statementSpacing": {
                "controlFlowStatements": "off", "imports": "off",
                "multilineCallStatements": "off", "singleLineCallStatements": "off",
                "returnStatements": "off", "typeAliases": "off", "variableDeclarations": "off"
            },
            "semicolons": { "statements": "off", "classMembers": "off", "typeMembers": "off" },
            "trailingCommas": "off"
        }
    }))
    .unwrap();
    config.rules.class_member_spacing = mode;
    config
}

fn assert_format(file: &str, source: &str, expected: &str, config: &FormatConfig) {
    let config = resolve_config(config.clone()).unwrap();
    for newline in ["\n", "\r\n"] {
        for bom in ["", "\u{feff}"] {
            for ending in ["", newline] {
                let source = format!("{bom}{}{ending}", source.replace('\n', newline));
                let output_newline = if source.contains("\r\n") {
                    "\r\n"
                } else {
                    "\n"
                };
                let expected = format!("{bom}{}{ending}", expected.replace('\n', output_newline));
                let result = format_text(Path::new(file), &source, &config)
                    .unwrap_or_else(|error| panic!("{error}\nsource: {source}"));
                let output = result.as_deref().unwrap_or(&source);
                assert_eq!(output, expected, "source: {source:?}");
                let second = format_text(Path::new(file), output, &config).unwrap();
                assert!(
                    second.is_none(),
                    "second pass changed: {output:?} to {second:?}"
                );
            }
        }
    }
}

#[test]
fn modes_normalize_fields_methods_and_existing_blank_lines() {
    let source = "class Counter {\n  value = 0;\n\n\n  limit = 10;\n  increment() { this.value += 1 }\n  reset() {}\n}";
    for (mode, expected) in [
        (
            ClassMemberSpacingMode::Smart,
            "class Counter {\n  value = 0;\n  limit = 10;\n\n  increment() { this.value += 1 }\n\n  reset() {}\n}",
        ),
        (
            ClassMemberSpacingMode::Separate,
            "class Counter {\n  value = 0;\n\n  limit = 10;\n\n  increment() { this.value += 1 }\n\n  reset() {}\n}",
        ),
        (
            ClassMemberSpacingMode::Compact,
            "class Counter {\n  value = 0;\n  limit = 10;\n  increment() { this.value += 1 }\n  reset() {}\n}",
        ),
        (ClassMemberSpacingMode::Off, source),
    ] {
        for line_width in [1, 1_000] {
            let mut config = isolated(mode);
            config.line_width = line_width;
            assert_format("sample.ts", source, expected, &config);
        }
    }
}

#[test]
fn unfolds_inline_classes_and_preserves_existing_edge_gaps_and_indentation() {
    for mode in [
        ClassMemberSpacingMode::Smart,
        ClassMemberSpacingMode::Separate,
        ClassMemberSpacingMode::Compact,
    ] {
        let gap = if mode == ClassMemberSpacingMode::Separate {
            "\n\n"
        } else {
            "\n"
        };
        assert_format(
            "sample.js",
            "class C { a = 1; b = 2; }",
            &format!("class C {{\n  a = 1;{gap}  b = 2;\n}}"),
            &isolated(mode),
        );
        for source in [
            "class Empty {}",
            "class C { a = 1; }",
            "class C { method() {} }",
            "class C { /* empty */ }",
        ] {
            assert_format("sample.ts", source, source, &isolated(mode));
        }
    }
    assert_format(
        "sample.ts",
        "  const C = class { a=1;\n\t\tb=2; }",
        "  const C = class {\n\t\ta=1;\n\t\tb=2;\n  }",
        &isolated(ClassMemberSpacingMode::Smart),
    );
    let source = "class C {\n\n    a=1;\n\n\n    b=2;\n\n  }";
    let expected = "class C {\n\n    a=1;\n    b=2;\n\n  }";
    assert_format(
        "sample.ts",
        source,
        expected,
        &isolated(ClassMemberSpacingMode::Smart),
    );
}

#[test]
fn classifies_every_member_kind_without_rewriting_member_contents() {
    let source = "abstract class C {\n  static readonly a = 1;\n  #b = 2;\n  declare c: string;\n  abstract d: number;\n  [key: string]: unknown;\n  constructor() {}\n  get value() { return 1 }\n  set value(v: number) {}\n  accessor other = 1;\n  abstract accessor hidden: number;\n  abstract run(): void;\n  static {}\n}";
    let expected = "abstract class C {\n  static readonly a = 1;\n  #b = 2;\n  declare c: string;\n  abstract d: number;\n  [key: string]: unknown;\n\n  constructor() {}\n\n  get value() { return 1 }\n\n  set value(v: number) {}\n\n  accessor other = 1;\n\n  abstract accessor hidden: number;\n\n  abstract run(): void;\n\n  static {}\n}";
    assert_format(
        "sample.ts",
        source,
        expected,
        &isolated(ClassMemberSpacingMode::Smart),
    );
}

#[test]
fn keeps_overload_families_together_in_every_enabled_mode() {
    let source = "class C {\n  field = 1;\n  run(value: string): string;\n\n\n  'run'(value: number): number;\n\n  run(value: unknown) { return value }\n  static run(): void;\n  static run() {}\n  get run() { return 1 }\n}";
    for mode in [
        ClassMemberSpacingMode::Smart,
        ClassMemberSpacingMode::Separate,
        ClassMemberSpacingMode::Compact,
    ] {
        let gap = if mode == ClassMemberSpacingMode::Compact {
            "\n"
        } else {
            "\n\n"
        };
        let expected = format!(
            "class C {{\n  field = 1;{gap}  run(value: string): string;\n  'run'(value: number): number;\n  run(value: unknown) {{ return value }}{gap}  static run(): void;\n  static run() {{}}{gap}  get run() {{ return 1 }}\n}}"
        );
        assert_format("sample.ts", source, &expected, &isolated(mode));
    }
    assert_format(
        "sample.d.ts",
        "declare class C { run(v: string): string; run(v: number): number; other(): void; }",
        "declare class C {\n  run(v: string): string;\n  run(v: number): number;\n\n  other(): void;\n}",
        &isolated(ClassMemberSpacingMode::Separate),
    );
    assert_format(
        "sample.ts",
        "class C { constructor(v: string); constructor(v: number); constructor(v: unknown) {} }",
        "class C {\n  constructor(v: string);\n  constructor(v: number);\n  constructor(v: unknown) {}\n}",
        &isolated(ClassMemberSpacingMode::Separate),
    );
}

#[test]
fn preserves_comment_attachment_decorators_and_protected_gaps() {
    let mut config = isolated(ClassMemberSpacingMode::Compact);
    config.rules.comment_spacing = true;
    let source = "class C {\n  a=1; // trailing\n  /** next */\n  @decorate\n  b=2;\n\n\n  // detached\n\n\n  c=3;\n}";
    let expected = "class C {\n  a=1; // trailing\n\n  /** next */\n  @decorate\n  b=2;\n\n\n  // detached\n\n\n  c=3;\n}";
    assert_format("sample.ts", source, expected, &config);
    assert_format(
        "sample.ts",
        "class C {\n  a=1;\n  @decorate\n  b=2;\n  c=3;\n}",
        "class C {\n  a=1;\n\n  @decorate\n  b=2;\n\n  c=3;\n}",
        &isolated(ClassMemberSpacingMode::Smart),
    );
}

#[test]
fn preserves_typescript_directive_target_lines_and_directive_lines() {
    for source in [
        "// @ts-ignore\nclass C { a=1; b=2; }",
        "class C {\n  // @ts-expect-error\n  a=1; b=2;\n}",
        "class C {\n  /* @ts-ignore */ a=1; b=2;\n}",
        "class C {\n  a=1;\n  /* detail\n   * @ts-expect-error */\n  b=2; c=3;\n}",
    ] {
        assert_format(
            "sample.ts",
            source,
            source,
            &isolated(ClassMemberSpacingMode::Smart),
        );
    }
}

#[test]
fn fields_use_final_nested_layout_in_the_first_pass() {
    let mut config = isolated(ClassMemberSpacingMode::Smart);
    config.rules.object_property_spacing = true;
    let source = "class C { before=0; value={a:1,b:2}; after=3; }";
    let expected = "class C {\n  before=0;\n\n  value={\n    a:1,\n    b:2\n  };\n\n  after=3;\n}";
    assert_format("sample.ts", source, expected, &config);
    config.rules.type_literal_layout = worsier_formatter::TypeLiteralLayoutRule::Threshold(0);
    assert_format(
        "sample.ts",
        "class C { before=0; value: { a: string; b: number; }; after=3; }",
        "class C {\n  before=0;\n  value: { a: string; b: number; };\n  after=3;\n}",
        &config,
    );
}

#[test]
fn nested_class_expansion_reaches_outer_fields_objects_and_calls() {
    let source = "class Outer { before=0; nested=class { a=1; b=2; }; after=3; }";
    let expected =
        "class Outer {\n  before=0;\n\n  nested=class {\n    a=1;\n    b=2;\n  };\n\n  after=3;\n}";
    assert_format(
        "sample.ts",
        source,
        expected,
        &isolated(ClassMemberSpacingMode::Smart),
    );
    let mut config = isolated(ClassMemberSpacingMode::Smart);
    config.rules.object_property_spacing = true;
    assert_format(
        "sample.ts",
        "const object={a:class { x=1; y=2; },b:3};",
        "const object={\n  a:class {\n    x=1;\n    y=2;\n  },\n\n  b:3\n};",
        &config,
    );
    config.rules.statement_spacing.multiline_call_statements =
        worsier_formatter::StatementSpacingMode::Separate;
    assert_format(
        "sample.ts",
        "before(); use(class { x=1; y=2; }); after();",
        "before();\n\nuse(class {\n  x=1;\n  y=2;\n});\n\nafter();",
        &config,
    );
}

#[test]
fn method_body_layout_reaches_fields_in_the_same_pass() {
    let mut config = isolated(ClassMemberSpacingMode::Smart);
    config.rules.statement_spacing.single_line_call_statements =
        worsier_formatter::StatementSpacingMode::Compact.into();
    assert_format(
        "sample.ts",
        "class C { before=0; field=()=>{one();two();}; after=1; }",
        "class C {\n  before=0;\n\n  field=()=>{\n    one();\n    two();\n  };\n\n  after=1;\n}",
        &config,
    );
}

#[test]
fn semicolon_modes_remain_independent_and_computed_members_stay_safe() {
    let source = "class C { a = value; [key] = next; *method() {} }";
    for mode in [
        worsier_formatter::SemicolonMode::Off,
        worsier_formatter::SemicolonMode::Always,
        worsier_formatter::SemicolonMode::AsNeeded,
    ] {
        let mut config = isolated(ClassMemberSpacingMode::Smart);
        config.rules.semicolons.class_members = mode;
        let expected = if mode == worsier_formatter::SemicolonMode::AsNeeded {
            "class C {\n  a = value\n  ;[key] = next\n\n  ;*method() {}\n}"
        } else {
            "class C {\n  a = value;\n  [key] = next;\n\n  *method() {}\n}"
        };
        assert_format("sample.ts", source, expected, &config);
    }
    let mut config = isolated(ClassMemberSpacingMode::Smart);
    config.rules.semicolons.class_members = worsier_formatter::SemicolonMode::AsNeeded;
    let commented_guard = "class C {\n  a = value\n  /* next */ ;[key] = next\n}";
    assert_format("sample.ts", commented_guard, commented_guard, &config);
    assert_format(
        "sample.ts",
        "class C { a=1;;; b=2;; }",
        "class C {\n  a=1;;;\n  b=2;;\n}",
        &isolated(ClassMemberSpacingMode::Smart),
    );
}

#[test]
fn vue_scripts_and_off_mode_keep_other_rules_independent() {
    let config = isolated(ClassMemberSpacingMode::Smart);
    let source = "<template><p>same</p></template>\n<script setup lang=\"ts\">\n  class C { a=1; method() {} }\n</script>\n<style>p { color: red; }</style>";
    let expected = "<template><p>same</p></template>\n<script setup lang=\"ts\">\n  class C {\n    a=1;\n\n    method() {}\n  }\n</script>\n<style>p { color: red; }</style>";
    assert_format("sample.vue", source, expected, &config);
    let mut off = isolated(ClassMemberSpacingMode::Off);
    let source = "class C { a=\"one\"; b=\"two\"; }";
    assert_format("sample.ts", source, source, &off);
    off.rules.quote_style = worsier_formatter::QuoteStyle::Single;
    assert_format("sample.ts", source, "class C { a='one'; b='two'; }", &off);
}

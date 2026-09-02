use std::fmt::Write;
use std::path::Path;

use worsier_formatter::{
    FormatConfig, InterfaceLayoutMode, InterfaceLayoutRule, SemicolonMode, StatementSpacingMode,
    TrailingCommaMode, format_text, resolve_config,
};

fn comments_only() -> FormatConfig {
    let mut config = FormatConfig::default();
    config.rules.import_layout = false;
    config.rules.interface_layout = InterfaceLayoutRule::Mode(InterfaceLayoutMode::Off);
    config.rules.object_property_spacing = false;
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

fn format(file: &str, source: &str, config: &FormatConfig) -> String {
    let config = resolve_config(config.clone()).unwrap();
    format_text(Path::new(file), source, &config)
        .unwrap()
        .unwrap_or_else(|| source.to_owned())
}

fn assert_format(source: &str, expected: &str, config: &FormatConfig) {
    for newline in ["\n", "\r\n"] {
        let source = source.replace('\n', newline);
        let expected = expected.replace('\n', newline);
        let output = format("sample.ts", &source, config);
        assert_eq!(output, expected, "source: {source:?}");
        assert_eq!(
            format("sample.ts", &output, config),
            output,
            "second pass: {source:?}"
        );
    }
}

#[test]
fn separates_attached_groups_and_keeps_existing_blank_lines() {
    for config in [comments_only(), FormatConfig::default()] {
        for gap in ["\n", "\n\n", "\n\n\n"] {
            let source = format!("// first\nwoof(){gap}// second\nbark()");
            let before = if gap == "\n" { "\n\n" } else { gap };
            let expected = format!("// first\nwoof(){before}// second\nbark()");
            assert_format(&source, &expected, &config);
        }
        assert_format(
            "first() // trailing\n// next\n/* detail */\n/** doc */\nsecond()",
            "first() // trailing\n\n// next\n/* detail */\n/** doc */\nsecond()",
            &config,
        );
    }
}

#[test]
fn preserves_both_sides_of_detached_and_dangling_groups() {
    for config in [comments_only(), FormatConfig::default()] {
        for comment in ["// section", "/* section */", "/**\n * section\n */"] {
            for before in ["\n", "\n\n", "\n\n\n"] {
                let source = format!("first(){before}{comment}\n\n\nsecond()");
                assert_format(&source, &source, &config);
            }
        }
        for source in [
            "first()\n\n\n// footer",
            "// header\n\n\nfirst()",
            "first()\n// section\n\n\n// attached\nsecond()",
            "first()\n// group one\n\n// group two\n\nsecond()",
            "function f() {\n  first()\n\n\n  // end\n\n\n}",
        ] {
            assert_format(source, source, &config);
        }
    }
}

#[test]
fn covers_syntax_outside_statement_spacing() {
    for (source, expected) in [
        (
            "first = 1\n// second\nsecond = 2",
            "first = 1\n\n// second\nsecond = 2",
        ),
        (
            "class C {\n  first = 1\n  // second\n  second = 2\n}",
            "class C {\n  first = 1\n\n  // second\n  second = 2\n}",
        ),
        (
            "type T = {\n  first: number;\n  // second\n  second: string;\n}",
            "type T = {\n  first: number;\n\n  // second\n  second: string;\n}",
        ),
        (
            "interface T {\n  first: number;\n  // second\n  second: string;\n}",
            "interface T {\n  first: number;\n\n  // second\n  second: string;\n}",
        ),
        (
            "const x = {\n  first: 1,\n  // second\n  second: 2\n}",
            "const x = {\n  first: 1,\n\n  // second\n  second: 2\n}",
        ),
        (
            "const x = [\n  first,\n  // second\n  second\n]",
            "const x = [\n  first,\n\n  // second\n  second\n]",
        ),
        (
            "call(\n  first,\n  // second\n  second\n)",
            "call(\n  first,\n\n  // second\n  second\n)",
        ),
        (
            "function f(\n  first,\n  // second\n  second\n) {}",
            "function f(\n  first,\n\n  // second\n  second\n) {}",
        ),
        (
            "type T = [\n  string,\n  // second\n  number\n]",
            "type T = [\n  string,\n\n  // second\n  number\n]",
        ),
        (
            "enum E {\n  First,\n  // second\n  Second\n}",
            "enum E {\n  First,\n\n  // second\n  Second\n}",
        ),
        (
            "export {\n  first,\n  // second\n  second\n}",
            "export {\n  first,\n\n  // second\n  second\n}",
        ),
    ] {
        assert_format(source, expected, &comments_only());
    }
}

#[test]
fn leaves_first_elements_inline_comments_and_empty_containers_alone() {
    for source in [
        "// first\nfirst()",
        "// only comment\n/* and another */\n",
        "function f() {\n  // first\n  first()\n}",
        "function f() {\n  // empty\n}",
        "const x = [\n  // first\n  first\n]",
        "const x = {\n  // first\n  first: 1\n}",
        "class C {\n  // first\n  first = 1\n}",
        "call(\n  // first\n  first\n)",
        "function f(\n  // first\n  first\n) {}",
        "function f<\n  // first\n  T\n>() {}",
        "type T = Array<\n  // first\n  string\n>",
        "if (ready)\n  // body\n  first()",
        "if (ready) first(); else\n  // body\n  second()",
        "while (ready)\n  // body\n  first()",
        "for (;;)\n  // body\n  first()",
        "for (const x of xs)\n  // body\n  first()",
        "for (const x in xs)\n  // body\n  first()",
        "do\n  // body\n  first(); while (ready)",
        "switch (value) {\n  // first case\n  case 1:\n    // first statement\n    first()\n}",
        "first()\n/* next */ second()",
        "first()\n/* next\n * detail */ second()",
        "first() /* trailing */\nsecond()",
        "first(); /* trailing\n */ /* same line */\nsecond();",
        "first();\n/* same line */ /* leading\n */ second();",
        "const value = first + /* inline */ second",
        "const text = '// fake comment';\nconst template = `/* fake */`",
    ] {
        assert_format(source, source, &comments_only());
    }
}

#[test]
fn leaves_concise_arrow_body_comments_unpadded() {
    for config in [comments_only(), FormatConfig::default()] {
        for source in [
            "const f = () =>\n  // body\n  foo()",
            "const f = () =>\n  // body\n  (foo())",
            "const f = async () =>\n  // body\n  await foo()",
            "const f = () =>\n  // outer body\n  () =>\n    // inner body\n    foo()",
        ] {
            assert_format(source, source, &config);
        }
    }
}

#[test]
fn leaves_parenthesized_first_generic_arguments_unpadded() {
    for config in [comments_only(), FormatConfig::default()] {
        for source in [
            "type T = Array<\n  // first\n  (string)\n>",
            "type T = Array<\n  // first\n  ((string | number))\n>",
            "type T = Array<\n  // first\n  Array<\n    // nested first\n    (string)\n  >\n>",
            "type Box<\n  // first\n  T\n> = T",
        ] {
            assert_format(source, source, &config);
        }
        assert_format(
            "type T = Map<string,\n  // second\n  (number)\n>",
            "type T = Map<string,\n\n  // second\n  (number)\n>",
            &config,
        );
    }
}

#[test]
fn distinguishes_relational_operators_from_angle_delimiters() {
    for mut config in [comments_only(), FormatConfig::default()] {
        for operator in [">", "<"] {
            let source = format!("const x = a\n// compare\n{operator} b");
            let expected = format!("const x = a\n\n// compare\n{operator} b");
            assert_format(&source, &expected, &config);
        }
        for source in [
            "type T = Array<string\n  // footer\n>",
            "type T = Array<Array<string\n  // nested footer\n>>",
            "function f<T\n  // footer\n>() {}",
            "const x = <string\n  // footer\n>value",
            "const x = <(Array<string>)\n  // footer\n> (value)",
        ] {
            assert_format(source, source, &config);
        }
        config.rules.comment_spacing = false;
        let source = "const x = a\n// compare\n> b";
        assert_format(source, source, &config);
    }
}

#[test]
fn shares_policy_with_object_interface_and_import_layouts() {
    for (source, expected) in [
        (
            "const x = {\n  first: 1,\n  // second\n  second: 2\n}",
            "const x = {\n  first: 1,\n\n  // second\n  second: 2\n}",
        ),
        (
            "interface T {\n  first: number;\n  // second\n  second: string;\n}",
            "interface T {\n  first: number;\n\n  // second\n  second: string;\n}",
        ),
        (
            "import {\n  first,\n  /* second */\n  second\n} from 'x'",
            "import {\n  first,\n\n  /* second */\n  second\n} from 'x'",
        ),
        (
            "import {\n  // first\n  first,\n  // second\n  second\n} from 'x'",
            "import {\n  // first\n  first,\n\n  // second\n  second\n} from 'x'",
        ),
        (
            "import {\n  first,\n\n\n  /**\n   * section\n   */\n\n\n  second\n} from 'x'",
            "import {\n  first,\n\n\n  /**\n   * section\n   */\n\n\n  second\n} from 'x'",
        ),
        (
            "const x = {\n  first: 1,\n\n\n  // detached\n\n\n  second: 2\n}",
            "const x = {\n  first: 1,\n\n\n  // detached\n\n\n  second: 2\n}",
        ),
        (
            "interface T {\n  first: number;\n\n\n  // detached\n\n\n  second: string;\n}",
            "interface T {\n  first: number;\n\n\n  // detached\n\n\n  second: string;\n}",
        ),
    ] {
        assert_format(source, expected, &FormatConfig::default());
    }
}

#[test]
fn keeps_inline_import_comments_inline_when_imports_expand() {
    assert_format(
        "import { first, // trailing\nsecond } from 'x'",
        "import {\n  first, // trailing\n  second\n} from 'x'",
        &FormatConfig::default(),
    );
}

#[test]
fn keeps_multiline_inline_import_comments_attached_in_one_pass() {
    for comment in ["/* c\n * detail */", "/** c\n * detail */"] {
        let source = format!("import {{ a,\n{comment} b }} from 'long-module-name'");
        let expected = format!("import {{\n  a,\n  {comment} b\n}} from 'long-module-name'");
        let mut config = FormatConfig::default();
        assert_format(&source, &expected, &config);
        config.line_width = 20;
        assert_format(&source, &expected, &config);
    }

    let mut config = FormatConfig::default();
    config.rules.comment_spacing = false;
    // Disabling the rule preserves legacy import layout, including its indentation behavior.
    for newline in ["\n", "\r\n"] {
        let source =
            "import { a,\n/* c\n * detail */ b } from 'long-module-name'".replace('\n', newline);
        let expected = "import {\n  a,\n  /* c\n   * detail */\n  b\n} from 'long-module-name'"
            .replace('\n', newline);
        assert_eq!(format("sample.ts", &source, &config), expected);
    }
}

#[test]
fn keeps_comment_policy_stable_across_import_shapes() {
    for source in [
        "import {\n  // empty\n} from 'x'",
        "import {\n  first,\n  // footer\n} from 'x'",
        "import {\n  first,\n\n  // detached\n\n\n} from 'x'",
        "import { first, // trailing\n// second\nsecond } from 'x'",
        "import { first, /* trailing */ // still trailing\nsecond } from 'x'",
        "import { first, /* trailing\n * detail */ second } from 'x'",
        "import { /* first */ first, /* second */ second } from 'long-module-name'",
        "import {\n  first,\n  /* group */ /* detail */\n  second\n} from 'x'",
        "import {\n  first,\n  /* group */\n  /* detail */\n  second\n} from 'x'",
        "import\n// default\nvalue from 'x'",
        "import value from\n// source\n'x'",
    ] {
        let config = FormatConfig {
            line_width: 20,
            ..FormatConfig::default()
        };
        let output = format("sample.ts", source, &config);
        assert_eq!(format("sample.ts", &output, &config), output, "{source}");
    }
}

#[test]
fn formats_real_jsx_expressions_without_touching_jsx_comments_or_text() {
    let source = "const node = <Panel values={[\n  first,\n  // second\n  second\n]}>{/* JSX comment */}<span>// text</span></Panel>";
    let expected = source.replace("first,\n", "first,\n\n");
    for file in ["sample.jsx", "sample.tsx"] {
        let output = format(file, source, &comments_only());
        assert_eq!(output, expected);
        assert_eq!(format(file, &output, &comments_only()), output);
    }
}

#[test]
fn preserves_directives_bom_eof_and_embedded_scripts() {
    for directive in [
        "@ts-ignore",
        "@ts-expect-error",
        "eslint-disable-next-line no-console",
    ] {
        let source = format!("\u{feff}first()\n// {directive}\nsecond()");
        let expected = format!("\u{feff}first()\n\n// {directive}\nsecond()");
        assert_format(&source, &expected, &FormatConfig::default());
    }
    for file in ["sample.js", "sample.jsx", "sample.tsx"] {
        let source = "first()\n// second\nsecond()";
        assert_eq!(
            format(file, source, &comments_only()),
            "first()\n\n// second\nsecond()"
        );
    }
    assert_eq!(
        format(
            "sample.d.ts",
            "declare const first: string;\n// second\ndeclare const second: string;",
            &comments_only()
        ),
        "declare const first: string;\n\n// second\ndeclare const second: string;"
    );
    let source = "<template><!-- keep --><div /></template>\n<script setup lang=\"ts\">\nfirst()\n// second\nsecond()\n</script>\n<style>/* keep */</style>";
    let expected = source.replace("first()\n//", "first()\n\n//");
    let output = format("sample.vue", source, &comments_only());
    assert_eq!(output, expected);
    assert_eq!(format("sample.vue", &output, &comments_only()), output);
}

#[test]
fn disabling_comment_spacing_restores_existing_compact_behavior() {
    let source = "first()\n\n// second\nsecond()\n\n\n// section\n\n\nthird()";
    let mut config = FormatConfig::default();
    config.rules.comment_spacing = false;
    assert_format(
        source,
        "first()\n// second\nsecond()\n// section\n\nthird()",
        &config,
    );
    config = comments_only();
    config.rules.comment_spacing = false;
    assert_format(source, source, &config);
}

#[test]
fn formats_large_comment_sequences_in_one_pass() {
    let mut source = String::from("first()\n");
    let mut expected = source.clone();
    for index in 0..4000 {
        writeln!(source, "// item {index}\nitem{index}()").unwrap();
        writeln!(expected, "\n// item {index}\nitem{index}()").unwrap();
    }
    assert_format(&source, &expected, &FormatConfig::default());
}

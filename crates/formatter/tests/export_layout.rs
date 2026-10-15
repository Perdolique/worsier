use std::path::Path;

use worsier_formatter::{
    BracketSpacingMode, FormatConfig, QuoteStyle, SemicolonMode, StatementSpacingMode,
    TrailingCommaMode, format_text, resolve_config,
};

fn isolated() -> FormatConfig {
    serde_json::from_value(serde_json::json!({
        "rules": {
            "bracketSpacing": { "curly": "off", "square": "off" },
            "commentSpacing": false,
            "importLayout": false,
            "exportLayout": true,
            "interfaceLayout": "off",
            "typeLiteralLayout": "off",
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
fn expands_and_collapses_at_the_inclusive_width_boundary() {
    let source = "export{alpha,beta}from'pkg'";
    let flat = "export { alpha, beta } from 'pkg'";
    let expanded = "export {\n  alpha,\n  beta\n} from 'pkg'";
    let mut config = isolated();
    config.line_width = u32::try_from(flat.chars().count()).unwrap();
    assert_format("sample.ts", source, flat, &config);
    assert_format("sample.ts", expanded, flat, &config);
    config.line_width -= 1;
    assert_format("sample.ts", source, expanded, &config);
    assert_format("sample.ts", flat, expanded, &config);
}

#[test]
fn covers_local_reexport_type_alias_and_string_named_forms() {
    for (source, flat, expanded) in [
        (
            "export{alpha,beta}",
            "export { alpha, beta }",
            "export {\n  alpha,\n  beta\n}",
        ),
        (
            "export type{Alpha,Beta}from'pkg'",
            "export type { Alpha, Beta } from 'pkg'",
            "export type {\n  Alpha,\n  Beta\n} from 'pkg'",
        ),
        (
            "export type{Alpha,Beta}",
            "export type { Alpha, Beta }",
            "export type {\n  Alpha,\n  Beta\n}",
        ),
        (
            "export{type Alpha,alpha as renamed}from'pkg'",
            "export { type Alpha, alpha as renamed } from 'pkg'",
            "export {\n  type Alpha,\n  alpha as renamed\n} from 'pkg'",
        ),
        (
            "export{alpha as 'a-b',beta as 'c-d'}",
            "export { alpha as 'a-b', beta as 'c-d' }",
            "export {\n  alpha as 'a-b',\n  beta as 'c-d'\n}",
        ),
        (
            "export{'a-b' as alpha,'c-d' as beta}from'pkg'",
            "export { 'a-b' as alpha, 'c-d' as beta } from 'pkg'",
            "export {\n  'a-b' as alpha,\n  'c-d' as beta\n} from 'pkg'",
        ),
        (
            "export{alpha}from'a-very-long-package-name'",
            "export { alpha } from 'a-very-long-package-name'",
            "export {\n  alpha\n} from 'a-very-long-package-name'",
        ),
    ] {
        let mut config = isolated();
        assert_format("sample.ts", source, flat, &config);
        config.line_width = 1;
        assert_format("sample.ts", source, expanded, &config);
    }
}

#[test]
fn formats_named_exports_in_supported_script_kinds() {
    for file in [
        "sample.js",
        "sample.mjs",
        "sample.jsx",
        "sample.ts",
        "sample.mts",
        "sample.tsx",
    ] {
        assert_format(
            file,
            "export{alpha,beta}",
            "export { alpha, beta }",
            &isolated(),
        );
    }
}

#[test]
fn leaves_empty_wildcard_default_and_exported_declarations_outside_layout() {
    let mut config = isolated();
    config.line_width = 1;
    for source in [
        "export{}",
        "export{ }from'pkg'",
        "export*from'pkg'",
        "export*as namespace from'pkg'",
        "export default {one:1,two:2}",
        "export const options={one:1,two:2}",
        "export interface Example{one:string;two:number}",
        "export type Example={one:string;two:number}",
    ] {
        assert_format("sample.ts", source, source, &config);
    }
}

#[test]
fn disabling_export_layout_preserves_layout_without_disabling_other_rules() {
    let source = "export{\n alpha,\n beta,\n}from\"pkg\";";
    let mut config = isolated();
    config.rules.export_layout = false;
    config.rules.class_member_spacing = worsier_formatter::ClassMemberSpacingMode::Off;
    assert_format("sample.ts", source, source, &config);
    config.rules.bracket_spacing.curly = BracketSpacingMode::Always;
    config.rules.quote_style = QuoteStyle::Single;
    config.rules.trailing_commas = TrailingCommaMode::Never;
    config.rules.semicolons.statements = SemicolonMode::AsNeeded;
    assert_format(
        "sample.ts",
        source,
        "export{\n alpha,\n beta\n}from'pkg'",
        &config,
    );
    assert_format(
        "sample.ts",
        "export{alpha,beta}from\"pkg\";",
        "export{ alpha,beta }from'pkg'",
        &config,
    );
}

#[test]
fn width_uses_final_quotes_braces_commas_attributes_and_semicolons() {
    for mode in [
        SemicolonMode::Off,
        SemicolonMode::Always,
        SemicolonMode::AsNeeded,
    ] {
        for curly in [
            BracketSpacingMode::Always,
            BracketSpacingMode::Never,
            BracketSpacingMode::Off,
        ] {
            let mut config = isolated();
            config.rules.semicolons.statements = mode;
            config.rules.quote_style = QuoteStyle::Single;
            config.rules.trailing_commas = TrailingCommaMode::Never;
            config.rules.bracket_spacing.curly = curly;
            let inside = if curly == BracketSpacingMode::Never {
                "alpha, beta"
            } else {
                " alpha, beta "
            };
            let attributes = if curly == BracketSpacingMode::Never {
                "{type: 'json'}"
            } else {
                "{ type: 'json' }"
            };
            let semicolon = if mode == SemicolonMode::AsNeeded {
                ""
            } else {
                ";"
            };
            let flat = format!("export {{{inside}}} from 'don\\'t' with {attributes}{semicolon}");
            let expanded = format!(
                "export {{\n  alpha,\n  beta\n}} from 'don\\'t' with {attributes}{semicolon}"
            );
            config.line_width = u32::try_from(flat.chars().count()).unwrap();
            let source = "export{alpha,beta,}from\"don't\"with{type:\"json\",};";
            assert_format("sample.ts", source, &flat, &config);
            config.line_width -= 1;
            assert_format("sample.ts", source, &expanded, &config);
        }
    }
    let mut config = isolated();
    config.rules.semicolons.statements = SemicolonMode::Always;
    config.line_width = 21;
    assert_format(
        "sample.ts",
        "export{alpha,beta}",
        "export {\n  alpha,\n  beta\n};",
        &config,
    );
}

#[test]
fn width_uses_final_always_commas_in_the_named_list_and_attributes() {
    let mut config = isolated();
    config.rules.trailing_commas = TrailingCommaMode::Always;
    for (source, expected) in [
        ("export{alpha,beta,}", "export { alpha, beta }"),
        (
            "export{alpha,beta,}from'pkg'with{type:'json',}",
            "export { alpha, beta } from 'pkg' with { type: 'json' }",
        ),
    ] {
        config.line_width = u32::try_from(expected.chars().count()).unwrap();
        assert_format("sample.ts", source, expected, &config);
    }
}

#[test]
fn preserves_trailing_comments_and_multiline_comment_text_with_comment_spacing_off() {
    let mut config = isolated();
    config.line_width = 1;
    for (source, expected) in [
        (
            "export { alpha, // alpha\n beta } from 'pkg'",
            "export {\n  alpha, // alpha\n  beta\n} from 'pkg'",
        ),
        (
            "export{/* multiline\ncomment */alpha,beta}from'pkg'",
            "export {\n  /* multiline\ncomment */ alpha,\n  beta\n} from 'pkg'",
        ),
    ] {
        assert_format("sample.ts", source, expected, &config);
    }
}

#[test]
fn trailing_comma_modes_follow_the_final_list_layout() {
    for (mode, flat, expanded) in [
        (
            TrailingCommaMode::Off,
            "export { alpha, beta, }",
            "export {\n  alpha,\n  beta,\n}",
        ),
        (
            TrailingCommaMode::Never,
            "export { alpha, beta }",
            "export {\n  alpha,\n  beta\n}",
        ),
        (
            TrailingCommaMode::Always,
            "export { alpha, beta }",
            "export {\n  alpha,\n  beta,\n}",
        ),
    ] {
        let mut config = isolated();
        config.rules.trailing_commas = mode;
        assert_format("sample.ts", "export{alpha,beta,}", flat, &config);
        config.line_width = 1;
        assert_format("sample.ts", "export{alpha,beta,}", expanded, &config);
        if mode == TrailingCommaMode::Always {
            assert_format("sample.ts", "export{alpha,beta}", expanded, &config);
        }
    }
}

#[test]
fn preserves_comment_connections_and_protected_gaps() {
    let mut config = isolated();
    config.rules.comment_spacing = true;
    for (source, expected) in [
        (
            "export{/* a */alpha,/* b */beta}from'pkg'",
            "export { /* a */ alpha, /* b */ beta } from 'pkg'",
        ),
        (
            "export {\n  // alpha\n  alpha,\n  // beta\n  beta\n} from 'pkg'",
            "export {\n  // alpha\n  alpha,\n\n  // beta\n  beta\n} from 'pkg'",
        ),
        (
            "export {\n  alpha,\n\n\n  // detached\n\n  beta\n\n\n} from 'pkg'",
            "export {\n  alpha,\n\n\n  // detached\n\n  beta\n} from 'pkg'",
        ),
        (
            "export { alpha, // alpha\n beta } from 'pkg'",
            "export {\n  alpha, // alpha\n  beta\n} from 'pkg'",
        ),
        (
            "export {\n  alpha,\n\n  // dangling\n\n\n} from 'pkg'",
            "export {\n  alpha,\n\n  // dangling\n\n\n} from 'pkg'",
        ),
    ] {
        assert_format("sample.ts", source, expected, &config);
    }
    config.line_width = 1;
    assert_format(
        "sample.ts",
        "export{/* multiline\ncomment */alpha,beta}from'pkg'",
        "export {\n  /* multiline\ncomment */ alpha,\n  beta\n} from 'pkg'",
        &config,
    );
}

#[test]
fn preserves_directive_target_lines_even_with_comment_spacing_disabled() {
    let mut config = isolated();
    config.line_width = 1;
    for comment_spacing in [false, true] {
        config.rules.comment_spacing = comment_spacing;
        for source in [
            "// @ts-ignore\nexport{alpha,beta}from'pkg'",
            "/* @ts-expect-error */\nexport{alpha,beta}",
            "/* @ts-ignore */ export{alpha,beta}",
            "/* @ts-expect-error */ export{alpha,beta}\nrun()",
            "/* multiline\n * @ts-expect-error */\nexport{alpha,beta}",
            "export {\n  // @ts-ignore\n  alpha, beta,\n  gamma\n}",
            "// @ts-expect-error\nexport{alpha}; export{beta,gamma}",
        ] {
            assert_format("sample.ts", source, source, &config);
        }
    }
    config.rules.comment_spacing = false;
    config.rules.trailing_commas = TrailingCommaMode::Never;
    assert_format(
        "sample.ts",
        "// @ts-ignore\nexport{alpha,beta,}",
        "// @ts-ignore\nexport{alpha,beta}",
        &config,
    );
}

#[test]
fn accounts_for_final_indentation_in_nested_typescript_modules() {
    let mut config = isolated();
    let flat = "declare module 'pkg' {\n  export { alpha, beta };\n}";
    config.line_width = 25;
    assert_format(
        "sample.d.ts",
        "declare module 'pkg' {\n  export{alpha,beta};\n}",
        flat,
        &config,
    );
    config.line_width = 24;
    assert_format(
        "sample.d.ts",
        flat,
        "declare module 'pkg' {\n  export {\n    alpha,\n    beta\n  };\n}",
        &config,
    );
    config.rules.statement_spacing.variable_declarations = StatementSpacingMode::Separate;
    let source = "namespace N { const alpha=1; export{alpha,beta}; const beta=2; }";
    let expected = "namespace N {\n  const alpha=1;\n\n  export {\n    alpha,\n    beta\n  };\n\n  const beta=2;\n}";
    assert_format("sample.ts", source, expected, &config);
    config.rules.semicolons.statements = SemicolonMode::AsNeeded;
    assert_format(
        "sample.ts",
        source,
        "namespace N {\n  const alpha=1\n\n  export { alpha, beta }\n\n  const beta=2\n}",
        &config,
    );
}

#[test]
fn preserves_bom_eof_shape_and_indented_vue_scripts() {
    let mut config = isolated();
    config.line_width = 24;
    for eof in ["", "\n"] {
        assert_format(
            "sample.ts",
            &format!("\u{feff}export{{alpha,beta}}{eof}"),
            &format!("\u{feff}export {{ alpha, beta }}{eof}"),
            &config,
        );
    }
    let source = "\u{feff}<template><div>unchanged</div></template>\n<script lang=\"ts\">\n  export{alpha,beta};\n</script>\n<style>.x{}</style>";
    let expected = "\u{feff}<template><div>unchanged</div></template>\n<script lang=\"ts\">\n  export {\n    alpha,\n    beta\n  };\n</script>\n<style>.x{}</style>";
    assert_format("sample.vue", source, expected, &config);
}

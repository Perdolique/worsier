use std::path::Path;

use worsier_formatter::{
    BracketSpacingMode, FormatConfig, InterfaceLayoutMode, InterfaceLayoutRule, QuoteStyle,
    SemicolonMode, StatementSpacingMode, TrailingCommaMode, format_text, resolve_config,
};

fn isolated(curly: BracketSpacingMode, square: BracketSpacingMode) -> FormatConfig {
    let mut config = FormatConfig::default();
    config.rules.bracket_spacing.curly = curly;
    config.rules.bracket_spacing.square = square;
    config.rules.comment_spacing = false;
    config.rules.import_layout = false;
    config.rules.interface_layout = InterfaceLayoutRule::Mode(InterfaceLayoutMode::Off);
    config.rules.object_property_spacing = false;
    config.rules.quote_style = QuoteStyle::Off;
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

fn assert_format(file: &str, source: &str, expected: &str, config: FormatConfig) {
    let config = resolve_config(config).unwrap();
    let output = format_text(Path::new(file), source, &config)
        .unwrap()
        .unwrap_or_else(|| source.to_owned());
    assert_eq!(output, expected);
    assert!(
        format_text(Path::new(file), &output, &config)
            .unwrap()
            .is_none()
    );
}

#[test]
fn defaults_cover_objects_arrays_patterns_and_types() {
    let source = "let {taskId}=receipt\nconst value={key: [ one, two ]}\nconst [ head ]=items\n({key}=value)\n([ head ]=items)\ntype Shape={name: string}\ntype Pair=[ string, number ]\ninterface Person {id: string}";
    let expected = "let { taskId }=receipt\nconst value={ key: [one, two] }\nconst [head]=items\n({ key }=value)\n([head]=items)\ntype Shape={ name: string }\ntype Pair=[string, number]\ninterface Person { id: string }";
    assert_format(
        "sample.ts",
        source,
        expected,
        isolated(BracketSpacingMode::Always, BracketSpacingMode::Never),
    );
}

#[test]
fn handles_mapped_types_sparse_arrays_and_empty_module_lists() {
    let source =
        "type Flags<T>={[K in keyof T]:T[K]}\nconst holes=[ , , ]\nimport{ }from'pkg'\nexport{  }";
    let expected =
        "type Flags<T>={ [K in keyof T]:T[K] }\nconst holes=[, ,]\nimport{}from'pkg'\nexport{}";
    assert_format(
        "sample.ts",
        source,
        expected,
        isolated(BracketSpacingMode::Always, BracketSpacingMode::Never),
    );
}

#[test]
fn never_and_always_reverse_the_defaults() {
    let source = "const value={ key:1 };const items=[one, two]";
    let expected = "const value={key:1};const items=[ one, two ]";
    assert_format(
        "sample.ts",
        source,
        expected,
        isolated(BracketSpacingMode::Never, BracketSpacingMode::Always),
    );
}

#[test]
fn off_leaves_existing_gaps_untouched() {
    let source = "const value={key:1};const items=[ one ]\nlet { taskId }=receipt";
    assert_format(
        "sample.ts",
        source,
        source,
        isolated(BracketSpacingMode::Off, BracketSpacingMode::Off),
    );
}

#[test]
fn handles_imports_exports_attributes_and_line_width() {
    let source = "import{value}from'pkg' with {type:'json'}\nexport{value}\nexport{value as renamed}from'pkg' with {type:'json'}";
    let expected = "import{ value }from'pkg' with { type:'json' }\nexport{ value }\nexport{ value as renamed }from'pkg' with { type:'json' }";
    assert_format(
        "sample.ts",
        source,
        expected,
        isolated(BracketSpacingMode::Always, BracketSpacingMode::Never),
    );

    let source = "import{one,two}from'pkg'";
    let mut never = isolated(BracketSpacingMode::Never, BracketSpacingMode::Off);
    never.rules.import_layout = true;
    never.line_width = 29;
    assert_format("sample.ts", source, "import {one, two} from 'pkg'", never);

    let mut attributes = isolated(BracketSpacingMode::Never, BracketSpacingMode::Off);
    attributes.rules.import_layout = true;
    assert_format(
        "sample.ts",
        "import{value}from'pkg' with {type:'json'}",
        "import {value} from 'pkg' with {type: 'json'}",
        attributes,
    );

    let mut always = isolated(BracketSpacingMode::Always, BracketSpacingMode::Off);
    always.rules.import_layout = true;
    always.line_width = 29;
    assert_format(
        "sample.ts",
        source,
        "import {\n  one,\n  two\n} from 'pkg'",
        always,
    );
}

#[test]
fn preserves_multiline_gaps_comments_and_line_endings() {
    let source = "\u{feff}const value={/* start */key:1/* end */}\r\nconst list=[ /* start */ item /* end */ ]\r\nconst multiline={\r\n  key:1\r\n}\r\nconst empty={  };const emptyList=[\t ]";
    let expected = "\u{feff}const value={ /* start */key:1/* end */ }\r\nconst list=[/* start */ item /* end */]\r\nconst multiline={\r\n  key:1\r\n}\r\nconst empty={};const emptyList=[]";
    assert_format(
        "sample.ts",
        source,
        expected,
        isolated(BracketSpacingMode::Always, BracketSpacingMode::Never),
    );
}

#[test]
fn composes_with_trailing_comma_removal_at_a_closing_brace() {
    let mut config = isolated(BracketSpacingMode::Always, BracketSpacingMode::Never);
    config.rules.trailing_commas = TrailingCommaMode::Never;
    assert_format(
        "sample.ts",
        "const value={key:1,};const list=[ item, ]",
        "const value={ key:1 };const list=[item]",
        config,
    );
}

#[test]
fn leaves_blocks_indexing_computed_keys_and_jsx_braces_alone() {
    let source = "if(ok){run()}\nconst found=items[ index ]\nconst {[ key ]:value}=source\nconst view=<div>{value}</div>";
    let expected = "if(ok){run()}\nconst found=items[ index ]\nconst { [ key ]:value }=source\nconst view=<div>{value}</div>";
    assert_format(
        "sample.tsx",
        source,
        expected,
        isolated(BracketSpacingMode::Always, BracketSpacingMode::Never),
    );
}

use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast::ast::{Comment, CommentKind, CommentPosition, ImportDeclaration, Program, Statement};
use oxc_parser::{Kind, ParseOptions, Parser, Token, config::TokensParserConfig};
use oxc_span::{ContentEq, GetSpan, SourceType, Span};

use crate::{FormatError, ResolvedConfig};

const BOM: char = '\u{feff}';

#[cfg(test)]
thread_local! {
    static SPAN_LOOKUP_COMPARISONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static CORRUPT_REWRITE_FOR_TEST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[inline]
fn span_lookup_comparison(result: bool) -> bool {
    #[cfg(test)]
    SPAN_LOOKUP_COMPARISONS.set(SPAN_LOOKUP_COMPARISONS.get() + 1);
    result
}

#[cfg(test)]
fn maybe_corrupt_rewrite_for_test(rewritten: &mut String) {
    CORRUPT_REWRITE_FOR_TEST.with(|corrupt| {
        if corrupt.replace(false) {
            rewritten.push_str("\nconst __worsier_verification_probe = true;");
        }
    });
}

#[cfg(not(test))]
fn maybe_corrupt_rewrite_for_test(_: &mut String) {}

/// Formats static import declarations in JavaScript, TypeScript, JSX, or TSX source text.
///
/// # Errors
///
/// Returns a [`FormatError`] when the source type is unsupported, parsing fails, or semantic
/// verification finds that an import rewrite changed the program AST.
pub fn format_text(
    file_name: &Path,
    source_text: &str,
    config: &ResolvedConfig,
) -> Result<Option<String>, FormatError> {
    let (bom, source) = source_text
        .strip_prefix(BOM)
        .map_or(("", source_text), |text| ("\u{feff}", text));
    let source_type = source_type(file_name)?;
    let allocator = Allocator::default();
    let parsed = parse_with_tokens(&allocator, source, source_type)?;

    if parsed.is_flow_language {
        return Err(FormatError::UnsupportedSource {
            message: "Flow is not supported".to_owned(),
        });
    }
    if !config.imports_enabled() {
        return Ok(None);
    }

    let newline = detect_newline(source);
    let edits = rewrite_edits(
        source,
        &parsed.program,
        &parsed.tokens,
        config.line_width(),
        newline,
    )?;
    if edits.is_empty() {
        return Ok(None);
    }

    let mut rewritten = apply_edits(source, &edits)?;
    maybe_corrupt_rewrite_for_test(&mut rewritten);
    if config.verify_ast() {
        verify(file_name, source_type, &parsed.program, &rewritten)?;
    }

    if bom.is_empty() {
        return Ok(Some(rewritten));
    }

    let mut output = String::with_capacity(bom.len() + rewritten.len());
    output.push_str(bom);
    output.push_str(&rewritten);
    Ok(Some(output))
}

fn source_type(path: &Path) -> Result<SourceType, FormatError> {
    let source_type =
        SourceType::from_path(path).map_err(|_| FormatError::unsupported_source(path))?;
    Ok(if source_type.is_javascript() {
        source_type.with_jsx(true)
    } else {
        source_type
    })
}

fn parse<'a>(
    allocator: &'a Allocator,
    source: &'a str,
    source_type: SourceType,
) -> Result<oxc_parser::ParserReturn<'a>, FormatError> {
    let parsed = Parser::new(allocator, source, source_type)
        .with_options(parse_options())
        .parse();
    parse_result(parsed)
}

fn parse_with_tokens<'a>(
    allocator: &'a Allocator,
    source: &'a str,
    source_type: SourceType,
) -> Result<oxc_parser::ParserReturn<'a>, FormatError> {
    // The token-producing parser can panic on malformed lexer input such as NUL. Run the
    // diagnostic parser first so invalid source is rejected before token collection begins.
    let preflight_allocator = Allocator::default();
    parse(&preflight_allocator, source, source_type)?;

    let parsed = Parser::new(allocator, source, source_type)
        .with_options(parse_options())
        .with_config(TokensParserConfig)
        .parse();
    parse_result(parsed)
}

const fn parse_options() -> ParseOptions {
    ParseOptions {
        preserve_parens: false,
        enable_ident_hashes: false,
        allow_return_outside_function: true,
        allow_v8_intrinsics: true,
    }
}

fn parse_result(
    parsed: oxc_parser::ParserReturn<'_>,
) -> Result<oxc_parser::ParserReturn<'_>, FormatError> {
    if parsed.diagnostics.is_empty() {
        Ok(parsed)
    } else {
        let diagnostics = parsed
            .diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        Err(FormatError::Parse { diagnostics })
    }
}

fn verify(
    file_name: &Path,
    source_type: SourceType,
    input: &Program<'_>,
    output: &str,
) -> Result<(), FormatError> {
    let allocator = Allocator::default();
    let parsed =
        parse(&allocator, output, source_type).map_err(|error| FormatError::Verification {
            message: format!("{}: output does not parse: {error}", file_name.display()),
        })?;

    if input.content_ne(&parsed.program) {
        return Err(FormatError::Verification {
            message: format!("{}: output AST differs from input AST", file_name.display()),
        });
    }

    Ok(())
}

#[derive(Debug)]
struct Edit {
    start: u32,
    end: u32,
    replacement: String,
}

#[derive(Clone, Copy, Debug)]
struct StatementShape {
    span: Span,
    import_multiline: Option<bool>,
}

fn rewrite_edits(
    source: &str,
    program: &Program<'_>,
    tokens: &[Token],
    line_width: u32,
    newline: &str,
) -> Result<Vec<Edit>, FormatError> {
    let mut edits = Vec::new();
    let mut statements = program
        .directives
        .iter()
        .map(|directive| StatementShape {
            span: directive.span(),
            import_multiline: None,
        })
        .collect::<Vec<_>>();

    for statement in &program.body {
        let span = statement.span();
        let import_multiline = if let Statement::ImportDeclaration(declaration) = statement {
            let declaration_tokens = tokens_in_span(tokens, span);
            let declaration_comments = comments_in_span(&program.comments, span);
            let formatted = format_import(
                declaration,
                source,
                declaration_tokens,
                declaration_comments,
                line_width,
                newline,
            )?;
            let original = source_slice(source, span)?;
            if formatted.text != original {
                edits.push(Edit {
                    start: span.start,
                    end: span.end,
                    replacement: formatted.text,
                });
            }
            Some(formatted.multiline)
        } else {
            None
        };
        statements.push(StatementShape {
            span,
            import_multiline,
        });
    }

    statements.sort_by_key(|statement| statement.span.start);
    for pair in statements.windows(2) {
        let [previous, next] = pair else {
            unreachable!("windows(2) always contains two statements")
        };
        if previous.import_multiline.is_none() && next.import_multiline.is_none() {
            continue;
        }
        let blank_line = match (previous.import_multiline, next.import_multiline) {
            (Some(false), Some(false)) => false,
            (None, None) => unreachable!("non-import boundaries were skipped"),
            _ => true,
        };
        let replacement = if blank_line {
            newline.repeat(2)
        } else {
            newline.to_owned()
        };
        let separator_span = Span::new(previous.span.end, next.span.start);
        let separator = source_slice(source, separator_span)?;
        let formatted_separator =
            format_boundary_separator(source, separator_span, &program.comments, &replacement)?;
        if separator != formatted_separator {
            edits.push(Edit {
                start: previous.span.end,
                end: next.span.start,
                replacement: formatted_separator,
            });
        }
    }

    edits.sort_by_key(|edit| (edit.start, edit.end));
    Ok(edits)
}

fn format_boundary_separator(
    source: &str,
    span: Span,
    comments: &[Comment],
    statement_separator: &str,
) -> Result<String, FormatError> {
    let boundary_comments = comments_in_span(comments, span);
    if boundary_comments.is_empty() {
        let separator = source_slice(source, span)?;
        return if separator.chars().all(char::is_whitespace) {
            Ok(statement_separator.to_owned())
        } else {
            Ok(separator.to_owned())
        };
    }

    let trailing_end = boundary_comments
        .iter()
        .rev()
        .find(|comment| comment.position == CommentPosition::Trailing)
        .map_or(span.start, |comment| comment.span.end);
    let leading_start = boundary_comments
        .iter()
        .find(|comment| comment.position == CommentPosition::Leading)
        .map_or(span.end, |comment| comment.span.start);
    if trailing_end > leading_start {
        return Ok(source_slice(source, span)?.to_owned());
    }

    let mut output = String::new();
    output.push_str(source_slice(source, Span::new(span.start, trailing_end))?);
    output.push_str(statement_separator);
    output.push_str(source_slice(source, Span::new(leading_start, span.end))?);
    Ok(output)
}

struct FormattedImport {
    text: String,
    multiline: bool,
}

fn format_import(
    declaration: &ImportDeclaration<'_>,
    source: &str,
    tokens: &[Token],
    comments: &[Comment],
    line_width: u32,
    newline: &str,
) -> Result<FormattedImport, FormatError> {
    let named_braces = named_braces(declaration, tokens);
    let text = if let Some((left_brace, right_brace)) = named_braces {
        let flat = format_named_import(
            declaration.span,
            left_brace,
            right_brace,
            source,
            tokens,
            comments,
            newline,
            false,
        )?;
        if contains_line_break(&flat) || flat.chars().count() > line_width as usize {
            format_named_import(
                declaration.span,
                left_brace,
                right_brace,
                source,
                tokens,
                comments,
                newline,
                true,
            )?
        } else {
            flat
        }
    } else {
        canonicalize_range(declaration.span, source, tokens, comments, newline, false)?.text
    };

    Ok(FormattedImport {
        multiline: contains_line_break(&text),
        text,
    })
}

fn named_braces(declaration: &ImportDeclaration<'_>, tokens: &[Token]) -> Option<(Span, Span)> {
    let specifiers = declaration.specifiers.as_ref()?;
    let has_named = specifiers.is_empty()
        || specifiers.iter().any(|specifier| {
            matches!(
                specifier,
                oxc_ast::ast::ImportDeclarationSpecifier::ImportSpecifier(_)
            )
        });
    if !has_named {
        return None;
    }

    let mut left_brace = None;
    for token in tokens.iter().filter(|token| {
        token.start() >= declaration.span.start && token.end() <= declaration.source.span.start
    }) {
        match token.kind() {
            Kind::LCurly if left_brace.is_none() => left_brace = Some(token.span()),
            Kind::RCurly if left_brace.is_some() => return Some((left_brace?, token.span())),
            _ => {}
        }
    }
    None
}

#[allow(
    clippy::too_many_arguments,
    reason = "the import formatter needs both source ranges and the parser's lexical evidence"
)]
fn format_named_import(
    declaration_span: Span,
    left_brace: Span,
    right_brace: Span,
    source: &str,
    tokens: &[Token],
    comments: &[Comment],
    newline: &str,
    multiline: bool,
) -> Result<String, FormatError> {
    let prefix = canonicalize_range(
        Span::new(declaration_span.start, left_brace.start),
        source,
        tokens,
        comments,
        newline,
        false,
    )?;
    let suffix = canonicalize_range(
        Span::new(right_brace.end, declaration_span.end),
        source,
        tokens,
        comments,
        newline,
        false,
    )?;
    let ranges = named_segments(left_brace.end, right_brace.start, tokens);
    let last_token_segment = ranges
        .iter()
        .rposition(|range| range_has_token(*range, tokens));
    let mut segments = Vec::new();
    for (index, range) in ranges.into_iter().enumerate() {
        let has_token = range_has_token(range, tokens);
        let add_comma = has_token && last_token_segment.is_some_and(|last| index < last);
        let segment = canonicalize_range(range, source, tokens, comments, newline, add_comma)?;
        if !segment.text.is_empty() {
            segments.push(segment);
        }
    }

    let mut output = prefix.text;
    push_separator_after(&mut output, prefix.ends_line_comment, newline);
    output.push('{');

    if segments.is_empty() {
        output.push('}');
    } else if multiline {
        output.push_str(newline);
        for segment in &segments {
            output.push_str(&indent_lines(&segment.text, "  "));
            output.push_str(newline);
        }
        output.push('}');
    } else {
        output.push(' ');
        for (index, segment) in segments.iter().enumerate() {
            if index > 0 {
                push_separator_after(&mut output, segments[index - 1].ends_line_comment, newline);
            }
            output.push_str(&segment.text);
        }
        if segments
            .last()
            .is_some_and(|segment| segment.ends_line_comment)
        {
            output.push_str(newline);
            output.push('}');
        } else {
            output.push_str(" }");
        }
    }

    if !suffix.text.is_empty() {
        push_separator_after(&mut output, false, newline);
        output.push_str(&suffix.text);
    }
    Ok(output)
}

fn named_segments(start: u32, end: u32, tokens: &[Token]) -> Vec<Span> {
    let mut ranges = Vec::new();
    let mut segment_start = start;
    for token in tokens
        .iter()
        .filter(|token| token.start() >= start && token.end() <= end)
    {
        if token.kind() == Kind::Comma {
            ranges.push(Span::new(segment_start, token.start()));
            segment_start = token.end();
        }
    }
    ranges.push(Span::new(segment_start, end));
    ranges
}

fn range_has_token(range: Span, tokens: &[Token]) -> bool {
    !tokens_in_span(tokens, range).is_empty()
}

fn tokens_in_span(tokens: &[Token], span: Span) -> &[Token] {
    let start = tokens.partition_point(|token| span_lookup_comparison(token.start() < span.start));
    let end = start
        + tokens[start..].partition_point(|token| {
            span_lookup_comparison(token.start() < span.end && token.end() <= span.end)
        });
    &tokens[start..end]
}

fn comments_in_span(comments: &[Comment], span: Span) -> &[Comment] {
    let start =
        comments.partition_point(|comment| span_lookup_comparison(comment.span.start < span.start));
    let end = start
        + comments[start..].partition_point(|comment| {
            span_lookup_comparison(comment.span.start < span.end && comment.span.end <= span.end)
        });
    &comments[start..end]
}

#[derive(Clone, Copy)]
enum LexicalKind {
    Token(Kind),
    LineComment,
    BlockComment,
}

struct LexicalItem<'a> {
    span: Span,
    text: &'a str,
    kind: LexicalKind,
}

struct CanonicalText {
    text: String,
    ends_line_comment: bool,
}

fn canonicalize_range(
    range: Span,
    source: &str,
    tokens: &[Token],
    comments: &[Comment],
    newline: &str,
    comma_after_last_token: bool,
) -> Result<CanonicalText, FormatError> {
    let tokens = tokens_in_span(tokens, range);
    let comments = comments_in_span(comments, range);
    let mut items = Vec::new();
    for token in tokens {
        items.push(LexicalItem {
            span: token.span(),
            text: source_slice(source, token.span())?,
            kind: LexicalKind::Token(token.kind()),
        });
    }
    for comment in comments {
        items.push(LexicalItem {
            span: comment.span,
            text: source_slice(source, comment.span)?,
            kind: match comment.kind {
                CommentKind::Line => LexicalKind::LineComment,
                CommentKind::SingleLineBlock | CommentKind::MultiLineBlock => {
                    LexicalKind::BlockComment
                }
            },
        });
    }
    items.sort_by_key(|item| item.span.start);

    let last_token = items
        .iter()
        .rposition(|item| matches!(item.kind, LexicalKind::Token(_)));
    let mut output = String::new();
    for (index, item) in items.iter().enumerate() {
        if let Some(previous) = index.checked_sub(1).and_then(|index| items.get(index)) {
            output.push_str(item_separator(previous, item, newline));
        }
        output.push_str(item.text);
        if comma_after_last_token && last_token == Some(index) {
            output.push(',');
        }
    }

    Ok(CanonicalText {
        ends_line_comment: items
            .last()
            .is_some_and(|item| matches!(item.kind, LexicalKind::LineComment)),
        text: output,
    })
}

fn item_separator<'a>(
    previous: &LexicalItem<'_>,
    current: &LexicalItem<'_>,
    newline: &'a str,
) -> &'a str {
    if matches!(previous.kind, LexicalKind::LineComment)
        || (matches!(previous.kind, LexicalKind::BlockComment)
            && contains_line_break(previous.text))
    {
        return newline;
    }

    match (previous.kind, current.kind) {
        (LexicalKind::Token(Kind::LCurly), LexicalKind::Token(Kind::RCurly))
        | (
            _,
            LexicalKind::Token(
                Kind::Comma | Kind::Semicolon | Kind::RParen | Kind::RBrack | Kind::Colon,
            ),
        ) => "",
        (_, LexicalKind::Token(Kind::RCurly)) => " ",
        (LexicalKind::Token(Kind::LParen | Kind::LBrack), _) => "",
        _ => " ",
    }
}

fn push_separator_after(output: &mut String, line_comment: bool, newline: &str) {
    if line_comment {
        output.push_str(newline);
    } else if !output.is_empty() {
        output.push(' ');
    }
}

fn indent_lines(text: &str, indent: &str) -> String {
    let mut output = String::with_capacity(text.len() + indent.len());
    output.push_str(indent);
    for character in text.chars() {
        output.push(character);
        if character == '\n' {
            output.push_str(indent);
        }
    }
    output
}

fn detect_newline(source: &str) -> &'static str {
    let bytes = source.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            return if index > 0 && bytes[index - 1] == b'\r' {
                "\r\n"
            } else {
                "\n"
            };
        }
    }
    "\n"
}

fn contains_line_break(text: &str) -> bool {
    text.contains(['\n', '\r'])
}

fn source_slice(source: &str, span: Span) -> Result<&str, FormatError> {
    source
        .get(span.start as usize..span.end as usize)
        .ok_or_else(|| FormatError::internal("parser span was not a valid source range"))
}

fn apply_edits(source: &str, edits: &[Edit]) -> Result<String, FormatError> {
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0;
    for edit in edits {
        let start = edit.start as usize;
        let end = edit.end as usize;
        if start < cursor || end < start {
            return Err(FormatError::internal("source rewrite edits overlapped"));
        }
        let unchanged = source
            .get(cursor..start)
            .ok_or_else(|| FormatError::internal("source rewrite edit was out of bounds"))?;
        output.push_str(unchanged);
        output.push_str(&edit.replacement);
        cursor = end;
    }
    let unchanged = source
        .get(cursor..)
        .ok_or_else(|| FormatError::internal("source rewrite edit was out of bounds"))?;
    output.push_str(unchanged);
    Ok(output)
}

#[cfg(feature = "benchmarking")]
/// Parses benchmark input without rewriting it.
///
/// # Errors
///
/// Returns the same parse and source-type errors as [`format_text`].
pub fn benchmark_parse(file_name: &Path, source: &str) -> Result<(), FormatError> {
    let allocator = Allocator::default();
    parse(&allocator, source, source_type(file_name)?)?;
    Ok(())
}

#[cfg(feature = "benchmarking")]
/// Runs import rewriting with the supplied configuration.
///
/// # Errors
///
/// Returns the same errors as [`format_text`].
pub fn benchmark_rewrite(
    file_name: &Path,
    source: &str,
    config: &ResolvedConfig,
) -> Result<Option<String>, FormatError> {
    format_text(file_name, source, config)
}

#[cfg(feature = "benchmarking")]
/// Parses benchmark input and verifies it against a second parse.
///
/// # Errors
///
/// Returns the same parse, source-type, and verification errors as [`format_text`].
pub fn benchmark_verify(file_name: &Path, source: &str) -> Result<(), FormatError> {
    let source_type = source_type(file_name)?;
    let allocator = Allocator::default();
    let parsed = parse(&allocator, source, source_type)?;
    verify(file_name, source_type, &parsed.program, source)
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use std::path::Path;

    use oxc_allocator::Allocator;

    use super::{CORRUPT_REWRITE_FOR_TEST, SPAN_LOOKUP_COMPARISONS, parse, source_type, verify};
    use crate::{FormatConfig, RulesConfig, format_text, resolve_config};

    fn format(source: &str) -> String {
        format_with(source, FormatConfig::default())
    }

    fn format_with(source: &str, config: FormatConfig) -> String {
        let config = resolve_config(config).unwrap();
        format_text(Path::new("sample.ts"), source, &config)
            .unwrap()
            .unwrap_or_else(|| source.to_owned())
    }

    #[test]
    fn formats_static_import_families_without_reordering() {
        let source = "import{z as local,type A,b}from\"pkg\";\nimport type{Foo,Bar as Baz}from'x'\nimport value,*as space from\"ns\";\nimport\"side\";";
        let expected = "import { z as local, type A, b } from \"pkg\";\nimport type { Foo, Bar as Baz } from 'x'\nimport value, * as space from \"ns\";\nimport \"side\";";
        assert_eq!(format(source), expected);
    }

    #[test]
    fn formats_default_and_named_imports() {
        let source = "import React,{useState,type ComponentType as Type}from'react';";
        let flat = "import React, { useState, type ComponentType as Type } from 'react';";
        assert_eq!(format(source), flat);

        let multiline = format_with(
            source,
            FormatConfig {
                line_width: 30,
                ..FormatConfig::default()
            },
        );
        assert_eq!(
            multiline,
            "import React, {\n  useState,\n  type ComponentType as Type\n} from 'react';"
        );
        assert_eq!(
            format_with(
                &multiline,
                FormatConfig {
                    line_width: 30,
                    ..FormatConfig::default()
                }
            ),
            multiline
        );
    }

    #[test]
    fn keeps_the_closing_brace_outside_a_final_line_comment() {
        let source = "import { a // keep\n} from \"x\";";
        let expected = "import {\n  a // keep\n} from \"x\";";
        assert_eq!(format(source), expected);
        assert_eq!(format(expected), expected);

        let no_verify = FormatConfig {
            verify_ast: false,
            ..FormatConfig::default()
        };
        assert_eq!(format_with(source, no_verify), expected);
    }

    #[test]
    fn breaks_named_imports_one_specifier_per_line() {
        let source = "import { one } from 'a-very-long-package-name'";
        let output = format_with(
            source,
            FormatConfig {
                line_width: 20,
                ..FormatConfig::default()
            },
        );
        assert_eq!(output, "import {\n  one\n} from 'a-very-long-package-name'");
    }

    #[test]
    fn line_width_boundary_is_inclusive() {
        let source = "import{a,b}from'x'";
        let flat = "import { a, b } from 'x'";
        let flat_width = u32::try_from(flat.chars().count()).unwrap();
        assert_eq!(
            format_with(
                source,
                FormatConfig {
                    line_width: flat_width,
                    ..FormatConfig::default()
                },
            ),
            flat
        );
        assert_eq!(
            format_with(
                source,
                FormatConfig {
                    line_width: flat_width - 1,
                    ..FormatConfig::default()
                },
            ),
            "import {\n  a,\n  b\n} from 'x'"
        );
    }

    #[test]
    fn applies_the_import_spacing_matrix_in_both_directions() {
        let source = "const before={raw:true};\n\n\nimport a from'a'\n\nimport{one,two}from'long-package'\nimport{three,four}from'other-long-package'\n\nimport b from'b'\n\nconst after=[1,2];";
        let output = format_with(
            source,
            FormatConfig {
                line_width: 25,
                ..FormatConfig::default()
            },
        );
        assert_eq!(
            output,
            "const before={raw:true};\n\nimport a from 'a'\n\nimport {\n  one,\n  two\n} from 'long-package'\n\nimport {\n  three,\n  four\n} from 'other-long-package'\n\nimport b from 'b'\n\nconst after=[1,2];"
        );
    }

    #[test]
    fn preserves_every_non_import_byte_and_the_eof_shape() {
        let source = "import{a,b}from\"pkg\";\nconst odd={ untouched :true,nested:[1,  2] };\nexport{odd};\nconst quote=\"double\"";
        let output = format(source);
        assert_eq!(
            output,
            "import { a, b } from \"pkg\";\n\nconst odd={ untouched :true,nested:[1,  2] };\nexport{odd};\nconst quote=\"double\""
        );
        assert!(!output.ends_with('\n'));
    }

    #[test]
    fn preserves_bom_crlf_comments_attributes_and_semicolons() {
        let source = "\u{feff}import{/* first */type A, // second\r\nb as local,}from\"pkg\"with{type:\"json\"};\r\n\r\nconst value={raw:true};\r\n";
        let output = format(source);
        assert!(output.starts_with('\u{feff}'));
        assert_eq!(output.matches("/* first */").count(), 1);
        assert_eq!(output.matches("// second").count(), 1);
        assert!(
            output.contains("from \"pkg\" with { type: \"json\" };"),
            "{output:?}"
        );
        assert!(!output.replace("\r\n", "").contains('\n'));
        assert!(output.ends_with(";\r\n"));
    }

    #[test]
    fn leaves_default_namespace_side_effect_and_dynamic_imports_on_one_line() {
        let source = "import defaultValue from'a-package-name-that-is-long';\nimport*as values from'another-package-name-that-is-long';\nimport'side-effect-package-name-that-is-long';\nconst loaded=import('dynamic');";
        let output = format_with(
            source,
            FormatConfig {
                line_width: 10,
                ..FormatConfig::default()
            },
        );
        assert_eq!(
            output,
            "import defaultValue from 'a-package-name-that-is-long';\nimport * as values from 'another-package-name-that-is-long';\nimport 'side-effect-package-name-that-is-long';\n\nconst loaded=import('dynamic');"
        );
    }

    #[test]
    fn disabling_imports_preserves_the_complete_source() {
        let source = "import{a,b}from'x';const value={raw:true};";
        let config = resolve_config(FormatConfig {
            rules: RulesConfig { imports: false },
            ..FormatConfig::default()
        })
        .unwrap();
        assert!(
            format_text(Path::new("sample.ts"), source, &config)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn output_is_idempotent_and_ast_verified() {
        let source = "import{type A,b as c,d}from'x'assert{type:'json'};\nconst value={raw:true};";
        let output = format(source);
        let config = resolve_config(FormatConfig::default()).unwrap();
        assert!(
            format_text(Path::new("sample.ts"), &output, &config)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn verification_rejects_a_different_program() {
        let file_name = Path::new("sample.ts");
        let source_type = source_type(file_name).unwrap();
        let allocator = Allocator::default();
        let input = parse(&allocator, "import { original } from 'pkg';", source_type).unwrap();

        let error = verify(
            file_name,
            source_type,
            &input.program,
            "import { changed } from 'pkg';",
        )
        .unwrap_err();
        assert_eq!(error.code(), "VERIFICATION_ERROR");
    }

    #[test]
    fn format_text_runs_semantic_verification() {
        let config = resolve_config(FormatConfig::default()).unwrap();
        CORRUPT_REWRITE_FOR_TEST.set(true);

        let error = format_text(
            Path::new("sample.ts"),
            "import{original}from'pkg';",
            &config,
        )
        .unwrap_err();

        assert_eq!(error.code(), "VERIFICATION_ERROR");
    }

    #[test]
    fn import_span_lookups_stay_bounded_for_many_imports() {
        let import_count = 512;
        let mut source = String::new();
        for index in 0..import_count {
            writeln!(
                source,
                "import{{value{index},type Type{index}}}from'package-{index}';"
            )
            .unwrap();
        }
        let config = resolve_config(FormatConfig {
            verify_ast: false,
            ..FormatConfig::default()
        })
        .unwrap();

        SPAN_LOOKUP_COMPARISONS.set(0);
        format_text(Path::new("many-imports.ts"), &source, &config).unwrap();
        let comparisons = SPAN_LOOKUP_COMPARISONS.get();
        assert!(comparisons > 0);
        assert!(
            comparisons < import_count * 128,
            "span lookups performed {comparisons} comparisons for {import_count} imports"
        );
    }

    #[test]
    fn keeps_boundary_comments_attached_while_normalizing_import_spacing() {
        let source = "import a from'a'; // trailing\n// leading\nconst value={raw:true};";
        let output = format(source);
        assert_eq!(
            output,
            "import a from 'a'; // trailing\n\n// leading\nconst value={raw:true};"
        );
        assert_eq!(output.matches("// trailing").count(), 1);
        assert_eq!(output.matches("// leading").count(), 1);
    }

    #[test]
    fn separates_directives_from_following_imports() {
        assert_eq!(
            format("'use strict';import value from'pkg';"),
            "'use strict';\n\nimport value from 'pkg';"
        );
    }

    #[test]
    fn rejects_invalid_source_instead_of_copying_it() {
        let config = resolve_config(FormatConfig::default()).unwrap();
        let error = format_text(Path::new("sample.ts"), "const value = @;", &config).unwrap_err();
        assert_eq!(error.code(), "PARSE_ERROR");
    }

    #[test]
    fn rejects_token_lexer_crash_input_without_panicking() {
        let config = resolve_config(FormatConfig::default()).unwrap();
        let error = format_text(Path::new("sample.ts"), "\0", &config).unwrap_err();
        assert_eq!(error.code(), "PARSE_ERROR");
    }
}

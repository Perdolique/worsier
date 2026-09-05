use oxc_ast::ast::{JSXAttributeValue, Program, StringLiteral};
use oxc_ast_visit::{Visit, walk};

use crate::QuoteStyle;

use super::{Edit, FormatError, apply_edits, source_slice};

pub(super) fn rewrite(
    source: &str,
    program: &Program<'_>,
    style: QuoteStyle,
) -> Result<Option<String>, FormatError> {
    if style == QuoteStyle::Off {
        return Ok(None);
    }

    let mut collector = QuoteCollector {
        source,
        style,
        edits: Vec::new(),
        error: None,
    };
    collector.visit_program(program);
    if let Some(error) = collector.error {
        return Err(error);
    }
    collector.edits.sort_unstable_by_key(|edit| edit.start);
    collector.edits.dedup_by_key(|edit| (edit.start, edit.end));
    if collector.edits.is_empty() {
        Ok(None)
    } else {
        apply_edits(source, &collector.edits).map(Some)
    }
}

struct QuoteCollector<'s> {
    source: &'s str,
    style: QuoteStyle,
    edits: Vec<Edit>,
    error: Option<FormatError>,
}

impl QuoteCollector<'_> {
    fn record(&mut self, literal: &StringLiteral<'_>) {
        if self.error.is_some() {
            return;
        }
        match quote_replacement(self.source, literal.span, self.style) {
            Ok(Some(replacement)) => self.edits.push(Edit {
                start: literal.span.start,
                end: literal.span.end,
                replacement,
            }),
            Ok(None) => {}
            Err(error) => self.error = Some(error),
        }
    }
}

impl<'a> Visit<'a> for QuoteCollector<'_> {
    fn visit_string_literal(&mut self, literal: &StringLiteral<'a>) {
        self.record(literal);
        walk::walk_string_literal(self, literal);
    }

    fn visit_jsx_attribute_value(&mut self, value: &JSXAttributeValue<'a>) {
        if !matches!(value, JSXAttributeValue::StringLiteral(_)) {
            walk::walk_jsx_attribute_value(self, value);
        }
    }
}

pub(super) fn canonical_span(
    source: &str,
    span: oxc_span::Span,
    style: QuoteStyle,
) -> Result<String, FormatError> {
    match quote_replacement(source, span, style)? {
        Some(replacement) => Ok(replacement),
        None => source_slice(source, span).map(ToOwned::to_owned),
    }
}

fn quote_replacement(
    source: &str,
    span: oxc_span::Span,
    style: QuoteStyle,
) -> Result<Option<String>, FormatError> {
    let raw = source_slice(source, span)?;
    let bytes = raw.as_bytes();
    let Some(&original_quote @ (b'\'' | b'"')) = bytes.first() else {
        return Err(FormatError::internal(
            "string literal span did not start with a quote",
        ));
    };
    if bytes.last() != Some(&original_quote) || bytes.len() < 2 {
        return Err(FormatError::internal(
            "string literal span did not end with its opening quote",
        ));
    }
    let target_quote = match style {
        QuoteStyle::Single => b'\'',
        QuoteStyle::Double => b'"',
        QuoteStyle::Off => return Ok(None),
    };

    let body = &raw[1..raw.len() - 1];
    let body_bytes = body.as_bytes();
    let mut replacement = String::with_capacity(raw.len());
    replacement.push(char::from(target_quote));
    let mut cursor = 0;
    while cursor < body_bytes.len() {
        if body_bytes[cursor] != b'\\' {
            if body_bytes[cursor] == target_quote {
                replacement.push('\\');
                replacement.push(char::from(target_quote));
                cursor += 1;
                continue;
            }
            let character = body[cursor..]
                .chars()
                .next()
                .ok_or_else(|| FormatError::internal("invalid string literal boundary"))?;
            replacement.push(character);
            cursor += character.len_utf8();
            continue;
        }

        let slash_start = cursor;
        while cursor < body_bytes.len() && body_bytes[cursor] == b'\\' {
            cursor += 1;
        }
        let slash_count = cursor - slash_start;
        if cursor < body_bytes.len() && matches!(body_bytes[cursor], b'\'' | b'"') {
            let quote = body_bytes[cursor];
            let decoded_slashes = slash_count / 2;
            let output_slashes = decoded_slashes * 2 + usize::from(quote == target_quote);
            replacement.extend(std::iter::repeat_n('\\', output_slashes));
            replacement.push(char::from(quote));
            cursor += 1;
        } else {
            replacement.extend(std::iter::repeat_n('\\', slash_count));
        }
    }
    replacement.push(char::from(target_quote));

    if replacement == raw {
        Ok(None)
    } else {
        Ok(Some(replacement))
    }
}

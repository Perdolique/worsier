#[cfg(test)]
use std::cell::Cell;

use memchr::{memchr, memchr2, memmem};
use oxc_span::SourceType;

use crate::FormatError;
use crate::embedded::EmbeddedRegion;

const BOM: &[u8] = b"\xef\xbb\xbf";

#[cfg(test)]
thread_local! {
    static SCANNER_STEPS: Cell<usize> = const { Cell::new(0) };
}

#[inline]
fn record_steps(steps: usize) {
    #[cfg(test)]
    SCANNER_STEPS.set(SCANNER_STEPS.get().saturating_add(steps));
    #[cfg(not(test))]
    let _ = steps;
}

#[cfg(test)]
pub(crate) fn reset_scanner_steps() {
    SCANNER_STEPS.set(0);
}

#[cfg(test)]
pub(crate) fn scanner_steps() -> usize {
    SCANNER_STEPS.get()
}

#[derive(Debug)]
struct OpeningTag<'a> {
    name: &'a str,
    end: usize,
    self_closing: bool,
    attributes: Vec<Attribute<'a>>,
}

#[derive(Debug)]
struct Attribute<'a> {
    name: &'a str,
    value: Option<&'a str>,
}

pub(crate) fn embedded_regions(source: &str) -> Result<Vec<EmbeddedRegion>, FormatError> {
    let bytes = source.as_bytes();
    let mut regions = Vec::new();
    let mut cursor = usize::from(bytes.starts_with(BOM)) * BOM.len();

    while let Some(relative) = memchr(b'<', &bytes[cursor..]) {
        let start = cursor + relative;
        record_steps(relative + 1);
        if bytes[start..].starts_with(b"<!--") {
            cursor = comment_end(bytes, start)?;
            continue;
        }
        if bytes[start..].starts_with(b"</") {
            return Err(parse_error(format!(
                "unexpected top-level closing tag at byte {start}"
            )));
        }
        if !bytes
            .get(start + 1)
            .is_some_and(|byte| is_name_start(*byte))
        {
            cursor = start + 1;
            continue;
        }

        let tag = opening_tag(source, start)?;
        if tag.self_closing {
            cursor = tag.end;
            continue;
        }
        let content_start = tag.end;
        let (content_end, block_end) = block_end(source, &tag, content_start)?;
        if tag.name.eq_ignore_ascii_case("script")
            && !has_attribute(&tag.attributes, "src")
            && let Some((source_type, lang)) = script_source_type(&tag.attributes)
        {
            regions.push(EmbeddedRegion {
                range: content_start..content_end,
                source_type,
                label: script_label(&tag.attributes, lang),
            });
        }
        cursor = block_end;
    }
    record_steps(bytes.len().saturating_sub(cursor));
    Ok(regions)
}

fn block_end(
    source: &str,
    tag: &OpeningTag<'_>,
    content_start: usize,
) -> Result<(usize, usize), FormatError> {
    if tag.name.eq_ignore_ascii_case("template") {
        nested_template_end(source, content_start)
    } else {
        raw_block_end(source, tag.name, content_start)
    }
}

fn raw_block_end(
    source: &str,
    name: &str,
    mut cursor: usize,
) -> Result<(usize, usize), FormatError> {
    let bytes = source.as_bytes();
    while let Some(relative) = memchr(b'<', &bytes[cursor..]) {
        let start = cursor + relative;
        record_steps(relative + 1);
        if let Some(end) = closing_tag_end(source, start, name)? {
            return Ok((start, end));
        }
        cursor = start + 1;
    }
    record_steps(bytes.len().saturating_sub(cursor));
    Err(parse_error(format!("unclosed top-level <{name}> block")))
}

fn nested_template_end(source: &str, mut cursor: usize) -> Result<(usize, usize), FormatError> {
    let bytes = source.as_bytes();
    let mut depth = 1usize;
    while let Some(relative) = memchr2(b'<', b'{', &bytes[cursor..]) {
        let start = cursor + relative;
        record_steps(relative + 1);
        if bytes[start] == b'{' {
            cursor = if bytes.get(start + 1) == Some(&b'{') {
                interpolation_end(bytes, start)?
            } else {
                start + 1
            };
            continue;
        }
        if bytes[start..].starts_with(b"<!--") {
            cursor = comment_end(bytes, start)?;
            continue;
        }
        if let Some(end) = closing_tag_end(source, start, "template")? {
            depth -= 1;
            if depth == 0 {
                return Ok((start, end));
            }
            cursor = end;
            continue;
        }
        if bytes
            .get(start + 1)
            .is_some_and(|byte| is_name_start(*byte))
        {
            let nested = opening_tag(source, start)?;
            if nested.name.eq_ignore_ascii_case("template") && !nested.self_closing {
                depth += 1;
            }
            if nested.name.eq_ignore_ascii_case("script")
                || nested.name.eq_ignore_ascii_case("style")
            {
                if nested.self_closing {
                    cursor = nested.end;
                } else {
                    let (_, end) = raw_block_end(source, nested.name, nested.end)?;
                    cursor = end;
                }
            } else {
                cursor = nested.end;
            }
            continue;
        }
        cursor = start + 1;
    }
    record_steps(bytes.len().saturating_sub(cursor));
    Err(parse_error("unclosed top-level <template> block"))
}

fn interpolation_end(bytes: &[u8], start: usize) -> Result<usize, FormatError> {
    let content_start = start + 2;
    let relative = memmem::find(&bytes[content_start..], b"}}").ok_or_else(|| {
        record_steps(bytes.len().saturating_sub(start));
        parse_error(format!("unclosed Vue interpolation at byte {start}"))
    })?;
    let end = content_start + relative + 2;
    record_steps(end - start);
    Ok(end)
}

fn opening_tag(source: &str, start: usize) -> Result<OpeningTag<'_>, FormatError> {
    let bytes = source.as_bytes();
    let mut cursor = start + 1;
    let name_start = cursor;
    while bytes
        .get(cursor)
        .is_some_and(|byte| is_name_continue(*byte))
    {
        cursor += 1;
    }
    record_steps(cursor - start);
    let name = &source[name_start..cursor];
    let mut attributes = Vec::new();

    loop {
        skip_ascii_whitespace(bytes, &mut cursor);
        match bytes.get(cursor) {
            Some(b'>') => {
                record_steps(1);
                return Ok(OpeningTag {
                    name,
                    end: cursor + 1,
                    self_closing: false,
                    attributes,
                });
            }
            Some(b'/') => {
                cursor += 1;
                skip_ascii_whitespace(bytes, &mut cursor);
                if bytes.get(cursor) != Some(&b'>') {
                    return Err(parse_error(format!(
                        "malformed self-closing <{name}> tag at byte {start}"
                    )));
                }
                record_steps(2);
                return Ok(OpeningTag {
                    name,
                    end: cursor + 1,
                    self_closing: true,
                    attributes,
                });
            }
            None => {
                return Err(parse_error(format!(
                    "unclosed opening <{name}> tag at byte {start}"
                )));
            }
            Some(_) => {}
        }

        let attribute_start = cursor;
        while bytes
            .get(cursor)
            .is_some_and(|byte| !byte.is_ascii_whitespace() && !matches!(byte, b'=' | b'>' | b'/'))
        {
            cursor += 1;
        }
        if attribute_start == cursor {
            return Err(parse_error(format!(
                "malformed attribute in <{name}> at byte {cursor}"
            )));
        }
        let attribute_name = &source[attribute_start..cursor];
        skip_ascii_whitespace(bytes, &mut cursor);
        let value = if bytes.get(cursor) == Some(&b'=') {
            cursor += 1;
            skip_ascii_whitespace(bytes, &mut cursor);
            Some(attribute_value(source, name, start, &mut cursor)?)
        } else {
            None
        };
        record_steps(cursor - attribute_start);
        attributes.push(Attribute {
            name: attribute_name,
            value,
        });
    }
}

fn attribute_value<'a>(
    source: &'a str,
    tag_name: &str,
    tag_start: usize,
    cursor: &mut usize,
) -> Result<&'a str, FormatError> {
    let bytes = source.as_bytes();
    if let Some(quote @ (b'\'' | b'"')) = bytes.get(*cursor).copied() {
        *cursor += 1;
        let value_start = *cursor;
        let relative = memchr(quote, &bytes[*cursor..]).ok_or_else(|| {
            parse_error(format!(
                "unclosed quoted attribute in <{tag_name}> at byte {tag_start}"
            ))
        })?;
        *cursor += relative;
        let value = &source[value_start..*cursor];
        *cursor += 1;
        return Ok(value);
    }
    let value_start = *cursor;
    while bytes
        .get(*cursor)
        .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b'>')
    {
        *cursor += 1;
    }
    if value_start == *cursor {
        return Err(parse_error(format!(
            "missing attribute value in <{tag_name}> at byte {tag_start}"
        )));
    }
    Ok(&source[value_start..*cursor])
}

fn closing_tag_end(
    source: &str,
    start: usize,
    expected_name: &str,
) -> Result<Option<usize>, FormatError> {
    let bytes = source.as_bytes();
    if !bytes[start..].starts_with(b"</") {
        return Ok(None);
    }
    let mut cursor = start + 2;
    let name_start = cursor;
    while bytes
        .get(cursor)
        .is_some_and(|byte| is_name_continue(*byte))
    {
        cursor += 1;
    }
    let name = &source[name_start..cursor];
    if !name.eq_ignore_ascii_case(expected_name) {
        return Ok(None);
    }
    skip_ascii_whitespace(bytes, &mut cursor);
    if bytes.get(cursor) != Some(&b'>') {
        return Err(parse_error(format!(
            "malformed closing </{expected_name}> tag at byte {start}"
        )));
    }
    record_steps(cursor + 1 - start);
    Ok(Some(cursor + 1))
}

fn comment_end(bytes: &[u8], start: usize) -> Result<usize, FormatError> {
    let content_start = start + 4;
    let relative = memmem::find(&bytes[content_start..], b"-->").ok_or_else(|| {
        record_steps(bytes.len().saturating_sub(start));
        parse_error(format!("unclosed HTML comment at byte {start}"))
    })?;
    let end = content_start + relative + 3;
    record_steps(end - start);
    Ok(end)
}

fn script_source_type<'a>(
    attributes: &'a [Attribute<'a>],
) -> Option<(SourceType, Option<&'a str>)> {
    let lang = attributes
        .iter()
        .find(|attribute| attribute.name.eq_ignore_ascii_case("lang"))
        .map(|attribute| attribute.value);
    match lang {
        None => Some((SourceType::mjs(), None)),
        Some(Some("js")) => Some((SourceType::mjs(), Some("js"))),
        Some(Some("jsx")) => Some((SourceType::jsx(), Some("jsx"))),
        Some(Some("ts")) => Some((SourceType::ts(), Some("ts"))),
        Some(Some("tsx")) => Some((SourceType::tsx(), Some("tsx"))),
        Some(None | Some(_)) => None,
    }
}

fn script_label(attributes: &[Attribute<'_>], lang: Option<&str>) -> String {
    let setup = has_attribute(attributes, "setup");
    match (setup, lang) {
        (false, None) => "<script>".to_owned(),
        (true, None) => "<script setup>".to_owned(),
        (false, Some(lang)) => format!("<script lang=\"{lang}\">"),
        (true, Some(lang)) => format!("<script setup lang=\"{lang}\">"),
    }
}

fn has_attribute(attributes: &[Attribute<'_>], name: &str) -> bool {
    attributes
        .iter()
        .any(|attribute| attribute.name.eq_ignore_ascii_case(name))
}

fn skip_ascii_whitespace(bytes: &[u8], cursor: &mut usize) {
    let start = *cursor;
    while bytes.get(*cursor).is_some_and(u8::is_ascii_whitespace) {
        *cursor += 1;
    }
    record_steps(*cursor - start);
}

const fn is_name_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic()
}

const fn is_name_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':')
}

fn parse_error(diagnostics: impl Into<String>) -> FormatError {
    FormatError::EmbeddedParse {
        diagnostics: format!("Vue SFC: {}", diagnostics.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{embedded_regions, reset_scanner_steps, scanner_steps};

    fn labels(source: &str) -> Vec<(String, String)> {
        embedded_regions(source)
            .unwrap()
            .into_iter()
            .map(|region| (region.label, source[region.range].to_owned()))
            .collect()
    }

    #[test]
    fn extracts_normal_and_setup_scripts_with_supported_languages() {
        let source = concat!(
            "<script>js</script>",
            "<script setup='yes' lang='js'>explicit js</script>",
            "<script lang=jsx setup>jsx</script>",
            "<script data-value=plain LANG = \"ts\" setup>ts</script>",
            "<script setup lang=\"tsx\">tsx</script>",
        );
        assert_eq!(
            labels(source),
            [
                ("<script>".to_owned(), "js".to_owned()),
                (
                    "<script setup lang=\"js\">".to_owned(),
                    "explicit js".to_owned()
                ),
                ("<script setup lang=\"jsx\">".to_owned(), "jsx".to_owned()),
                ("<script setup lang=\"ts\">".to_owned(), "ts".to_owned()),
                ("<script setup lang=\"tsx\">".to_owned(), "tsx".to_owned()),
            ]
        );
    }

    #[test]
    fn handles_comments_quotes_nested_templates_and_opaque_blocks() {
        let source = concat!(
            "\u{feff}<!-- <script>fake</script> -->\r\n",
            "<template data-label=\">\"><template><div>ok</div></template>",
            "<script>template child</script></template>",
            "<style>/* <script>style</script> */</style>",
            "<docs><script>custom</script></docs>",
            "<script data-label=\">\">\r\nconst é=1;\r\n</script>",
        );
        assert_eq!(
            labels(source),
            [("<script>".to_owned(), "\r\nconst é=1;\r\n".to_owned())]
        );
    }

    #[test]
    fn treats_interpolations_and_custom_blocks_as_opaque_content() {
        let source = concat!(
            r#"<template>{{ "<template></template><script><!--" }}</template>"#,
            r#"<i18n>{"message":"<!--"}</i18n>"#,
            "<script>real script</script>",
        );
        assert_eq!(
            labels(source),
            [("<script>".to_owned(), "real script".to_owned())]
        );
    }

    #[test]
    fn treats_html_comment_markers_as_script_content() {
        let source = "<script>const marker = '<!--';</script>";
        assert_eq!(
            labels(source),
            [("<script>".to_owned(), "const marker = '<!--';".to_owned())]
        );
    }

    #[test]
    fn skips_src_unknown_languages_self_closing_and_empty_scripts() {
        let source = concat!(
            "<script src='./external.ts' lang='ts'></script>",
            "<script src=./external.ts lang=ts></script>",
            "<script lang=coffee>coffee()</script>",
            "<script lang>boolean lang</script>",
            "<script setup />",
            "<script></script>",
            "<script lang=ts></script>",
        );
        assert_eq!(
            labels(source),
            [
                ("<script>".to_owned(), String::new()),
                ("<script lang=\"ts\">".to_owned(), String::new()),
            ]
        );
    }

    #[test]
    fn rejects_unclosed_comments_tags_and_blocks() {
        for source in [
            "<!-- no end",
            "<script lang=\"ts>const value=1;</script>",
            "<script lang=>const value=1;</script>",
            "<script>const value=1;",
            "<template><template></template>",
            "<template>{{ value</template>",
            "</template>",
        ] {
            assert!(embedded_regions(source).is_err(), "{source}");
        }
    }

    #[test]
    fn scanner_steps_stay_linear_for_adversarial_input() {
        let mut source = "<template>".to_owned();
        for _ in 0..10_000 {
            source.push_str("<not-script data-value=\">\">x</not-script>");
        }
        source.push_str("</template><script lang=ts>const value=1;</script>");
        reset_scanner_steps();
        let regions = embedded_regions(&source).unwrap();
        assert_eq!(regions.len(), 1);
        assert!(
            scanner_steps() <= source.len() * 8,
            "{} steps for {} bytes",
            scanner_steps(),
            source.len()
        );
    }
}

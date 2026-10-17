use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use oxc_ast::ast::{
    CommentPosition, TSInterfaceDeclaration, TSType, TSTypeAliasDeclaration, TSUnionType,
};
use oxc_ast_visit::{
    Visit,
    walk::{walk_ts_interface_declaration, walk_ts_type_alias_declaration, walk_ts_union_type},
};
use oxc_parser::Kind;
use oxc_span::{GetSpan, SourceType, Span};

use super::{
    Allocator, Comment, Edit, ExpandedLayout, FormatError, LayoutIndents, LineBreakIndex,
    ObjectLayout, ParentItem, ResolvedConfig, StatementList, SwitchLayout, Token,
    TypeMemberSemicolonConfig, UnionLayoutMode, apply_edits, comments_in_span, contains_line_break,
    detect_newline, is_line_scoped_typescript_directive, parse_tokens, rewrite_after_quotes,
    source_slice, tokens_in_span, typescript_directive_target_lines,
};

#[derive(Debug)]
pub(super) struct UnionLayout {
    span: Span,
    pub prefix: Span,
    pub variants: Vec<Span>,
    pub layout_parent: Option<ExpandedLayout>,
    pub parent_item: Option<ParentItem>,
    object_variants: bool,
}

impl UnionLayout {
    pub(super) fn collect(
        tokens: &[Token],
        union: &TSUnionType<'_>,
        layout_parent: Option<ExpandedLayout>,
        parent_item: Option<ParentItem>,
    ) -> Self {
        let previous = tokens.partition_point(|token| token.start() < union.span.start);
        let prefix_start = previous
            .checked_sub(1)
            .map_or(union.span.start, |index| tokens[index].end());
        // Oxc omits parenthesized type wrappers with preserve_parens disabled. Recover
        // their token boundaries so moving a separator cannot remove required grouping.
        let mut variants = union.types.iter().map(GetSpan::span).collect::<Vec<_>>();
        variants[0].start = tokens_in_span(tokens, union.span)
            .iter()
            .find(|token| token.kind() != Kind::Pipe)
            .unwrap()
            .start();
        for index in 0..union.types.len() - 1 {
            let previous = union.types[index].span();
            let next = union.types[index + 1].span();
            let gap = tokens_in_span(tokens, Span::new(previous.end, next.start));
            let separator = gap
                .iter()
                .position(|token| token.kind() == Kind::Pipe)
                .unwrap();
            variants[index].end = separator
                .checked_sub(1)
                .map_or(previous.end, |index| gap[index].end());
            variants[index + 1].start = gap.get(separator + 1).map_or(next.start, Token::start);
        }
        variants.last_mut().unwrap().end = union.span.end;
        Self {
            span: union.span,
            prefix: Span::new(prefix_start, variants[0].start),
            variants,
            layout_parent,
            parent_item,
            object_variants: union
                .types
                .iter()
                .all(|variant| matches!(variant, TSType::TSTypeLiteral(_))),
        }
    }

    fn boundaries(&self) -> impl Iterator<Item = Span> + '_ {
        std::iter::once(self.prefix).chain(
            self.variants
                .windows(2)
                .map(|pair| Span::new(pair[0].end, pair[1].start)),
        )
    }
}

struct SyntaxCollector<'s> {
    tokens: &'s [Token],
    unions: Vec<UnionLayout>,
    declaration_depth: usize,
}

impl<'a> Visit<'a> for SyntaxCollector<'_> {
    fn visit_ts_type_alias_declaration(&mut self, declaration: &TSTypeAliasDeclaration<'a>) {
        self.declaration_depth += 1;
        walk_ts_type_alias_declaration(self, declaration);
        self.declaration_depth -= 1;
    }

    fn visit_ts_interface_declaration(&mut self, declaration: &TSInterfaceDeclaration<'a>) {
        self.declaration_depth += 1;
        walk_ts_interface_declaration(self, declaration);
        self.declaration_depth -= 1;
    }

    fn visit_ts_union_type(&mut self, union: &TSUnionType<'a>) {
        // Children come first so their final multiline shape is known to their parent.
        walk_ts_union_type(self, union);
        if self.declaration_depth > 0 && union.types.len() >= 2 {
            self.unions
                .push(UnionLayout::collect(self.tokens, union, None, None));
        }
    }
}

fn syntax(parsed: &oxc_parser::ParserReturn<'_>) -> Vec<UnionLayout> {
    let mut collector = SyntaxCollector {
        tokens: &parsed.tokens,
        unions: Vec::new(),
        declaration_depth: 0,
    };
    collector.visit_program(&parsed.program);
    collector.unions
}

fn protected_lines(
    source: &str,
    parsed: &oxc_parser::ParserReturn<'_>,
    lines: &LineBreakIndex,
) -> Result<HashSet<u32>, FormatError> {
    let mut protected = typescript_directive_target_lines(source, &parsed.program.comments)?;
    protected.extend(
        parsed
            .program
            .comments
            .iter()
            .filter(|comment| {
                source_slice(source, comment.span).is_ok_and(is_line_scoped_typescript_directive)
            })
            .map(|comment| lines.line_start(comment.span.end)),
    );
    Ok(protected)
}

fn protected_token_boundaries(
    source: &str,
    parsed: &oxc_parser::ParserReturn<'_>,
    lines: &LineBreakIndex,
) -> Result<Vec<u32>, FormatError> {
    let mut protected = protected_lines(source, parsed, lines)?
        .into_iter()
        .collect::<Vec<_>>();
    protected.sort_unstable();
    let mut boundaries = Vec::new();
    for line in protected {
        let first = parsed.tokens.partition_point(|token| token.start() < line);
        for token in &parsed.tokens[first..] {
            if lines.line_start(token.start()) != line {
                break;
            }
            boundaries.push(token.start());
            // A union prefix starts at the previous token's end. Protect that
            // boundary too, so a union cannot move onto a directive's target line.
            boundaries.push(token.end());
        }
    }
    Ok(boundaries)
}

fn is_protected(union: &UnionLayout, protected: &[u32]) -> bool {
    let index = protected.partition_point(|start| *start < union.prefix.start);
    protected
        .get(index)
        .is_some_and(|start| *start < union.span.end)
}

/// Normalizes union boundaries before collecting parent shapes. A collapsed union must not
/// leave its former multiline flag on a call, object property, or class field.
fn flat_edits(
    source: &str,
    parsed: &oxc_parser::ParserReturn<'_>,
    unions: &[UnionLayout],
    mode: UnionLayoutMode,
) -> Result<Vec<Edit>, FormatError> {
    let lines = LineBreakIndex::new(source);
    let protected = protected_token_boundaries(source, parsed, &lines)?;
    let mut edits = Vec::new();
    let mut breaks = source
        .char_indices()
        .filter(|(_, character)| matches!(character, '\n' | '\r'))
        .map(|(index, _)| u32::try_from(index).unwrap())
        .collect::<BTreeSet<_>>();
    for union in unions {
        if (mode == UnionLayoutMode::Auto && union.object_variants)
            || is_protected(union, &protected)
            || union
                .boundaries()
                .any(|span| !comments_in_span(&parsed.program.comments, span).is_empty())
        {
            continue;
        }
        if union
            .variants
            .iter()
            .any(|variant| breaks.range(variant.start..variant.end).next().is_some())
        {
            continue;
        }
        for (index, boundary) in union.boundaries().enumerate() {
            let original = source_slice(source, boundary)?;
            let replacement = inline_boundary(source, boundary, index)?;
            if original != replacement {
                let removed = breaks
                    .range(boundary.start..boundary.end)
                    .copied()
                    .collect::<Vec<_>>();
                for offset in removed {
                    breaks.remove(&offset);
                }
                edits.push(Edit {
                    start: boundary.start,
                    end: boundary.end,
                    replacement: replacement.to_owned(),
                });
            }
        }
    }
    edits.sort_by_key(|edit| (edit.start, edit.end));
    Ok(edits)
}

fn inline_boundary(source: &str, boundary: Span, index: usize) -> Result<&str, FormatError> {
    let original = source_slice(source, boundary)?;
    if index > 0 {
        return Ok(" | ");
    }
    if !contains_line_break(original) && !original.contains('|') {
        return Ok(original);
    }
    let previous = source
        .get(..boundary.start as usize)
        .and_then(|prefix| prefix.chars().next_back());
    Ok(if matches!(previous, Some('(' | '[' | '<')) {
        ""
    } else {
        " "
    })
}

fn rewrite_object_chains(
    source: &str,
    parsed: &oxc_parser::ParserReturn<'_>,
) -> Result<Option<String>, FormatError> {
    let lines = LineBreakIndex::new(source);
    let protected = protected_token_boundaries(source, parsed, &lines)?;
    let comments = &parsed.program.comments;
    let mut edits = Vec::new();
    let mut indents = BTreeMap::new();
    // Outer variants move first; inner chains then compose their relative shift
    // with that move instead of producing overlapping indentation edits.
    for union in syntax(parsed).into_iter().rev() {
        if !union.object_variants || is_protected(&union, &protected) {
            continue;
        }
        for (index, boundary) in union.boundaries().enumerate() {
            let commented = !comments_in_span(comments, boundary).is_empty();
            let base_at = if commented && lines.contains(boundary) {
                union.variants[index].start
            } else {
                boundary.start
            };
            let base = effective_indent(source, &lines, &mut indents, base_at);
            rebase_variant(
                source,
                parsed,
                &lines,
                &mut indents,
                union.variants[index],
                &base,
            );
            if !commented {
                let replacement = inline_boundary(source, boundary, index)?;
                if source_slice(source, boundary)? != replacement {
                    edits.push(Edit {
                        start: boundary.start,
                        end: boundary.end,
                        replacement: replacement.to_owned(),
                    });
                }
            }
        }
    }
    edits.sort_by_key(|edit| edit.start);
    let mut indent_edits = Vec::new();
    for (start, replacement) in indents {
        let original = source[start as usize..].trim_start_matches([' ', '\t']);
        let end = u32::try_from(source.len() - original.len()).unwrap();
        if source_slice(source, Span::new(start, end))? == replacement {
            continue;
        }
        // A joined boundary already owns any indentation on the line it removes.
        let next = edits.partition_point(|edit| edit.end < start);
        if edits.get(next).is_some_and(|edit| edit.start <= end) {
            continue;
        }
        indent_edits.push(Edit {
            start,
            end,
            replacement,
        });
    }
    edits.extend(indent_edits);
    edits.sort_by_key(|edit| edit.start);
    if edits.is_empty() {
        Ok(None)
    } else {
        apply_edits(source, &edits).map(Some)
    }
}

fn effective_indent(
    source: &str,
    lines: &LineBreakIndex,
    indents: &mut BTreeMap<u32, String>,
    offset: u32,
) -> String {
    let start = lines.line_start(offset);
    indents
        .entry(start)
        .or_insert_with(|| {
            source[start as usize..]
                .chars()
                .take_while(|character| matches!(character, ' ' | '\t'))
                .collect()
        })
        .clone()
}

fn rebase_variant(
    source: &str,
    parsed: &oxc_parser::ParserReturn<'_>,
    lines: &LineBreakIndex,
    indents: &mut BTreeMap<u32, String>,
    variant: Span,
    base: &str,
) {
    let previous = effective_indent(source, lines, indents, variant.start);
    if previous == base {
        return;
    }
    let first = lines
        .offsets
        .partition_point(|offset| *offset < variant.start);
    for &offset in &lines.offsets[first..] {
        let start = offset + 1;
        if start >= variant.end {
            break;
        }
        if source
            .as_bytes()
            .get(start as usize)
            .is_some_and(|byte| matches!(byte, b'\r' | b'\n'))
        {
            continue;
        }
        let token = parsed.tokens.partition_point(|token| token.end() <= start);
        let comment = parsed
            .program
            .comments
            .partition_point(|comment| comment.span.end <= start);
        if parsed
            .tokens
            .get(token)
            .is_some_and(|token| token.start() < start)
            || parsed
                .program
                .comments
                .get(comment)
                .is_some_and(|comment| comment.span.start < start)
        {
            continue;
        }
        let indent = effective_indent(source, lines, indents, start);
        if let Some(relative) = indent.strip_prefix(&previous) {
            indents.insert(start, format!("{base}{relative}"));
        }
    }
    indents.insert(lines.line_start(variant.start), base.to_owned());
}

fn expansions(
    source: &str,
    parsed: &oxc_parser::ParserReturn<'_>,
    mode: UnionLayoutMode,
    width: u32,
) -> Result<HashSet<u32>, FormatError> {
    let lines = LineBreakIndex::new(source);
    let protected = protected_token_boundaries(source, parsed, &lines)?;
    let mut widths = HashMap::new();
    let mut selected = BTreeSet::new();
    for union in syntax(parsed) {
        if is_protected(&union, &protected) {
            continue;
        }
        if mode == UnionLayoutMode::Auto && union.object_variants {
            continue;
        }
        let start = lines.line_start(union.prefix.start);
        let line_width = widths.entry(start).or_insert_with(|| {
            source[start as usize..]
                .split(['\n', '\r'])
                .next()
                .unwrap_or_default()
                .chars()
                .count()
        });
        if mode == UnionLayoutMode::Always
            || *line_width > width as usize
            || contains_line_break(source_slice(
                source,
                Span::new(union.prefix.start, union.span.end),
            )?)
            || selected
                .range(union.span.start..union.span.end)
                .next()
                .is_some()
        {
            selected.insert(union.span.start);
        }
    }
    Ok(selected.into_iter().collect())
}

// A layout transaction first removes collapsible union breaks, then measures the text
// produced by the other rules. Object chains join before width-based unions are selected
// to participate in the ordinary layout cascade.
// If that cascade unfolds a containing list, repeat the transaction using its final position;
// this lets auto collapse a union that fits after the parent moved to its own line.
pub(super) fn rewrite(
    source: &str,
    parsed: &oxc_parser::ParserReturn<'_>,
    source_type: SourceType,
    newline_hint: Option<&'static str>,
    config: &ResolvedConfig,
    type_semicolons: TypeMemberSemicolonConfig,
) -> Result<Option<String>, FormatError> {
    let newline_hint = Some(detect_newline(source, newline_hint));
    let unions = if source_type.is_typescript() && config.union_layout() != UnionLayoutMode::Off {
        syntax(parsed)
    } else {
        Vec::new()
    };
    if unions.is_empty() {
        return rewrite_after_quotes(
            source,
            parsed,
            source_type,
            newline_hint,
            config,
            type_semicolons,
            None,
        );
    }
    let mut current = transaction(
        source,
        parsed,
        &unions,
        source_type,
        newline_hint,
        config,
        type_semicolons,
    )?;
    if current == source {
        return Ok(None);
    }
    let mut seen = HashSet::new();
    seen.insert(source.to_owned());
    loop {
        if !seen.insert(current.clone()) {
            return Err(FormatError::internal("union layout did not converge"));
        }
        let allocator = Allocator::default();
        let parsed = parse_tokens(&allocator, &current, source_type)?;
        let unions = syntax(&parsed);
        let next = transaction(
            &current,
            &parsed,
            &unions,
            source_type,
            newline_hint,
            config,
            type_semicolons,
        )?;
        if next == current {
            return Ok(Some(current));
        }
        current = next;
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "union decisions share the parsed source and resolved formatting context"
)]
fn transaction(
    source: &str,
    parsed: &oxc_parser::ParserReturn<'_>,
    unions: &[UnionLayout],
    source_type: SourceType,
    newline_hint: Option<&'static str>,
    config: &ResolvedConfig,
    type_semicolons: TypeMemberSemicolonConfig,
) -> Result<String, FormatError> {
    let edits = flat_edits(source, parsed, unions, config.union_layout())?;
    let flat = if edits.is_empty() {
        None
    } else {
        Some(apply_edits(source, &edits)?)
    };
    let flat_source = flat.as_deref().unwrap_or(source);
    let flat_allocator = Allocator::default();
    let flat_parsed;
    let flat_parse = if flat.is_some() {
        flat_parsed = parse_tokens(&flat_allocator, flat_source, source_type)?;
        &flat_parsed
    } else {
        parsed
    };
    let base = rewrite_after_quotes(
        flat_source,
        flat_parse,
        source_type,
        newline_hint,
        config,
        type_semicolons,
        None,
    )?;
    let base_source = base.as_deref().unwrap_or(flat_source);
    let base_allocator = Allocator::default();
    let base_parsed;
    let base_parse = if base.is_some() {
        base_parsed = parse_tokens(&base_allocator, base_source, source_type)?;
        &base_parsed
    } else {
        flat_parse
    };
    let joined = if config.union_layout() == UnionLayoutMode::Auto {
        rewrite_object_chains(base_source, base_parse)?
    } else {
        None
    };
    let joined_source = joined.as_deref().unwrap_or(base_source);
    let joined_allocator = Allocator::default();
    let joined_parsed;
    let joined_parse = if joined.is_some() {
        joined_parsed = parse_tokens(&joined_allocator, joined_source, source_type)?;
        &joined_parsed
    } else {
        base_parse
    };
    let selected = expansions(
        joined_source,
        joined_parse,
        config.union_layout(),
        config.line_width(),
    )?;
    if selected.is_empty() {
        return Ok(joined_source.to_owned());
    }
    let output = rewrite_after_quotes(
        joined_source,
        joined_parse,
        source_type,
        newline_hint,
        config,
        type_semicolons,
        Some(&selected),
    )?;
    Ok(output.unwrap_or_else(|| joined_source.to_owned()))
}

#[allow(
    clippy::too_many_arguments,
    reason = "union indentation uses the shared parent layout graph"
)]
pub(super) fn append_edits(
    source: &str,
    comments: &[Comment],
    newline: &str,
    lists: &[StatementList],
    switches: &[SwitchLayout],
    objects: &[ObjectLayout],
    indents: &mut LayoutIndents<'_>,
    edits: &mut Vec<Edit>,
) -> Result<(), FormatError> {
    for (index, union) in indents.unions.iter().enumerate() {
        let base = indents.union_base_indent(source, lists, switches, objects, index);
        let indent = format!("{base}  ");
        for boundary in union.boundaries() {
            let replacement = expanded_boundary(source, comments, boundary, newline, &indent)?;
            if source_slice(source, boundary)? != replacement {
                edits.push(Edit {
                    start: boundary.start,
                    end: boundary.end,
                    replacement,
                });
            }
        }
    }
    Ok(())
}

fn expanded_boundary(
    source: &str,
    comments: &[Comment],
    span: Span,
    newline: &str,
    indent: &str,
) -> Result<String, FormatError> {
    let boundary_comments = comments_in_span(comments, span);
    if boundary_comments.is_empty() {
        return Ok(format!("{newline}{indent}| "));
    }
    // Keep raw comment text and every existing line break. Move only the separator and
    // same-line padding, preserving whether a comment belongs to either neighboring variant.
    let first_leading = boundary_comments
        .iter()
        .find(|comment| comment.position == CommentPosition::Leading);
    let last = boundary_comments.last().unwrap();
    let suffix = source_slice(source, Span::new(last.span.end, span.end))?;
    if let Some(leading) = first_leading
        && !contains_line_break(suffix)
    {
        let before = without_pipe(
            source,
            Span::new(span.start, leading.span.start),
            boundary_comments,
        )?;
        let after = without_pipe(
            source,
            Span::new(leading.span.start, span.end),
            boundary_comments,
        )?;
        let before = before.trim_end_matches([' ', '\t']);
        let separator = if contains_line_break(before) {
            ""
        } else {
            newline
        };
        return Ok(format!("{before}{separator}{indent}| {after}"));
    }
    let raw = without_pipe(source, span, boundary_comments)?;
    let raw = raw.trim_end_matches([' ', '\t']);
    let separator = if raw.ends_with(['\n', '\r']) {
        ""
    } else {
        newline
    };
    Ok(format!("{raw}{separator}{indent}| "))
}

fn without_pipe(source: &str, span: Span, comments: &[Comment]) -> Result<String, FormatError> {
    let text = source_slice(source, span)?;
    let mut output = String::with_capacity(text.len());
    let mut padding = String::new();
    let mut comment_index = 0;
    for (offset, character) in text.char_indices() {
        let position = span.start as usize + offset;
        while comments
            .get(comment_index)
            .is_some_and(|comment| comment.span.end as usize <= position)
        {
            comment_index += 1;
        }
        let inside_comment = comments
            .get(comment_index)
            .is_some_and(|comment| comment.span.start as usize <= position);
        if !inside_comment && character == '|' {
            continue;
        }
        if !inside_comment && matches!(character, ' ' | '\t') {
            padding.push(character);
            continue;
        }
        if inside_comment || !matches!(character, '\n' | '\r') {
            output.push_str(&padding);
        }
        padding.clear();
        output.push(character);
    }
    output.push_str(&padding);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;
    use crate::rewriter::{
        LEADING_INDENT_PREFIX_BYTES, LINE_START_INDEX_QUERIES, RAW_INDENT_PREFIX_BYTES,
    };

    #[test]
    fn object_chain_indentation_work_is_bounded() {
        for count in [8, 256] {
            let source = format!(
                "type U =\n [{}]",
                vec!["{ a: A } | { b: B }"; count].join(", ")
            );
            let config = crate::resolve_config(
                serde_json::from_value(serde_json::json!({
                    "lineWidth": source.len() * 2,
                    "rules": { "typeLiteralLayout": "off" }
                }))
                .unwrap(),
            )
            .unwrap();
            LEADING_INDENT_PREFIX_BYTES.set(0);
            RAW_INDENT_PREFIX_BYTES.set(0);
            assert!(
                crate::format_text(std::path::Path::new("sample.ts"), &source, &config)
                    .unwrap()
                    .is_none()
            );
            let scanned = LEADING_INDENT_PREFIX_BYTES.get() + RAW_INDENT_PREFIX_BYTES.get();
            assert!(
                scanned <= source.len() * 4,
                "read {scanned} prefix bytes for {} source bytes",
                source.len()
            );
        }
    }

    #[test]
    fn directive_protection_queries_stay_linear_for_nested_unions() {
        let depth = 16;
        let mut source = String::from("// @ts-expect-error\ntype Ignored = Missing\ntype U = ");
        source.push_str(&"A | Box<".repeat(depth));
        source.push_str("{\n");
        for index in 0..256 {
            writeln!(source, "  p{index}: number").unwrap();
        }
        source.push('}');
        source.push_str(&">".repeat(depth));
        let allocator = Allocator::default();
        let parsed = parse_tokens(&allocator, &source, SourceType::ts()).unwrap();
        let unions = syntax(&parsed);
        assert_eq!(unions.len(), depth);
        let query_budget = parsed.tokens.len() * 2;

        LINE_START_INDEX_QUERIES.set(0);
        assert!(
            flat_edits(&source, &parsed, &unions, UnionLayoutMode::Auto)
                .unwrap()
                .is_empty()
        );
        let queries = LINE_START_INDEX_QUERIES.get();
        assert!(
            queries <= query_budget,
            "flattening used {queries} line queries; linear budget: {query_budget}"
        );

        LINE_START_INDEX_QUERIES.set(0);
        let selected = expansions(&source, &parsed, UnionLayoutMode::Auto, 120).unwrap();
        assert_eq!(selected.len(), depth);
        let queries = LINE_START_INDEX_QUERIES.get();
        assert!(
            queries <= query_budget,
            "selection used {queries} line queries; linear budget: {query_budget}"
        );
    }
}

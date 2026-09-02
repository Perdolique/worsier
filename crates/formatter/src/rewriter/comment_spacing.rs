use std::collections::{HashMap, HashSet};

use oxc_ast::ast::{
    Program, Statement, SwitchCase, TSTypeAssertion, TSTypeParameterDeclaration,
    TSTypeParameterInstantiation,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::{Kind, Token};
use oxc_span::{GetSpan, Span};

use super::{
    Comment, Edit, FormatError, apply_edits, comments_in_span, source_slice, tokens_in_span,
};

#[cfg(test)]
thread_local! {
    static SCANNED_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Debug, Default)]
pub(super) struct CommentSpacing {
    enabled: bool,
    gaps: Vec<Gap>,
    before: HashMap<u32, usize>,
    after: HashMap<u32, usize>,
}

#[derive(Debug)]
struct Gap {
    span: Span,
    lines: usize,
}

struct Item {
    span: Span,
    kind: Option<Kind>,
    start_line: usize,
    end_line: usize,
}

impl CommentSpacing {
    pub(super) fn new(
        source: &str,
        program: &Program<'_>,
        tokens: &[Token],
    ) -> Result<Self, FormatError> {
        if program.comments.is_empty() {
            return Ok(Self::default());
        }
        let mut edges = SyntaxEdges {
            tokens,
            ..SyntaxEdges::default()
        };
        edges.visit_program(program);
        let items = lexical_items(source, &program.comments, tokens)?;
        let standalone = standalone_comments(&items);

        let source_end = u32::try_from(source.len()).unwrap();
        let mut spacing = Self {
            enabled: true,
            ..Self::default()
        };
        let mut index = 0;
        while index < items.len() {
            if !standalone[index] {
                index += 1;
                continue;
            }
            let first = index;
            while index + 1 < items.len()
                && standalone[index + 1]
                && items[index + 1].start_line - items[index].end_line < 2
            {
                index += 1;
            }
            let last = index;
            let previous = first.checked_sub(1).map(|index| &items[index]);
            let next = items.get(last + 1);
            let before = Span::new(
                previous.map_or(0, |item| item.span.end),
                items[first].span.start,
            );
            let after = Span::new(
                items[last].span.end,
                next.map_or(source_end, |item| item.span.start),
            );
            let before_lines = line_breaks(source_slice(source, before)?);
            let after_lines = line_breaks(source_slice(source, after)?);
            let attached = after_lines == 1 && next.is_some_and(|item| !is_closing(item, &edges));
            let at_start = previous.is_none_or(|item| is_opening(item, &edges))
                || next.is_some_and(|item| edges.body_starts.contains(&item.span.start));
            spacing.push(
                before,
                if attached && !at_start {
                    before_lines.max(2)
                } else {
                    before_lines
                },
            );
            for pair in items[first..=last].windows(2) {
                let span = Span::new(pair[0].span.end, pair[1].span.start);
                spacing.push(span, line_breaks(source_slice(source, span)?));
            }
            spacing.push(after, after_lines);
            index += 1;
        }
        Ok(spacing)
    }

    fn push(&mut self, span: Span, lines: usize) {
        if self.gaps.last().is_some_and(|gap| gap.span == span) {
            return;
        }
        self.before.insert(span.end, lines);
        self.after.insert(span.start, lines);
        self.gaps.push(Gap { span, lines });
    }

    pub(super) fn before(&self, offset: u32) -> Option<usize> {
        self.before.get(&offset).copied()
    }

    pub(super) const fn enabled(&self) -> bool {
        self.enabled
    }

    pub(super) fn after(&self, offset: u32) -> Option<usize> {
        self.after.get(&offset).copied()
    }

    // Boundary renderers copy comment text verbatim and contain no other syntax except an
    // optional semicolon. Match comments in order, then restore only their whitespace gaps.
    pub(super) fn boundary(
        &self,
        source: &str,
        span: Span,
        comments: &[Comment],
        formatted: String,
        newline: &str,
    ) -> Result<String, FormatError> {
        if self.gaps.is_empty() {
            return Ok(formatted);
        }
        let mut cursor = 0;
        let mut edits = Vec::new();
        for comment in comments_in_span(comments, span) {
            let text = source_slice(source, comment.span)?;
            let offset = formatted[cursor..]
                .find(text)
                .ok_or_else(|| FormatError::internal("boundary lost comment text"))?;
            let start = cursor + offset;
            let end = start + text.len();
            if let Some(lines) = self.before(comment.span.start) {
                let prefix = &formatted[..start];
                let gap_start = prefix.trim_end_matches(char::is_whitespace).len();
                append_gap_edit(
                    &formatted,
                    Span::new(
                        u32::try_from(gap_start).unwrap(),
                        u32::try_from(start).unwrap(),
                    ),
                    lines,
                    newline,
                    &mut edits,
                )?;
            }
            if let Some(lines) = self.after(comment.span.end) {
                let suffix = &formatted[end..];
                let gap_end =
                    formatted.len() - suffix.trim_start_matches(char::is_whitespace).len();
                append_gap_edit(
                    &formatted,
                    Span::new(u32::try_from(end).unwrap(), u32::try_from(gap_end).unwrap()),
                    lines,
                    newline,
                    &mut edits,
                )?;
            }
            cursor = end;
        }
        edits.sort_by_key(|edit| (edit.start, edit.end));
        edits.dedup_by_key(|edit| (edit.start, edit.end));
        if edits.is_empty() {
            Ok(formatted)
        } else {
            apply_edits(&formatted, &edits)
        }
    }

    pub(super) fn append_uncovered(
        &self,
        source: &str,
        newline: &str,
        edits: &mut Vec<Edit>,
    ) -> Result<(), FormatError> {
        if self.gaps.is_empty() {
            return Ok(());
        }
        let mut additions = Vec::new();
        let mut index = 0;
        for gap in &self.gaps {
            while index < edits.len() && edits[index].end <= gap.span.start {
                index += 1;
            }
            if edits
                .get(index)
                .is_some_and(|edit| edit.start < gap.span.end && edit.end > gap.span.start)
            {
                continue;
            }
            append_gap_edit(source, gap.span, gap.lines, newline, &mut additions)?;
        }
        edits.extend(additions);
        edits.sort_by_key(|edit| (edit.start, edit.end));
        Ok(())
    }
}

fn standalone_comments(items: &[Item]) -> Vec<bool> {
    let mut standalone = vec![false; items.len()];
    let mut previous_line = None;
    for (index, item) in items.iter().enumerate() {
        if item.kind.is_some() {
            previous_line = Some(item.end_line);
        } else {
            let connected = index.checked_sub(1).filter(|previous| {
                items[*previous].kind.is_none() && items[*previous].end_line == item.start_line
            });
            standalone[index] = previous_line != Some(item.start_line)
                && connected.is_none_or(|previous| standalone[previous]);
        }
    }
    let mut next_line = None;
    for (index, item) in items.iter().enumerate().rev() {
        if item.kind.is_some() {
            next_line = Some(item.start_line);
        } else {
            let connected = items
                .get(index + 1)
                .is_some_and(|next| next.kind.is_none() && next.start_line == item.end_line);
            standalone[index] &=
                next_line != Some(item.end_line) && (!connected || standalone[index + 1]);
        }
    }
    standalone
}

fn append_gap_edit(
    source: &str,
    span: Span,
    lines: usize,
    newline: &str,
    edits: &mut Vec<Edit>,
) -> Result<(), FormatError> {
    let original = source_slice(source, span)?;
    if line_breaks(original) != lines {
        let indent = original
            .rsplit(['\n', '\r', '\u{2028}', '\u{2029}'])
            .next()
            .unwrap_or("");
        edits.push(Edit {
            start: span.start,
            end: span.end,
            replacement: format!("{}{indent}", newline.repeat(lines)),
        });
    }
    Ok(())
}

fn lexical_items(
    source: &str,
    comments: &[Comment],
    tokens: &[Token],
) -> Result<Vec<Item>, FormatError> {
    let mut tokens = tokens
        .iter()
        .filter(|token| token.kind() != Kind::Eof)
        .peekable();
    let mut comments = comments.iter().peekable();
    let mut items = Vec::new();
    let mut cursor = 0;
    let mut line = 0;
    while tokens.peek().is_some() || comments.peek().is_some() {
        let take_comment = comments.peek().is_some_and(|comment| {
            tokens
                .peek()
                .is_none_or(|token| comment.span.start < token.start())
        });
        let (span, kind) = if take_comment {
            (comments.next().unwrap().span, None)
        } else {
            let token = tokens.next().unwrap();
            (token.span(), Some(token.kind()))
        };
        line += line_breaks(source_slice(source, Span::new(cursor, span.start))?);
        let start_line = line;
        line += line_breaks(source_slice(source, span)?);
        items.push(Item {
            span,
            kind,
            start_line,
            end_line: line,
        });
        cursor = span.end;
    }
    Ok(items)
}

fn line_breaks(text: &str) -> usize {
    #[cfg(test)]
    SCANNED_BYTES.set(SCANNED_BYTES.get() + text.len());
    let mut count = 0;
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                count += 1;
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
            }
            '\n' | '\u{2028}' | '\u{2029}' => count += 1,
            _ => {}
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use std::fmt::Write;
    use std::path::Path;

    use crate::{FormatConfig, format_text, resolve_config};

    use super::SCANNED_BYTES;

    #[test]
    fn comment_gap_scanning_stays_linear_and_skips_disabled_rules() {
        let mut source = String::from("first()\n");
        for index in 0..4000 {
            writeln!(source, "// item {index}\nitem{index}()").unwrap();
        }
        let config = resolve_config(FormatConfig::default()).unwrap();
        SCANNED_BYTES.set(0);
        format_text(Path::new("sample.ts"), &source, &config).unwrap();
        assert!(SCANNED_BYTES.get() > source.len());
        assert!(
            SCANNED_BYTES.get() < 8 * source.len(),
            "{} bytes scanned for {} source bytes",
            SCANNED_BYTES.get(),
            source.len()
        );

        let mut config = FormatConfig::default();
        config.rules.comment_spacing = false;
        let config = resolve_config(config).unwrap();
        SCANNED_BYTES.set(0);
        format_text(Path::new("sample.ts"), &source, &config).unwrap();
        assert_eq!(SCANNED_BYTES.get(), 0);
    }
}

fn is_opening(item: &Item, edges: &SyntaxEdges<'_>) -> bool {
    matches!(
        item.kind,
        Some(
            Kind::LCurly
                | Kind::LParen
                | Kind::LBrack
                | Kind::Arrow
                | Kind::TemplateHead
                | Kind::TemplateMiddle
                | Kind::HashbangComment
        )
    ) || (item.kind == Some(Kind::LAngle) && edges.angle_delimiters.contains(&item.span.start))
}

fn is_closing(item: &Item, edges: &SyntaxEdges<'_>) -> bool {
    matches!(
        item.kind,
        Some(
            Kind::RCurly
                | Kind::RParen
                | Kind::RBrack
                | Kind::Comma
                | Kind::Semicolon
                | Kind::TemplateTail
                | Kind::TemplateMiddle
        )
    ) || (item.kind == Some(Kind::RAngle) && edges.angle_delimiters.contains(&item.span.start))
}

#[derive(Default)]
struct SyntaxEdges<'t> {
    tokens: &'t [Token],
    body_starts: HashSet<u32>,
    angle_delimiters: HashSet<u32>,
}

impl<'a> Visit<'a> for SyntaxEdges<'_> {
    fn visit_statement(&mut self, statement: &Statement<'a>) {
        match statement {
            Statement::IfStatement(statement) => {
                self.body_starts.insert(statement.consequent.span().start);
                if let Some(alternate) = &statement.alternate {
                    self.body_starts.insert(alternate.span().start);
                }
            }
            Statement::WhileStatement(statement) => {
                self.body_starts.insert(statement.body.span().start);
            }
            Statement::DoWhileStatement(statement) => {
                self.body_starts.insert(statement.body.span().start);
            }
            Statement::ForStatement(statement) => {
                self.body_starts.insert(statement.body.span().start);
            }
            Statement::ForInStatement(statement) => {
                self.body_starts.insert(statement.body.span().start);
            }
            Statement::ForOfStatement(statement) => {
                self.body_starts.insert(statement.body.span().start);
            }
            Statement::WithStatement(statement) => {
                self.body_starts.insert(statement.body.span().start);
            }
            Statement::LabeledStatement(statement) => {
                self.body_starts.insert(statement.body.span().start);
            }
            _ => {}
        }
        walk::walk_statement(self, statement);
    }

    fn visit_switch_case(&mut self, case: &SwitchCase<'a>) {
        if let Some(first) = case.consequent.first() {
            self.body_starts.insert(first.span().start);
        }
        walk::walk_switch_case(self, case);
    }

    fn visit_ts_type_parameter_declaration(&mut self, parameters: &TSTypeParameterDeclaration<'a>) {
        self.angle_delimiters.insert(parameters.span.start);
        self.angle_delimiters.insert(parameters.span.end - 1);
        walk::walk_ts_type_parameter_declaration(self, parameters);
    }

    fn visit_ts_type_parameter_instantiation(
        &mut self,
        parameters: &TSTypeParameterInstantiation<'a>,
    ) {
        self.angle_delimiters.insert(parameters.span.start);
        self.angle_delimiters.insert(parameters.span.end - 1);
        walk::walk_ts_type_parameter_instantiation(self, parameters);
    }

    fn visit_ts_type_assertion(&mut self, assertion: &TSTypeAssertion<'a>) {
        let closing_span = Span::new(
            assertion.type_annotation.span().end,
            assertion.expression.span().start,
        );
        if let Some(closing) = tokens_in_span(self.tokens, closing_span)
            .iter()
            .find(|token| token.kind() == Kind::RAngle)
        {
            self.angle_delimiters.insert(closing.start());
        }
        walk::walk_ts_type_assertion(self, assertion);
    }
}

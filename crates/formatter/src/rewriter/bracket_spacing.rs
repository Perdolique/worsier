use oxc_ast::ast::{
    ArrayAssignmentTarget, ArrayExpression, ArrayPattern, ExportFromDeclaration,
    ExportNamedDeclaration, ImportDeclaration, ObjectAssignmentTarget, ObjectExpression,
    ObjectPattern, Program, TSInterfaceBody, TSMappedType, TSTupleType, TSTypeLiteral, WithClause,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::{Kind, Token};
use oxc_span::Span;

use crate::{BracketSpacingConfig, BracketSpacingMode};

use super::{
    Comment, Edit, FormatError, brace_tokens, exact_delimiters, named_braces, source_slice,
    tokens_in_span,
};

pub(super) fn append_edits(
    source: &str,
    program: &Program<'_>,
    tokens: &[Token],
    config: BracketSpacingConfig,
    import_layout: bool,
    edits: &mut Vec<Edit>,
) -> Result<(), FormatError> {
    if config.curly == BracketSpacingMode::Off && config.square == BracketSpacingMode::Off {
        return Ok(());
    }
    let mut collector = BracketCollector {
        source,
        tokens,
        comments: &program.comments,
        config,
        import_layout,
        edits: Vec::new(),
        error: None,
    };
    collector.visit_program(program);
    if let Some(error) = collector.error {
        return Err(error);
    }
    collector.edits.sort_by_key(|edit| (edit.start, edit.end));
    collector.edits.dedup_by_key(|edit| (edit.start, edit.end));
    let additions: Vec<_> = collector
        .edits
        .into_iter()
        .filter(|candidate| {
            let start = edits.partition_point(|edit| edit.end < candidate.start);
            !edits[start..]
                .iter()
                .take_while(|edit| edit.start <= candidate.end)
                .any(|edit| conflicts(candidate, edit))
        })
        .collect();
    edits.extend(additions);
    Ok(())
}

fn conflicts(candidate: &Edit, existing: &Edit) -> bool {
    if candidate.start == candidate.end {
        return if existing.start == existing.end {
            candidate.start == existing.start
        } else {
            existing.start <= candidate.start && candidate.start < existing.end
        };
    }
    if existing.start == existing.end {
        return candidate.start <= existing.start && existing.start < candidate.end;
    }
    candidate.start < existing.end && existing.start < candidate.end
}

struct BracketCollector<'s> {
    source: &'s str,
    tokens: &'s [Token],
    comments: &'s [Comment],
    config: BracketSpacingConfig,
    import_layout: bool,
    edits: Vec<Edit>,
    error: Option<FormatError>,
}

impl BracketCollector<'_> {
    fn record_exact(&mut self, span: Span, open: Kind, close: Kind, mode: BracketSpacingMode) {
        if mode == BracketSpacingMode::Off {
            return;
        }
        if let Some((open, close)) = exact_delimiters(self.tokens, span, open, close) {
            self.record(open, close, mode);
        }
    }

    fn record(&mut self, open: Span, close: Span, mode: BracketSpacingMode) {
        if mode == BracketSpacingMode::Off || self.error.is_some() {
            return;
        }
        let interior = Span::new(open.end, close.start);
        let tokens = tokens_in_span(self.tokens, interior);
        let comments = super::comments_in_span(self.comments, interior);
        let first = tokens
            .first()
            .map(Token::span)
            .into_iter()
            .chain(comments.first().map(|comment| comment.span))
            .min_by_key(|span| span.start);
        let last = tokens
            .last()
            .map(Token::span)
            .into_iter()
            .chain(comments.last().map(|comment| comment.span))
            .max_by_key(|span| span.end);
        let result = if let (Some(first), Some(last)) = (first, last) {
            self.record_gap(Span::new(open.end, first.start), mode, false)
                .and_then(|()| self.record_gap(Span::new(last.end, close.start), mode, false))
        } else {
            self.record_gap(interior, mode, true)
        };
        if let Err(error) = result {
            self.error = Some(error);
        }
    }

    fn record_gap(
        &mut self,
        gap: Span,
        mode: BracketSpacingMode,
        empty: bool,
    ) -> Result<(), FormatError> {
        let source = source_slice(self.source, gap)?;
        if !source.bytes().all(|byte| matches!(byte, b' ' | b'\t')) {
            return Ok(());
        }
        let replacement = if empty || mode == BracketSpacingMode::Never {
            ""
        } else {
            " "
        };
        if source != replacement {
            self.edits.push(Edit {
                start: gap.start,
                end: gap.end,
                replacement: replacement.to_owned(),
            });
        }
        Ok(())
    }
}

impl<'a> Visit<'a> for BracketCollector<'_> {
    fn visit_object_expression(&mut self, node: &ObjectExpression<'a>) {
        self.record_exact(node.span, Kind::LCurly, Kind::RCurly, self.config.curly);
        walk::walk_object_expression(self, node);
    }

    fn visit_object_pattern(&mut self, node: &ObjectPattern<'a>) {
        self.record_exact(node.span, Kind::LCurly, Kind::RCurly, self.config.curly);
        walk::walk_object_pattern(self, node);
    }

    fn visit_object_assignment_target(&mut self, node: &ObjectAssignmentTarget<'a>) {
        self.record_exact(node.span, Kind::LCurly, Kind::RCurly, self.config.curly);
        walk::walk_object_assignment_target(self, node);
    }

    fn visit_ts_type_literal(&mut self, node: &TSTypeLiteral<'a>) {
        self.record_exact(node.span, Kind::LCurly, Kind::RCurly, self.config.curly);
        walk::walk_ts_type_literal(self, node);
    }

    fn visit_ts_interface_body(&mut self, node: &TSInterfaceBody<'a>) {
        self.record_exact(node.span, Kind::LCurly, Kind::RCurly, self.config.curly);
        walk::walk_ts_interface_body(self, node);
    }

    fn visit_ts_mapped_type(&mut self, node: &TSMappedType<'a>) {
        self.record_exact(node.span, Kind::LCurly, Kind::RCurly, self.config.curly);
        walk::walk_ts_mapped_type(self, node);
    }

    fn visit_array_expression(&mut self, node: &ArrayExpression<'a>) {
        self.record_exact(node.span, Kind::LBrack, Kind::RBrack, self.config.square);
        walk::walk_array_expression(self, node);
    }

    fn visit_array_pattern(&mut self, node: &ArrayPattern<'a>) {
        self.record_exact(node.span, Kind::LBrack, Kind::RBrack, self.config.square);
        walk::walk_array_pattern(self, node);
    }

    fn visit_array_assignment_target(&mut self, node: &ArrayAssignmentTarget<'a>) {
        self.record_exact(node.span, Kind::LBrack, Kind::RBrack, self.config.square);
        walk::walk_array_assignment_target(self, node);
    }

    fn visit_ts_tuple_type(&mut self, node: &TSTupleType<'a>) {
        self.record_exact(node.span, Kind::LBrack, Kind::RBrack, self.config.square);
        walk::walk_ts_tuple_type(self, node);
    }

    fn visit_import_declaration(&mut self, node: &ImportDeclaration<'a>) {
        if self.import_layout {
            return;
        }
        if let Some((open, close)) = named_braces(node, self.tokens) {
            self.record(open, close, self.config.curly);
        }
        walk::walk_import_declaration(self, node);
    }

    fn visit_export_named_declaration(&mut self, node: &ExportNamedDeclaration<'a>) {
        if let Some((open, close)) = first_braces(self.tokens, node.span, None) {
            self.record(open, close, self.config.curly);
        }
        walk::walk_export_named_declaration(self, node);
    }

    fn visit_export_from_declaration(&mut self, node: &ExportFromDeclaration<'a>) {
        if let Some((open, close)) =
            first_braces(self.tokens, node.span, Some(node.source.span.start))
        {
            self.record(open, close, self.config.curly);
        }
        walk::walk_export_from_declaration(self, node);
    }

    fn visit_with_clause(&mut self, node: &WithClause<'a>) {
        if let Some((open, close)) = brace_tokens(self.tokens, node.span) {
            self.record(open, close, self.config.curly);
        }
        walk::walk_with_clause(self, node);
    }
}

fn first_braces(tokens: &[Token], span: Span, before: Option<u32>) -> Option<(Span, Span)> {
    let mut tokens = tokens_in_span(tokens, span)
        .iter()
        .filter(|token| before.is_none_or(|end| token.end() <= end));
    let open = tokens.find(|token| token.kind() == Kind::LCurly)?.span();
    let close = tokens.find(|token| token.kind() == Kind::RCurly)?.span();
    Some((open, close))
}

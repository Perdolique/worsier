use oxc_ast::ast::Comment;
use oxc_span::Span;

use crate::FormatError;
use crate::doc::{Doc, concat, hard_line, line_suffix, space, text};
use crate::index::{NodeCategory, NodeIndex, NodeRecord, category_priority};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Placement {
    Leading,
    LeadingInline,
    Trailing,
    TrailingOwnLine,
    Dangling,
    Suppression,
    Unattached,
}

#[derive(Clone, Copy, Debug)]
struct TrackedComment {
    comment: Comment,
    node: Option<usize>,
    placement: Placement,
    emitted: bool,
}

pub struct CommentTracker<'a> {
    source: &'a str,
    comments: Vec<TrackedComment>,
}

impl<'a> CommentTracker<'a> {
    #[must_use]
    pub fn new(source: &'a str, comments: &[Comment], index: &NodeIndex) -> Self {
        let comments = comments
            .iter()
            .copied()
            .map(|comment| {
                let (node, placement) = attach(source, comment, index);
                TrackedComment {
                    comment,
                    node,
                    placement,
                    emitted: false,
                }
            })
            .collect();
        Self { source, comments }
    }

    pub fn suppression(
        &mut self,
        span: Span,
        node_index: &NodeIndex,
    ) -> Result<Option<(Doc, Doc)>, FormatError> {
        if self.comments.is_empty() {
            return Ok(None);
        }
        let Some(index) = self.comments.iter().position(|comment| {
            !comment.emitted
                && comment.placement == Placement::Suppression
                && comment
                    .node
                    .and_then(|node| node_index.record(node))
                    .is_some_and(|node| node.span == span)
        }) else {
            return Ok(None);
        };

        self.comments[index].emitted = true;
        let directive = self.raw_comment(index)?.to_owned();
        for comment in &mut self.comments {
            if !comment.emitted
                && comment.comment.span.start >= span.start
                && comment.comment.span.end <= span.end
            {
                comment.emitted = true;
            }
        }
        let raw = self.slice(span)?.to_owned();
        let separator = if directive.starts_with("//") || self.has_newline(index, span.start) {
            hard_line()
        } else {
            space()
        };
        Ok(Some((concat([text(directive), separator]), text(raw))))
    }

    pub fn wrap(&mut self, span: Span, doc: Doc, index: &NodeIndex) -> Result<Doc, FormatError> {
        if self.comments.is_empty() {
            return Ok(doc);
        }
        let node = find_exact_node(index, span);
        let Some(node) = node else {
            return Ok(doc);
        };
        let leading = self.take(node.id, Placement::Leading)?;
        let leading_inline = self.take(node.id, Placement::LeadingInline)?;
        let trailing = self.take(node.id, Placement::Trailing)?;
        let trailing_own_line = self.take(node.id, Placement::TrailingOwnLine)?;
        Ok(concat([
            comments_before(leading),
            comments_inline_before(leading_inline),
            doc,
            comments_after(trailing),
            comments_on_following_lines(trailing_own_line),
        ]))
    }

    pub fn dangling(&mut self, span: Span, index: &NodeIndex) -> Result<Vec<Doc>, FormatError> {
        if self.comments.is_empty() {
            return Ok(Vec::new());
        }
        let Some(node) = find_exact_node(index, span) else {
            return Ok(Vec::new());
        };
        self.take(node.id, Placement::Dangling)
    }

    pub fn take_within(&mut self, span: Span) -> Result<Vec<(Span, String, bool)>, FormatError> {
        if self.comments.is_empty() {
            return Ok(Vec::new());
        }
        let ids = self
            .comments
            .iter()
            .enumerate()
            .filter(|(_, comment)| {
                !comment.emitted
                    && comment.placement != Placement::Suppression
                    && comment.comment.span.start >= span.start
                    && comment.comment.span.end <= span.end
            })
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        let mut comments = Vec::with_capacity(ids.len());
        for id in ids {
            let comment = self.comments[id].comment;
            let raw = self.raw_comment(id)?.to_owned();
            self.comments[id].emitted = true;
            comments.push((comment.span, raw, comment.is_line()));
        }
        Ok(comments)
    }

    pub fn finish(&self) -> Result<(), FormatError> {
        let remaining = self
            .comments
            .iter()
            .enumerate()
            .filter(|(_, comment)| !comment.emitted)
            .map(|(id, comment)| {
                format!(
                    "#{id}@{}..{}",
                    comment.comment.span.start, comment.comment.span.end
                )
            })
            .collect::<Vec<_>>();
        if remaining.is_empty() {
            Ok(())
        } else {
            Err(FormatError::internal(format!(
                "comments were not emitted exactly once: {}",
                remaining.join(", ")
            )))
        }
    }

    fn take(&mut self, node: usize, placement: Placement) -> Result<Vec<Doc>, FormatError> {
        let ids = self
            .comments
            .iter()
            .enumerate()
            .filter(|(_, comment)| {
                !comment.emitted && comment.node == Some(node) && comment.placement == placement
            })
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        let mut docs = Vec::with_capacity(ids.len());
        for id in ids {
            let raw = self.raw_comment(id)?.to_owned();
            let comment = if placement == Placement::Trailing && self.comments[id].comment.is_line()
            {
                line_suffix(text(raw))
            } else {
                text(raw)
            };
            self.comments[id].emitted = true;
            docs.push(comment);
        }
        Ok(docs)
    }

    fn raw_comment(&self, id: usize) -> Result<&'a str, FormatError> {
        self.slice(self.comments[id].comment.span)
    }

    fn slice(&self, span: Span) -> Result<&'a str, FormatError> {
        self.source
            .get(span.start as usize..span.end as usize)
            .ok_or_else(|| FormatError::internal("comment span is outside the source text"))
    }

    fn has_newline(&self, comment: usize, next_start: u32) -> bool {
        let end = self.comments[comment].comment.span.end as usize;
        self.source
            .get(end..next_start as usize)
            .is_some_and(|text| text.contains(['\n', '\r']))
    }
}

fn attach(source: &str, comment: Comment, index: &NodeIndex) -> (Option<usize>, Placement) {
    if is_suppression(source, comment)
        && let Some(node) = next_node(index, comment.span.end)
    {
        return (Some(node.id), Placement::Suppression);
    }

    if let Some(node) = trailing_node(source, index, comment.span.start) {
        return (Some(node.id), Placement::Trailing);
    }
    if let Some(node) = next_node(index, comment.span.end) {
        let placement = if comment.is_block()
            && source
                .get(comment.span.end as usize..node.span.start as usize)
                .is_some_and(|gap| !gap.contains(['\n', '\r']))
        {
            Placement::LeadingInline
        } else {
            Placement::Leading
        };
        return (Some(node.id), placement);
    }
    if let Some(node) = dangling_node(index, comment.span) {
        return (Some(node.id), Placement::Dangling);
    }
    if let Some(node) = previous_statement(index, comment.span.start) {
        return (Some(node.id), Placement::TrailingOwnLine);
    }
    if let Some(node) = index
        .records()
        .iter()
        .find(|node| node.category == NodeCategory::Program)
    {
        return (Some(node.id), Placement::Dangling);
    }
    (None, Placement::Unattached)
}

fn is_suppression(source: &str, comment: Comment) -> bool {
    let span = comment.content_span();
    source
        .get(span.start as usize..span.end as usize)
        .is_some_and(|content| content.trim() == "worsier-ignore")
}

fn previous_statement(index: &NodeIndex, before: u32) -> Option<&NodeRecord> {
    let node = index
        .records()
        .iter()
        .filter(|node| node.category == NodeCategory::Statement && node.span.end <= before)
        .max_by_key(|node| (node.span.end, node.span.end - node.span.start))?;
    index.preferred_record_with_span(node.span)
}

fn next_node(index: &NodeIndex, after: u32) -> Option<&NodeRecord> {
    let node = index
        .records()
        .iter()
        .filter(|node| node.category != NodeCategory::Program && node.span.start >= after)
        .min_by_key(|node| {
            (
                node.span.start,
                std::cmp::Reverse(category_priority(node.category)),
                std::cmp::Reverse(node.span.end - node.span.start),
            )
        })?;
    let start = node.span.start;
    let node = index
        .records()
        .iter()
        .filter(|node| node.category != NodeCategory::Program && node.span.start == start)
        .max_by_key(|node| {
            (
                category_priority(node.category),
                node.span.end - node.span.start,
            )
        })?;
    index.preferred_record_with_span(node.span)
}

fn trailing_node<'a>(
    source: &str,
    index: &'a NodeIndex,
    comment_start: u32,
) -> Option<&'a NodeRecord> {
    let node = index
        .records()
        .iter()
        .filter(|node| {
            if node.category == NodeCategory::Program || node.span.end > comment_start {
                return false;
            }
            source
                .get(node.span.end as usize..comment_start as usize)
                .is_some_and(|gap| gap.trim().is_empty())
        })
        .max_by_key(|node| {
            (
                node.span.end,
                category_priority(node.category),
                node.span.end - node.span.start,
            )
        })?;
    index.preferred_record_with_span(node.span)
}

fn dangling_node(index: &NodeIndex, comment: Span) -> Option<&NodeRecord> {
    let node = index
        .records()
        .iter()
        .filter(|node| {
            node.category == NodeCategory::Container
                && node.span.start <= comment.start
                && node.span.end >= comment.end
        })
        .min_by_key(|node| node.span.end - node.span.start)?;
    index.preferred_record_with_span(node.span)
}

fn find_exact_node(index: &NodeIndex, span: Span) -> Option<&NodeRecord> {
    index.preferred_record_with_span(span)
}

fn comments_before(comments: Vec<Doc>) -> Doc {
    let mut docs = Vec::new();
    for comment in comments {
        docs.extend([comment, hard_line()]);
    }
    concat(docs)
}

fn comments_inline_before(comments: Vec<Doc>) -> Doc {
    let mut docs = Vec::new();
    for comment in comments {
        docs.extend([comment, space()]);
    }
    concat(docs)
}

fn comments_after(comments: Vec<Doc>) -> Doc {
    let mut docs = Vec::new();
    for comment in comments {
        docs.extend([space(), comment]);
    }
    concat(docs)
}

fn comments_on_following_lines(comments: Vec<Doc>) -> Doc {
    let mut docs = Vec::new();
    for comment in comments {
        docs.extend([hard_line(), comment]);
    }
    concat(docs)
}

#[cfg(test)]
mod tests {
    use oxc_allocator::Allocator;
    use oxc_parser::{Parser, config::TokensParserConfig};
    use oxc_span::SourceType;

    use super::CommentTracker;
    use crate::index::NodeIndex;

    #[test]
    fn attaches_every_comment_to_a_node() {
        let allocator = Allocator::default();
        let source = "// leading\nconst value = { /* dangling */ key: 1 }; // trailing";
        let parsed = Parser::new(&allocator, source, SourceType::ts())
            .with_config(TokensParserConfig)
            .parse();
        let index = NodeIndex::build(&parsed.program, &parsed.tokens);
        let tracker = CommentTracker::new(source, &parsed.program.comments, &index);

        assert!(
            tracker
                .comments
                .iter()
                .all(|comment| comment.node.is_some())
        );
    }

    #[test]
    fn recognizes_suppression_directive() {
        let allocator = Allocator::default();
        let source = "// worsier-ignore\nconst value={ key: 1 };";
        let parsed = Parser::new(&allocator, source, SourceType::ts())
            .with_config(TokensParserConfig)
            .parse();
        let index = NodeIndex::build(&parsed.program, &parsed.tokens);
        let tracker = CommentTracker::new(source, &parsed.program.comments, &index);

        assert!(tracker.comments.iter().any(|comment| {
            comment.placement == super::Placement::Suppression && comment.node.is_some()
        }));
    }
}

use super::{
    ClassBody, ClassElement, ClassMemberSpacingMode, Comment, CommentSpacing, ContentEq, Edit,
    ExpandedLayout, FormatError, GetSpan, HashSet, Kind, LayoutIndents, LineBreakIndex,
    ObjectLayout, ParentItem, Span, StatementList, SwitchLayout, Token,
    append_object_boundary_edit, boundary_is_inline, boundary_shares_directive_target_line,
    brace_tokens, comments_in_span, first_token_in_span, source_slice, statement_syntax_span,
    tokens_in_span,
};

#[derive(Debug)]
pub(super) struct ClassMemberLayout {
    pub span: Span,
    pub boundary_start: u32,
    pub multiline: bool,
    kind: ClassMemberKind,
    preserve_before: bool,
}

#[derive(Debug, Eq, PartialEq)]
enum ClassMemberKind {
    Field,
    MethodLike,
    OverloadContinuation,
}

#[derive(Debug)]
pub(super) struct ClassLayout {
    open: Span,
    close: Span,
    pub members: Vec<ClassMemberLayout>,
    pub layout_parent: Option<ExpandedLayout>,
    pub parent_item: Option<ParentItem>,
    original_multiline: bool,
    pub expanded: bool,
    closing_start: u32,
    preserve_closing: bool,
}

impl ClassLayout {
    #[allow(
        clippy::too_many_arguments,
        reason = "class collection shares syntax and layout context"
    )]
    pub(super) fn collect(
        source: &str,
        tokens: &[Token],
        lines: &LineBreakIndex,
        directive_lines: &HashSet<u32>,
        body: &ClassBody<'_>,
        layout_parent: Option<ExpandedLayout>,
        parent_item: Option<ParentItem>,
    ) -> Option<Self> {
        let (open, close) = brace_tokens(tokens, body.span)?;
        let mut previous_end = open.end;
        let members = body
            .body
            .iter()
            .enumerate()
            .map(|(index, member)| {
                // The parser can attach a leading ASI guard to the preceding field.
                // Keep that guard with the next member so repeated formatting is stable.
                let mut span = statement_syntax_span(source, tokens, member_span(member));
                let gap = tokens_in_span(tokens, Span::new(previous_end, span.start));
                if first_token_in_span(tokens, span)
                    .is_some_and(|token| matches!(token.kind(), Kind::LBrack | Kind::Star))
                    && let Some(guard) = gap.last().filter(|token| token.kind() == Kind::Semicolon)
                    && source_slice(source, Span::new(guard.end(), span.start)).is_ok_and(|gap| {
                        gap.chars().all(|character| matches!(character, ' ' | '\t'))
                    })
                {
                    span.start = guard.start();
                }
                // Empty class elements are absent from the AST. Leave their semicolon tokens intact.
                let boundary_start = boundary_after_tokens(tokens, previous_end, span.start);
                let boundary = Span::new(boundary_start, span.start);
                previous_end = span.end;
                ClassMemberLayout {
                    span,
                    boundary_start,
                    multiline: lines.contains(span),
                    kind: if index > 0 && same_overload_family(&body.body[index - 1], member) {
                        ClassMemberKind::OverloadContinuation
                    } else if matches!(
                        member,
                        ClassElement::PropertyDefinition(_) | ClassElement::TSIndexSignature(_)
                    ) {
                        ClassMemberKind::Field
                    } else {
                        ClassMemberKind::MethodLike
                    },
                    preserve_before: boundary_shares_directive_target_line(
                        source,
                        boundary,
                        directive_lines,
                        Some(lines),
                    ),
                }
            })
            .collect::<Vec<_>>();
        let closing_start = boundary_after_tokens(tokens, previous_end, close.start);
        let closing = Span::new(closing_start, close.start);
        let preserve_closing =
            boundary_shares_directive_target_line(source, closing, directive_lines, Some(lines));
        let expanded = members.len() >= 2
            && (members.iter().any(|member| {
                !member.preserve_before
                    && boundary_is_inline(
                        source,
                        Span::new(member.boundary_start, member.span.start),
                    )
            }) || (!preserve_closing && boundary_is_inline(source, closing)));
        Some(Self {
            open,
            close,
            members,
            layout_parent,
            parent_item,
            original_multiline: lines.contains(body.span),
            expanded,
            closing_start,
            preserve_closing,
        })
    }
}

fn member_span(member: &ClassElement<'_>) -> Span {
    let mut span = member.span();
    let decorators = match member {
        ClassElement::MethodDefinition(member) => member.decorators.as_slice(),
        ClassElement::PropertyDefinition(member) => member.decorators.as_slice(),
        ClassElement::AccessorProperty(member) => member.decorators.as_slice(),
        ClassElement::StaticBlock(_) | ClassElement::TSIndexSignature(_) => &[],
    };
    if let Some(first) = decorators.first() {
        span.start = span.start.min(first.span.start);
    }
    span
}

fn boundary_after_tokens(tokens: &[Token], start: u32, end: u32) -> u32 {
    tokens_in_span(tokens, Span::new(start, end))
        .last()
        .map_or(start, Token::end)
}

fn same_overload_family(previous: &ClassElement<'_>, next: &ClassElement<'_>) -> bool {
    let (ClassElement::MethodDefinition(previous), ClassElement::MethodDefinition(next)) =
        (previous, next)
    else {
        return false;
    };
    if previous.value.body.is_some()
        || previous.kind != next.kind
        || previous.r#static != next.r#static
    {
        return false;
    }
    match (previous.key.static_name(), next.key.static_name()) {
        (Some(previous), Some(next)) => previous == next,
        _ => previous.key.content_eq(&next.key),
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "class boundaries share collected layouts and comment protections"
)]
pub(super) fn append_edits(
    source: &str,
    comments: &[Comment],
    comment_spacing: &CommentSpacing,
    newline: &str,
    lists: &[StatementList],
    switches: &[SwitchLayout],
    objects: &[ObjectLayout],
    mode: ClassMemberSpacingMode,
    indents: &mut LayoutIndents<'_>,
    edits: &mut Vec<Edit>,
) -> Result<(), FormatError> {
    let classes = indents.classes;
    for (class_index, class) in classes.iter().enumerate() {
        if class.members.len() < 2 {
            continue;
        }
        let lines = indents.line_breaks.unwrap();
        for (index, member) in class.members.iter().enumerate() {
            if member.preserve_before {
                continue;
            }
            let boundary = Span::new(member.boundary_start, member.span.start);
            if index == 0 && !(class.expanded && boundary_is_inline(source, boundary)) {
                continue;
            }
            let blank = index > 0
                && member.kind != ClassMemberKind::OverloadContinuation
                && match mode {
                    ClassMemberSpacingMode::Smart => {
                        let previous = &class.members[index - 1];
                        previous.kind != ClassMemberKind::Field
                            || member.kind != ClassMemberKind::Field
                            || previous.multiline
                            || member.multiline
                    }
                    ClassMemberSpacingMode::Separate => true,
                    ClassMemberSpacingMode::Compact | ClassMemberSpacingMode::Off => false,
                };
            let separator = newline.repeat(if blank { 2 } else { 1 });
            let indent =
                indents.class_member_indent(source, lists, switches, objects, class_index, index);
            append_object_boundary_edit(
                source,
                comments,
                comment_spacing,
                boundary,
                &separator,
                &indent,
                index
                    .checked_sub(1)
                    .map(|index| class.members[index].span.end),
                Some(member.span.start),
                lines,
                edits,
            )?;
        }
        let closing = Span::new(class.closing_start, class.close.start);
        if class.expanded && !class.preserve_closing && boundary_is_inline(source, closing) {
            let indent = indents.class_base_indent(source, lists, switches, objects, class_index);
            append_object_boundary_edit(
                source,
                comments,
                comment_spacing,
                closing,
                newline,
                &indent,
                class.members.last().map(|member| member.span.end),
                None,
                lines,
                edits,
            )?;
        }
    }
    Ok(())
}

impl LayoutIndents<'_> {
    fn class_base_indent(
        &mut self,
        source: &str,
        lists: &[StatementList],
        switches: &[SwitchLayout],
        objects: &[ObjectLayout],
        class_index: usize,
    ) -> String {
        if let Some(indent) = &self.class_bases[class_index] {
            return indent.clone();
        }
        let class = &self.classes[class_index];
        let existing_close = class
            .original_multiline
            .then(|| self.standalone_indent(source, class.close.start))
            .flatten();
        let indent = if let Some(indent) = existing_close {
            indent
        } else if let Some(parent) = class.parent_item {
            let parent_start = match parent {
                ParentItem::ClassMember {
                    class_index,
                    member_index,
                } => self.classes[class_index].members[member_index].span.start,
                ParentItem::Statement {
                    list_index,
                    item_index,
                } => lists[list_index].items[item_index].span.start,
                ParentItem::ObjectProperty {
                    object_index,
                    item_index,
                } => objects[object_index].items[item_index].span.start,
                ParentItem::TypeMember {
                    body_index,
                    member_index,
                } => self.type_bodies[body_index].members[member_index].start,
                ParentItem::SwitchCaseLabel { switch_index, .. } => {
                    switches[switch_index].span.start
                }
            };
            let lines = self.line_breaks.unwrap();
            if lines.line_start(class.open.start) == lines.line_start(parent_start) {
                self.parent_item_indent(source, lists, switches, objects, parent)
            } else {
                self.leading_indent(source, class.open.start)
            }
        } else {
            self.leading_indent(source, class.open.start)
        };
        self.class_bases[class_index] = Some(indent.clone());
        indent
    }

    pub(super) fn class_member_indent(
        &mut self,
        source: &str,
        lists: &[StatementList],
        switches: &[SwitchLayout],
        objects: &[ObjectLayout],
        class_index: usize,
        member_index: usize,
    ) -> String {
        if self.class_members[class_index].is_none() {
            let base = self.class_base_indent(source, lists, switches, objects, class_index);
            let class = &self.classes[class_index];
            let fallback = if class.expanded || class.original_multiline {
                format!("{base}  ")
            } else {
                base
            };
            let mut indents = class
                .members
                .iter()
                .map(|member| {
                    self.standalone_indent(source, member.span.start)
                        .or_else(|| {
                            let leading = comments_in_span(
                                self.comments,
                                Span::new(member.boundary_start, member.span.start),
                            )
                            .iter()
                            .find(|comment| comment.position == super::CommentPosition::Leading)?;
                            self.standalone_indent(source, leading.span.start)
                        })
                })
                .collect::<Vec<_>>();
            let mut previous = None;
            for indent in &mut indents {
                if indent.is_some() {
                    previous.clone_from(indent);
                } else {
                    indent.clone_from(&previous);
                }
            }
            let mut following = None;
            for indent in indents.iter_mut().rev() {
                if indent.is_some() {
                    following.clone_from(indent);
                } else {
                    indent.clone_from(&following);
                }
            }
            self.class_members[class_index] = Some(
                indents
                    .into_iter()
                    .map(|indent| indent.unwrap_or_else(|| fallback.clone()))
                    .collect(),
            );
        }
        self.class_members[class_index].as_ref().unwrap()[member_index].clone()
    }
}

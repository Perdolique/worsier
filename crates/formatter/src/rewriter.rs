use std::collections::{HashMap, VecDeque};
use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    BlockStatement, Comment, CommentKind, CommentPosition, Directive, FunctionBody,
    ImportDeclaration, Program, Statement, StaticBlock, SwitchCase, SwitchStatement,
    TSExternalModuleDeclaration, TSGlobalDeclaration, TSModuleBlock, TSNamespaceDeclaration,
    VariableDeclarationKind,
};
use oxc_ast_visit::{
    Visit,
    walk::{
        walk_block_statement, walk_function_body, walk_program, walk_statement, walk_static_block,
        walk_switch_case, walk_switch_statement, walk_ts_external_module_declaration,
        walk_ts_global_declaration, walk_ts_module_block, walk_ts_namespace_declaration,
    },
};
use oxc_parser::{Kind, ParseOptions, Parser, Token, config::TokensParserConfig};
use oxc_span::{ContentEq, GetSpan, SourceType, Span};

use crate::{FormatError, ResolvedConfig, StatementSpacingMode};

const BOM: char = '\u{feff}';

#[cfg(test)]
thread_local! {
    static SPAN_LOOKUP_COMPARISONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static INDENT_RESOLUTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static CORRUPT_REWRITE_FOR_TEST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[inline]
fn span_lookup_comparison(result: bool) -> bool {
    #[cfg(test)]
    SPAN_LOOKUP_COMPARISONS.set(SPAN_LOOKUP_COMPARISONS.get() + 1);
    result
}

#[inline]
fn indent_resolution() {
    #[cfg(test)]
    INDENT_RESOLUTIONS.set(INDENT_RESOLUTIONS.get() + 1);
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

/// Formats static imports and runtime variable declaration boundaries in JavaScript, TypeScript,
/// JSX, or TSX source text.
///
/// # Errors
///
/// Returns a [`FormatError`] when the source type is unsupported, parsing fails, or semantic
/// verification finds that a rewrite changed the program AST.
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
    if !config.import_layout_enabled()
        && config.import_spacing() == StatementSpacingMode::Off
        && config.variable_declaration_spacing() == StatementSpacingMode::Off
    {
        return Ok(None);
    }

    let newline = detect_newline(source);
    let edits = rewrite_edits(
        source,
        &parsed.program,
        &parsed.tokens,
        config.line_width(),
        newline,
        RewriteRules {
            import_layout: config.import_layout_enabled(),
            import_spacing: config.import_spacing(),
            variable_spacing: if source_type.is_typescript_definition() {
                StatementSpacingMode::Off
            } else {
                config.variable_declaration_spacing()
            },
        },
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
    target: StatementTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StatementTarget {
    Other,
    Import {
        multiline: bool,
        spacing: StatementSpacingMode,
    },
    Variable {
        multiline: bool,
        spacing: StatementSpacingMode,
    },
}

#[derive(Clone, Copy, Debug)]
struct RewriteRules {
    import_layout: bool,
    import_spacing: StatementSpacingMode,
    variable_spacing: StatementSpacingMode,
}

#[derive(Clone, Copy, Debug)]
struct ParentItem {
    list_index: usize,
    item_index: usize,
}

#[derive(Clone, Copy, Debug)]
enum ExpandedLayout {
    List(usize),
    Switch(usize),
}

#[derive(Clone, Copy, Debug)]
enum ListContainer {
    Program {
        span: Span,
    },
    Braced {
        span: Span,
        open: Span,
        close: Span,
    },
    SwitchCase {
        span: Span,
        colon: Span,
        switch_index: usize,
    },
}

impl ListContainer {
    const fn span(self) -> Span {
        match self {
            Self::Program { span } | Self::Braced { span, .. } | Self::SwitchCase { span, .. } => {
                span
            }
        }
    }
}

#[derive(Debug)]
struct StatementList {
    items: Vec<StatementShape>,
    container: ListContainer,
    layout_parent: Option<ExpandedLayout>,
    parent_item: Option<ParentItem>,
    original_multiline: bool,
    expanded: bool,
}

#[derive(Debug)]
struct SwitchLayout {
    span: Span,
    open: Span,
    close: Span,
    cases: Vec<Span>,
    layout_parent: Option<ExpandedLayout>,
    parent_item: Option<ParentItem>,
    original_multiline: bool,
    expanded: bool,
}

fn rewrite_edits(
    source: &str,
    program: &Program<'_>,
    tokens: &[Token],
    line_width: u32,
    newline: &str,
    rules: RewriteRules,
) -> Result<Vec<Edit>, FormatError> {
    let mut edits = Vec::new();
    let mut formatted_imports = HashMap::new();

    if rules.import_layout {
        for statement in &program.body {
            let Statement::ImportDeclaration(declaration) = statement else {
                continue;
            };
            let span = statement.span();
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
            formatted_imports.insert(span.start, formatted.multiline);
        }
    }

    let mut collector = StatementCollector {
        source,
        tokens,
        formatted_imports: &formatted_imports,
        import_spacing: rules.import_spacing,
        variable_spacing: rules.variable_spacing,
        ambient_depth: 0,
        current_list: None,
        current_layout: None,
        current_item: None,
        current_switch: None,
        lists: Vec::new(),
        switches: Vec::new(),
    };
    collector.visit_program(program);
    mark_expanded_layouts(&mut collector.lists, &mut collector.switches);
    append_layout_edits(
        source,
        &program.comments,
        newline,
        &collector.lists,
        &collector.switches,
        &mut edits,
    )?;

    edits.sort_by_key(|edit| (edit.start, edit.end));
    Ok(edits)
}

struct StatementCollector<'s> {
    source: &'s str,
    tokens: &'s [Token],
    formatted_imports: &'s HashMap<u32, bool>,
    import_spacing: StatementSpacingMode,
    variable_spacing: StatementSpacingMode,
    ambient_depth: usize,
    current_list: Option<usize>,
    current_layout: Option<ExpandedLayout>,
    current_item: Option<ParentItem>,
    current_switch: Option<usize>,
    lists: Vec<StatementList>,
    switches: Vec<SwitchLayout>,
}

impl StatementCollector<'_> {
    fn record_list<'a>(
        &mut self,
        directives: &[Directive<'a>],
        statements: &[Statement<'a>],
        container: ListContainer,
    ) -> Option<usize> {
        let mut items = directives
            .iter()
            .map(|directive| StatementShape {
                span: directive.span(),
                target: StatementTarget::Other,
            })
            .collect::<Vec<_>>();
        items.extend(
            statements
                .iter()
                .map(|statement| self.statement_shape(statement)),
        );
        items.sort_by_key(|item| item.span.start);
        if items.is_empty() {
            return None;
        }
        let original_multiline =
            source_slice(self.source, container.span()).is_ok_and(contains_line_break);
        let list_index = self.lists.len();
        self.lists.push(StatementList {
            items,
            container,
            layout_parent: self.current_layout,
            parent_item: self.current_item,
            original_multiline,
            expanded: false,
        });
        Some(list_index)
    }

    fn statement_shape(&self, statement: &Statement<'_>) -> StatementShape {
        let span = statement.span();
        let target = if matches!(statement, Statement::ImportDeclaration(_)) {
            StatementTarget::Import {
                multiline: self
                    .formatted_imports
                    .get(&span.start)
                    .copied()
                    .unwrap_or_else(|| {
                        source_slice(self.source, span).is_ok_and(contains_line_break)
                    }),
                spacing: self.import_spacing,
            }
        } else if self.ambient_depth == 0
            && matches!(
                statement,
                Statement::VariableDeclaration(declaration)
                    if !declaration.declare
                        && matches!(
                            declaration.kind,
                            VariableDeclarationKind::Var
                                | VariableDeclarationKind::Let
                                | VariableDeclarationKind::Const
                        )
            )
        {
            StatementTarget::Variable {
                multiline: source_slice(self.source, span).is_ok_and(contains_line_break),
                spacing: self.variable_spacing,
            }
        } else {
            StatementTarget::Other
        };
        StatementShape { span, target }
    }

    fn braced_container(&self, span: Span) -> Option<ListContainer> {
        let (open, close) = brace_tokens(self.tokens, span)?;
        Some(ListContainer::Braced { span, open, close })
    }

    fn direct_item(&self, statement: &Statement<'_>) -> Option<ParentItem> {
        let list_index = self.current_list?;
        let statement_span = statement.span();
        let items = &self.lists[list_index].items;
        let item_index = items
            .binary_search_by_key(&statement_span.start, |item| item.span.start)
            .ok()?;
        let item_span = items[item_index].span;
        if item_span.start != statement_span.start || item_span.end != statement_span.end {
            return None;
        }
        Some(ParentItem {
            list_index,
            item_index,
        })
    }
}

impl<'a> Visit<'a> for StatementCollector<'_> {
    fn visit_program(&mut self, program: &Program<'a>) {
        let span = sequence_span(&program.directives, &program.body).unwrap_or(program.span);
        let previous_list = self.current_list;
        let previous_layout = self.current_layout;
        self.current_list = self.record_list(
            &program.directives,
            &program.body,
            ListContainer::Program { span },
        );
        self.current_layout = self.current_list.map(ExpandedLayout::List);
        walk_program(self, program);
        self.current_list = previous_list;
        self.current_layout = previous_layout;
    }

    fn visit_statement(&mut self, statement: &Statement<'a>) {
        let previous_item = self.current_item;
        if let Some(item) = self.direct_item(statement) {
            self.current_item = Some(item);
        }
        walk_statement(self, statement);
        self.current_item = previous_item;
    }

    fn visit_function_body(&mut self, body: &FunctionBody<'a>) {
        let previous_list = self.current_list;
        let previous_layout = self.current_layout;
        self.current_list = self
            .braced_container(body.span)
            .and_then(|container| self.record_list(&body.directives, &body.statements, container));
        self.current_layout = self.current_list.map(ExpandedLayout::List);
        walk_function_body(self, body);
        self.current_list = previous_list;
        self.current_layout = previous_layout;
    }

    fn visit_block_statement(&mut self, block: &BlockStatement<'a>) {
        let previous_list = self.current_list;
        let previous_layout = self.current_layout;
        self.current_list = self
            .braced_container(block.span)
            .and_then(|container| self.record_list(&[], &block.body, container));
        self.current_layout = self.current_list.map(ExpandedLayout::List);
        walk_block_statement(self, block);
        self.current_list = previous_list;
        self.current_layout = previous_layout;
    }

    fn visit_static_block(&mut self, block: &StaticBlock<'a>) {
        let previous_list = self.current_list;
        let previous_layout = self.current_layout;
        self.current_list = self
            .braced_container(block.span)
            .and_then(|container| self.record_list(&[], &block.body, container));
        self.current_layout = self.current_list.map(ExpandedLayout::List);
        walk_static_block(self, block);
        self.current_list = previous_list;
        self.current_layout = previous_layout;
    }

    fn visit_ts_module_block(&mut self, block: &TSModuleBlock<'a>) {
        let previous_list = self.current_list;
        let previous_layout = self.current_layout;
        self.current_list = self
            .braced_container(block.span)
            .and_then(|container| self.record_list(&block.directives, &block.body, container));
        self.current_layout = self.current_list.map(ExpandedLayout::List);
        walk_ts_module_block(self, block);
        self.current_list = previous_list;
        self.current_layout = previous_layout;
    }

    fn visit_switch_statement(&mut self, statement: &SwitchStatement<'a>) {
        let Some((open, close)) = switch_brace_tokens(self.tokens, statement) else {
            walk_switch_statement(self, statement);
            return;
        };
        let switch_index = self.switches.len();
        self.switches.push(SwitchLayout {
            span: statement.span,
            open,
            close,
            cases: statement.cases.iter().map(GetSpan::span).collect(),
            layout_parent: self.current_layout,
            parent_item: self.current_item,
            original_multiline: source_slice(self.source, statement.span)
                .is_ok_and(contains_line_break),
            expanded: false,
        });
        let previous_switch = self.current_switch.replace(switch_index);
        let previous_layout = self
            .current_layout
            .replace(ExpandedLayout::Switch(switch_index));
        walk_switch_statement(self, statement);
        self.current_layout = previous_layout;
        self.current_switch = previous_switch;
    }

    fn visit_switch_case(&mut self, case: &SwitchCase<'a>) {
        let previous_list = self.current_list;
        let previous_layout = self.current_layout;
        self.current_list = if !case.consequent.is_empty()
            && let Some(switch_index) = self.current_switch
            && let Some(colon) = case_colon(self.tokens, case)
        {
            self.record_list(
                &[],
                &case.consequent,
                ListContainer::SwitchCase {
                    span: case.span,
                    colon,
                    switch_index,
                },
            )
        } else {
            None
        };
        self.current_layout = self.current_list.map(ExpandedLayout::List);
        walk_switch_case(self, case);
        self.current_list = previous_list;
        self.current_layout = previous_layout;
    }

    fn visit_ts_external_module_declaration(
        &mut self,
        declaration: &TSExternalModuleDeclaration<'a>,
    ) {
        let ambient = declaration.declare;
        self.ambient_depth += usize::from(ambient);
        walk_ts_external_module_declaration(self, declaration);
        self.ambient_depth -= usize::from(ambient);
    }

    fn visit_ts_namespace_declaration(&mut self, declaration: &TSNamespaceDeclaration<'a>) {
        let ambient = declaration.declare;
        self.ambient_depth += usize::from(ambient);
        walk_ts_namespace_declaration(self, declaration);
        self.ambient_depth -= usize::from(ambient);
    }

    fn visit_ts_global_declaration(&mut self, declaration: &TSGlobalDeclaration<'a>) {
        let ambient = declaration.declare;
        self.ambient_depth += usize::from(ambient);
        walk_ts_global_declaration(self, declaration);
        self.ambient_depth -= usize::from(ambient);
    }
}

fn sequence_span(directives: &[Directive<'_>], statements: &[Statement<'_>]) -> Option<Span> {
    let start = directives
        .first()
        .map(GetSpan::span)
        .or_else(|| statements.first().map(GetSpan::span))?;
    let end = statements
        .last()
        .map(GetSpan::span)
        .or_else(|| directives.last().map(GetSpan::span))?;
    Some(Span::new(start.start, end.end))
}

fn brace_tokens(tokens: &[Token], span: Span) -> Option<(Span, Span)> {
    let tokens = tokens_in_span(tokens, span);
    let open = tokens
        .iter()
        .find(|token| token.kind() == Kind::LCurly)?
        .span();
    let close = tokens
        .iter()
        .rfind(|token| token.kind() == Kind::RCurly)?
        .span();
    Some((open, close))
}

fn switch_brace_tokens(tokens: &[Token], statement: &SwitchStatement<'_>) -> Option<(Span, Span)> {
    let body_span = Span::new(statement.discriminant.span().end, statement.span.end);
    brace_tokens(tokens, body_span)
}

fn case_colon(tokens: &[Token], case: &SwitchCase<'_>) -> Option<Span> {
    let first_statement_start = case
        .consequent
        .first()
        .map_or(case.span.end, |statement| statement.span().start);
    tokens_in_span(tokens, Span::new(case.span.start, first_statement_start))
        .iter()
        .rfind(|token| token.kind() == Kind::Colon)
        .map(Token::span)
}

fn mark_expanded_layouts(lists: &mut [StatementList], switches: &mut [SwitchLayout]) {
    let mut pending = VecDeque::new();
    for (list_index, list) in lists.iter_mut().enumerate() {
        list.expanded = !list.original_multiline
            && list.items.len() >= 2
            && list.items.iter().any(|item| {
                matches!(
                    item.target,
                    StatementTarget::Variable { spacing, .. }
                        if spacing != StatementSpacingMode::Off
                )
            });
        if list.expanded {
            pending.push_back(ExpandedLayout::List(list_index));
        }
    }

    while let Some(layout) = pending.pop_front() {
        let parent = match layout {
            ExpandedLayout::List(list_index) => lists[list_index].layout_parent,
            ExpandedLayout::Switch(switch_index) => switches[switch_index].layout_parent,
        };
        match parent {
            Some(ExpandedLayout::List(list_index)) => {
                let parent_list = &mut lists[list_index];
                if !parent_list.expanded && !parent_list.original_multiline {
                    parent_list.expanded = true;
                    pending.push_back(ExpandedLayout::List(list_index));
                }
            }
            Some(ExpandedLayout::Switch(switch_index)) => {
                let parent_switch = &mut switches[switch_index];
                if !parent_switch.expanded && !parent_switch.original_multiline {
                    parent_switch.expanded = true;
                    pending.push_back(ExpandedLayout::Switch(switch_index));
                }
            }
            None => {}
        }
    }
}

fn append_layout_edits(
    source: &str,
    comments: &[Comment],
    newline: &str,
    lists: &[StatementList],
    switches: &[SwitchLayout],
    edits: &mut Vec<Edit>,
) -> Result<(), FormatError> {
    let mut indents = LayoutIndents::new(lists.len(), switches.len());
    append_list_layout_edits(
        source,
        comments,
        newline,
        lists,
        switches,
        &mut indents,
        edits,
    )?;
    append_switch_layout_edits(
        source,
        comments,
        newline,
        lists,
        switches,
        &mut indents,
        edits,
    )
}

fn append_list_layout_edits(
    source: &str,
    comments: &[Comment],
    newline: &str,
    lists: &[StatementList],
    switches: &[SwitchLayout],
    indents: &mut LayoutIndents,
    edits: &mut Vec<Edit>,
) -> Result<(), FormatError> {
    for (list_index, list) in lists.iter().enumerate() {
        let item_indent = if list.expanded {
            indents.item_indent(source, lists, switches, list_index)
        } else {
            String::new()
        };

        if list.expanded {
            match list.container {
                ListContainer::Program { .. } => {}
                ListContainer::Braced { open, .. } => append_boundary_edit(
                    source,
                    comments,
                    Span::new(open.end, list.items[0].span.start),
                    newline,
                    &item_indent,
                    edits,
                )?,
                ListContainer::SwitchCase { colon, .. } => append_boundary_edit(
                    source,
                    comments,
                    Span::new(colon.end, list.items[0].span.start),
                    newline,
                    &item_indent,
                    edits,
                )?,
            }
        }

        for pair in list.items.windows(2) {
            let [previous, next] = pair else {
                unreachable!("windows(2) always contains two statements")
            };
            let Some(blank_line) = boundary_blank_line(previous.target, next.target, list.expanded)
            else {
                continue;
            };
            let separator = if blank_line {
                newline.repeat(2)
            } else {
                newline.to_owned()
            };
            let span = Span::new(previous.span.end, next.span.start);
            let indent = if list.expanded {
                item_indent.clone()
            } else {
                existing_boundary_indent(
                    source,
                    comments,
                    span,
                    previous.span.start,
                    next.span.start,
                )
            };
            append_boundary_edit(source, comments, span, &separator, &indent, edits)?;
        }

        if list.expanded
            && let ListContainer::Braced { close, .. } = list.container
        {
            let base_indent = indents.list_base_indent(source, lists, switches, list_index);
            append_boundary_edit(
                source,
                comments,
                Span::new(list.items.last().unwrap().span.end, close.start),
                newline,
                &base_indent,
                edits,
            )?;
        }
    }

    Ok(())
}

fn append_switch_layout_edits(
    source: &str,
    comments: &[Comment],
    newline: &str,
    lists: &[StatementList],
    switches: &[SwitchLayout],
    indents: &mut LayoutIndents,
    edits: &mut Vec<Edit>,
) -> Result<(), FormatError> {
    for (switch_index, switch) in switches
        .iter()
        .enumerate()
        .filter(|(_, switch)| switch.expanded)
    {
        if switch.cases.is_empty() {
            continue;
        }
        let switch_indent = indents.switch_indent(source, lists, switches, switch_index);
        let case_indent = format!("{switch_indent}  ");
        append_boundary_edit(
            source,
            comments,
            Span::new(switch.open.end, switch.cases[0].start),
            newline,
            &case_indent,
            edits,
        )?;
        for pair in switch.cases.windows(2) {
            append_boundary_edit(
                source,
                comments,
                Span::new(pair[0].end, pair[1].start),
                newline,
                &case_indent,
                edits,
            )?;
        }
        append_boundary_edit(
            source,
            comments,
            Span::new(switch.cases.last().unwrap().end, switch.close.start),
            newline,
            &switch_indent,
            edits,
        )?;
    }

    Ok(())
}

fn boundary_blank_line(
    previous: StatementTarget,
    next: StatementTarget,
    expanded: bool,
) -> Option<bool> {
    let layout_requirement = expanded.then_some(false);
    let previous_requirement = statement_boundary_requirement(previous, next);
    let next_requirement = statement_boundary_requirement(next, previous);

    [layout_requirement, previous_requirement, next_requirement]
        .into_iter()
        .flatten()
        .max()
}

fn statement_boundary_requirement(
    statement: StatementTarget,
    sibling: StatementTarget,
) -> Option<bool> {
    let mode = match statement {
        StatementTarget::Other => return None,
        StatementTarget::Import { spacing, .. } | StatementTarget::Variable { spacing, .. } => {
            spacing
        }
    };
    match mode {
        StatementSpacingMode::Off => None,
        StatementSpacingMode::Compact => Some(false),
        StatementSpacingMode::Separate => Some(!same_single_line_category(statement, sibling)),
    }
}

const fn same_single_line_category(statement: StatementTarget, sibling: StatementTarget) -> bool {
    matches!(
        (statement, sibling),
        (
            StatementTarget::Import {
                multiline: false,
                ..
            },
            StatementTarget::Import {
                multiline: false,
                ..
            }
        ) | (
            StatementTarget::Variable {
                multiline: false,
                ..
            },
            StatementTarget::Variable {
                multiline: false,
                ..
            }
        )
    )
}

fn append_boundary_edit(
    source: &str,
    comments: &[Comment],
    span: Span,
    separator: &str,
    indent: &str,
    edits: &mut Vec<Edit>,
) -> Result<(), FormatError> {
    let original = source_slice(source, span)?;
    let formatted = format_boundary_separator(source, span, comments, separator, indent)?;
    if original != formatted {
        edits.push(Edit {
            start: span.start,
            end: span.end,
            replacement: formatted,
        });
    }
    Ok(())
}

fn existing_boundary_indent(
    source: &str,
    comments: &[Comment],
    span: Span,
    previous_start: u32,
    next_start: u32,
) -> String {
    let anchor = comments_in_span(comments, span)
        .iter()
        .find(|comment| comment.position == CommentPosition::Leading)
        .map_or(next_start, |comment| comment.span.start);
    let indent = line_indent_at(source, anchor);
    let boundary_is_multiline = source
        .get(span.start as usize..span.end as usize)
        .is_some_and(contains_line_break);
    if !indent.is_empty() || boundary_is_multiline {
        return indent;
    }
    line_indent_at(source, previous_start)
}

struct LayoutIndents {
    list_bases: Vec<Option<String>>,
    list_items: Vec<Option<String>>,
    switches: Vec<Option<String>>,
}

impl LayoutIndents {
    fn new(list_count: usize, switch_count: usize) -> Self {
        Self {
            list_bases: vec![None; list_count],
            list_items: vec![None; list_count],
            switches: vec![None; switch_count],
        }
    }

    fn item_indent(
        &mut self,
        source: &str,
        lists: &[StatementList],
        switches: &[SwitchLayout],
        list_index: usize,
    ) -> String {
        if let Some(indent) = self.list_items[list_index].clone() {
            return indent;
        }
        indent_resolution();
        let indent = match lists[list_index].container {
            ListContainer::Program { .. } => String::new(),
            ListContainer::Braced { .. } => {
                format!(
                    "{}  ",
                    self.list_base_indent(source, lists, switches, list_index)
                )
            }
            ListContainer::SwitchCase { switch_index, .. } => {
                format!(
                    "{}    ",
                    self.switch_indent(source, lists, switches, switch_index)
                )
            }
        };
        self.list_items[list_index] = Some(indent.clone());
        indent
    }

    fn list_base_indent(
        &mut self,
        source: &str,
        lists: &[StatementList],
        switches: &[SwitchLayout],
        list_index: usize,
    ) -> String {
        if let Some(indent) = self.list_bases[list_index].clone() {
            return indent;
        }
        indent_resolution();
        let indent = match lists[list_index].parent_item {
            Some(parent) => {
                if lists[parent.list_index].expanded {
                    self.item_indent(source, lists, switches, parent.list_index)
                } else {
                    line_indent_at(
                        source,
                        lists[parent.list_index].items[parent.item_index].span.start,
                    )
                }
            }
            None => match lists[list_index].container {
                ListContainer::Program { .. } => String::new(),
                ListContainer::Braced { open, .. } => line_indent_at(source, open.start),
                ListContainer::SwitchCase { switch_index, .. } => {
                    self.switch_indent(source, lists, switches, switch_index)
                }
            },
        };
        self.list_bases[list_index] = Some(indent.clone());
        indent
    }

    fn switch_indent(
        &mut self,
        source: &str,
        lists: &[StatementList],
        switches: &[SwitchLayout],
        switch_index: usize,
    ) -> String {
        if let Some(indent) = self.switches[switch_index].clone() {
            return indent;
        }
        indent_resolution();
        let switch = &switches[switch_index];
        let indent = if let Some(parent) = switch.parent_item {
            if lists[parent.list_index].expanded {
                self.item_indent(source, lists, switches, parent.list_index)
            } else {
                line_indent_at(
                    source,
                    lists[parent.list_index].items[parent.item_index].span.start,
                )
            }
        } else {
            line_indent_at(source, switch.span.start)
        };
        self.switches[switch_index] = Some(indent.clone());
        indent
    }
}

fn line_indent_at(source: &str, offset: u32) -> String {
    let prefix = source.get(..offset as usize).unwrap_or(source);
    let line = prefix.rsplit_once('\n').map_or(prefix, |(_, line)| line);
    if line
        .chars()
        .all(|character| matches!(character, ' ' | '\t' | '\r'))
    {
        line.trim_end_matches('\r').to_owned()
    } else {
        String::new()
    }
}

fn format_boundary_separator(
    source: &str,
    span: Span,
    comments: &[Comment],
    statement_separator: &str,
    indent: &str,
) -> Result<String, FormatError> {
    let boundary_comments = comments_in_span(comments, span);
    if boundary_comments.is_empty() {
        let separator = source_slice(source, span)?;
        return if separator.chars().all(char::is_whitespace) {
            Ok(format!("{statement_separator}{indent}"))
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
    output.push_str(indent);
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

    use super::{
        CORRUPT_REWRITE_FOR_TEST, INDENT_RESOLUTIONS, SPAN_LOOKUP_COMPARISONS, parse, source_type,
        verify,
    };
    use crate::{
        FormatConfig, RulesConfig, StatementSpacingConfig, StatementSpacingMode, format_text,
        resolve_config,
    };

    fn format(source: &str) -> String {
        format_with(source, FormatConfig::default())
    }

    fn format_with(source: &str, config: FormatConfig) -> String {
        format_file_with("sample.ts", source, config)
    }

    fn format_file_with(file_name: &str, source: &str, config: FormatConfig) -> String {
        let config = resolve_config(config).unwrap();
        format_text(Path::new(file_name), source, &config)
            .unwrap()
            .unwrap_or_else(|| source.to_owned())
    }

    fn format_with_rules(
        source: &str,
        import_layout: bool,
        imports: StatementSpacingMode,
        variable_declarations: StatementSpacingMode,
    ) -> String {
        format_with(
            source,
            FormatConfig {
                rules: RulesConfig {
                    import_layout,
                    statement_spacing: StatementSpacingConfig {
                        imports,
                        variable_declarations,
                    },
                },
                ..FormatConfig::default()
            },
        )
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
    fn applies_all_import_spacing_modes_without_requiring_import_layout() {
        let source = "import{a}from'x';\n\n\nimport{b}from'y';\n\n\nrun();";

        assert_eq!(
            format_with_rules(
                source,
                false,
                StatementSpacingMode::Separate,
                StatementSpacingMode::Off,
            ),
            "import{a}from'x';\nimport{b}from'y';\n\nrun();"
        );
        assert_eq!(
            format_with_rules(
                source,
                false,
                StatementSpacingMode::Compact,
                StatementSpacingMode::Off,
            ),
            "import{a}from'x';\nimport{b}from'y';\nrun();"
        );
        assert_eq!(
            format_with_rules(
                source,
                false,
                StatementSpacingMode::Off,
                StatementSpacingMode::Off,
            ),
            source
        );
    }

    #[test]
    fn import_spacing_uses_the_final_shape_only_when_layout_is_enabled() {
        let source = "import{one,two}from'long-package';import value from'x';";
        let without_layout = format_with(
            source,
            FormatConfig {
                line_width: 20,
                rules: RulesConfig {
                    import_layout: false,
                    statement_spacing: StatementSpacingConfig {
                        imports: StatementSpacingMode::Separate,
                        variable_declarations: StatementSpacingMode::Off,
                    },
                },
                ..FormatConfig::default()
            },
        );
        assert_eq!(
            without_layout,
            "import{one,two}from'long-package';\nimport value from'x';"
        );

        let with_layout = format_with(
            source,
            FormatConfig {
                line_width: 20,
                rules: RulesConfig {
                    import_layout: true,
                    statement_spacing: StatementSpacingConfig {
                        imports: StatementSpacingMode::Separate,
                        variable_declarations: StatementSpacingMode::Off,
                    },
                },
                ..FormatConfig::default()
            },
        );
        assert_eq!(
            with_layout,
            "import {\n  one,\n  two\n} from 'long-package';\n\nimport value from 'x';"
        );
    }

    #[test]
    fn import_layout_still_runs_when_import_spacing_is_off() {
        let source = "import{a}from'x';\n\n\nrun();";
        assert_eq!(
            format_with_rules(
                source,
                true,
                StatementSpacingMode::Off,
                StatementSpacingMode::Off,
            ),
            "import { a } from 'x';\n\n\nrun();"
        );
    }

    #[test]
    fn compact_import_spacing_ignores_single_and_multiline_shapes() {
        let source = "import {\n  one,\n  two\n} from 'pkg';\n\n\nimport value from 'other';";
        assert_eq!(
            format_with_rules(
                source,
                false,
                StatementSpacingMode::Compact,
                StatementSpacingMode::Off,
            ),
            "import {\n  one,\n  two\n} from 'pkg';\nimport value from 'other';"
        );
    }

    #[test]
    fn preserves_every_non_import_byte_and_the_eof_shape() {
        let source = "import{a,b}from\"pkg\";\nconst odd={ untouched :true,nested:[1,  2] };\nexport{odd};\nconst quote=\"double\"";
        let output = format(source);
        assert_eq!(
            output,
            "import { a, b } from \"pkg\";\n\nconst odd={ untouched :true,nested:[1,  2] };\n\nexport{odd};\n\nconst quote=\"double\""
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
    fn disabling_all_rules_preserves_the_complete_source() {
        let source = "import{a,b}from'x';const value={raw:true};";
        let config = resolve_config(FormatConfig {
            rules: RulesConfig {
                import_layout: false,
                statement_spacing: StatementSpacingConfig {
                    imports: StatementSpacingMode::Off,
                    variable_declarations: StatementSpacingMode::Off,
                },
            },
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
    fn nested_layout_indent_resolution_stays_linear() {
        let depth = 128;
        let mut source = String::new();
        for index in 0..depth {
            write!(source, "function f{index}(){{").unwrap();
        }
        source.push_str("const value=1;work();");
        for _ in 0..depth {
            source.push_str("finish();}");
        }
        let config = resolve_config(FormatConfig {
            verify_ast: false,
            ..FormatConfig::default()
        })
        .unwrap();

        INDENT_RESOLUTIONS.set(0);
        format_text(Path::new("nested-functions.ts"), &source, &config).unwrap();
        let resolutions = INDENT_RESOLUTIONS.get();
        assert!(resolutions > depth);
        assert!(
            resolutions < depth * 4,
            "indent resolution performed {resolutions} steps for {depth} nested functions"
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
    fn applies_the_variable_spacing_matrix_in_both_directions() {
        let source = "const a=1;let b=2;\nvar c={\n x:1\n};const d=4;\nrun();let e=5;";
        let expected =
            "const a=1;\nlet b=2;\n\nvar c={\n x:1\n};\n\nconst d=4;\n\nrun();\n\nlet e=5;";
        assert_eq!(format(source), expected);
        assert_eq!(format(expected), expected);
    }

    #[test]
    fn applies_all_variable_spacing_modes_and_only_active_modes_unfold_blocks() {
        let source = "function f(){const a=1;work();}";

        assert_eq!(
            format_with_rules(
                source,
                false,
                StatementSpacingMode::Off,
                StatementSpacingMode::Separate,
            ),
            "function f(){\n  const a=1;\n\n  work();\n}"
        );
        assert_eq!(
            format_with_rules(
                source,
                false,
                StatementSpacingMode::Off,
                StatementSpacingMode::Compact,
            ),
            "function f(){\n  const a=1;\n  work();\n}"
        );
        assert_eq!(
            format_with_rules(
                source,
                false,
                StatementSpacingMode::Off,
                StatementSpacingMode::Off,
            ),
            source
        );
    }

    #[test]
    fn compact_variable_spacing_ignores_single_and_multiline_shapes() {
        let source = "const first={\n  value:1\n};\n\n\nlet second=2;\n\n\nwork();";
        assert_eq!(
            format_with_rules(
                source,
                false,
                StatementSpacingMode::Off,
                StatementSpacingMode::Compact,
            ),
            "const first={\n  value:1\n};\nlet second=2;\nwork();"
        );
    }

    #[test]
    fn combines_both_sides_of_import_variable_boundaries() {
        let forward = "import value from 'pkg';const value=1;";
        let reverse = "const value=1;import value from 'pkg';";
        for source in [forward, reverse] {
            let blank_line = source.replacen(';', ";\n\n", 1);
            let one_line = source.replacen(';', ";\n", 1);

            assert_eq!(
                format_with_rules(
                    source,
                    false,
                    StatementSpacingMode::Separate,
                    StatementSpacingMode::Compact,
                ),
                blank_line
            );
            assert_eq!(
                format_with_rules(
                    source,
                    false,
                    StatementSpacingMode::Compact,
                    StatementSpacingMode::Compact,
                ),
                one_line
            );
            assert_eq!(
                format_with_rules(
                    source,
                    false,
                    StatementSpacingMode::Off,
                    StatementSpacingMode::Compact,
                ),
                one_line
            );
            assert_eq!(
                format_with_rules(
                    source,
                    false,
                    StatementSpacingMode::Compact,
                    StatementSpacingMode::Off,
                ),
                one_line
            );
            assert_eq!(
                format_with_rules(
                    source,
                    false,
                    StatementSpacingMode::Off,
                    StatementSpacingMode::Off,
                ),
                source
            );
        }
    }

    #[test]
    fn preserves_variable_declaration_contents_and_line_width_scope() {
        let source = "const { a,\n b }: Value = make(  1,2), other=[1,  2];let next=3;";
        let output = format_with(
            source,
            FormatConfig {
                line_width: 1,
                ..FormatConfig::default()
            },
        );
        assert_eq!(
            output,
            "const { a,\n b }: Value = make(  1,2), other=[1,  2];\n\nlet next=3;"
        );
        assert!(output.contains("make(  1,2), other=[1,  2]"));
    }

    #[test]
    fn keeps_import_layout_and_statement_spacing_independent() {
        let source = "import{a}from'x';const b=1;let c=2;run();";
        let variables_only = format_with(
            source,
            FormatConfig {
                rules: RulesConfig {
                    import_layout: false,
                    statement_spacing: StatementSpacingConfig {
                        imports: StatementSpacingMode::Off,
                        variable_declarations: StatementSpacingMode::Separate,
                    },
                },
                ..FormatConfig::default()
            },
        );
        assert_eq!(
            variables_only,
            "import{a}from'x';\n\nconst b=1;\nlet c=2;\n\nrun();"
        );

        let imports_only = format_with(
            source,
            FormatConfig {
                rules: RulesConfig {
                    import_layout: true,
                    statement_spacing: StatementSpacingConfig {
                        imports: StatementSpacingMode::Separate,
                        variable_declarations: StatementSpacingMode::Off,
                    },
                },
                ..FormatConfig::default()
            },
        );
        assert_eq!(
            imports_only,
            "import { a } from 'x';\n\nconst b=1;let c=2;run();"
        );

        assert_eq!(
            format("const before=1;import{value}from'pkg';const after=2;"),
            "const before=1;\n\nimport { value } from 'pkg';\n\nconst after=2;"
        );
    }

    #[test]
    fn unfolds_inline_blocks_and_cascades_two_space_indentation() {
        let source = "function outer(){if(ok){const a=1;work();}finish();}";
        let expected =
            "function outer(){\n  if(ok){\n    const a=1;\n\n    work();\n  }\n  finish();\n}";
        assert_eq!(format(source), expected);
        assert_eq!(format(expected), expected);

        assert_eq!(
            format("function f(){const a=1;}"),
            "function f(){const a=1;}"
        );
        assert_eq!(
            format("function f(){work();finish();}"),
            "function f(){work();finish();}"
        );

        let multiline_parent = "function outer() {\n  if(ok){const a=1;work();}\n}";
        let multiline_expected =
            "function outer() {\n  if(ok){\n    const a=1;\n\n    work();\n  }\n}";
        assert_eq!(format(multiline_parent), multiline_expected);
    }

    #[test]
    fn unfolds_switch_cases_with_label_and_consequent_indentation() {
        let source = "switch(x){case 1: const a=1;run();case 2: let b=2;done();}";
        let expected = "switch(x){\n  case 1:\n    const a=1;\n\n    run();\n  case 2:\n    let b=2;\n\n    done();\n}";
        assert_eq!(format(source), expected);
        assert_eq!(format(expected), expected);

        let nested = "function f(){switch(x){case 1: const a=1;run();}finish();}";
        let nested_expected = "function f(){\n  switch(x){\n    case 1:\n      const a=1;\n\n      run();\n  }\n  finish();\n}";
        assert_eq!(format(nested), nested_expected);

        let object_discriminant = "switch({x:1}){case 1: const a=1;run();}";
        let object_expected = "switch({x:1}){\n  case 1:\n    const a=1;\n\n    run();\n}";
        assert_eq!(format(object_discriminant), object_expected);
    }

    #[test]
    fn compact_mode_unfolds_nested_blocks_and_switch_cases_without_blank_lines() {
        let source =
            "function outer(){if(ok){const a=1;work();}switch(x){case 1:let b=2;done();}finish();}";
        let expected = "function outer(){\n  if(ok){\n    const a=1;\n    work();\n  }\n  switch(x){\n    case 1:\n      let b=2;\n      done();\n  }\n  finish();\n}";
        let output = format_with_rules(
            source,
            false,
            StatementSpacingMode::Off,
            StatementSpacingMode::Compact,
        );
        assert_eq!(output, expected);
        assert_eq!(
            format_with_rules(
                &output,
                false,
                StatementSpacingMode::Off,
                StatementSpacingMode::Compact,
            ),
            output
        );
    }

    #[test]
    fn compact_spacing_preserves_comments_bom_crlf_and_eof_shape() {
        let source = "\u{feff}import value from'pkg'; // trailing\r\n\r\n// leading\r\nconst a=1;\r\n\r\nwork();";
        let output = format_with_rules(
            source,
            false,
            StatementSpacingMode::Compact,
            StatementSpacingMode::Compact,
        );
        assert_eq!(
            output,
            "\u{feff}import value from'pkg'; // trailing\r\n// leading\r\nconst a=1;\r\nwork();"
        );
        assert!(!output.replace("\r\n", "").contains('\n'));
        assert!(!output.ends_with('\n'));
        assert_eq!(
            format_with_rules(
                &output,
                false,
                StatementSpacingMode::Compact,
                StatementSpacingMode::Compact,
            ),
            output
        );
    }

    #[test]
    fn formats_existing_nested_statement_lists_without_reindenting_them() {
        let source = "function f() {\n    before();\n  const a=1;\n      after();\n}";
        let expected = "function f() {\n    before();\n\n  const a=1;\n\n      after();\n}";
        assert_eq!(format(source), expected);

        let containers = "class C { static {\nconst a=1;work();\n} }\nnamespace Live {\nconst b=2;work();\n}\ntry {\nlet c=3;work();\n} catch {\nvar d=4;work();\n} finally {\nconst e=5;work();\n}";
        let output = format(containers);
        for declaration in [
            "const a=1;",
            "const b=2;",
            "let c=3;",
            "var d=4;",
            "const e=5;",
        ] {
            assert!(
                output.contains(&format!("{declaration}\n\nwork();")),
                "{output}"
            );
        }
    }

    #[test]
    fn preserves_existing_indent_when_splitting_inline_boundaries() {
        let source = "function f() {\n  const a=1;work();\n}\nswitch(x) {\n  case 1:\n    let b=2;done();\n}";
        let expected = "function f() {\n  const a=1;\n\n  work();\n}\nswitch(x) {\n  case 1:\n    let b=2;\n\n    done();\n}";
        assert_eq!(format(source), expected);
        assert_eq!(format(expected), expected);
    }

    #[test]
    fn excludes_non_runtime_and_ambient_variable_declarations() {
        let source = "export const exported=1;export let mutable=2;export var legacy=3;declare const ambient:number;using resource=get();await using asyncResource=getAsync();for(let i=0;i<1;i++)work();for(const item of items)use(item);";
        assert_eq!(format(source), source);

        for ambient in [
            "declare namespace Ambient { const value:number; function work():void; }",
            "declare module 'pkg' { const value:number; function work():void; }",
            "declare global { const value:number; function work():void; }",
        ] {
            assert_eq!(format(ambient), ambient);
        }

        let definition = "const first:number;const second:string;";
        for file_name in ["types.d.ts", "types.d.mts", "types.d.cts"] {
            assert_eq!(
                format_file_with(file_name, definition, FormatConfig::default()),
                definition
            );
        }
    }

    #[test]
    fn formats_runtime_variables_in_ordinary_typescript_namespaces() {
        assert_eq!(
            format("namespace Live { const value=1;work(); }"),
            "namespace Live {\n  const value=1;\n\n  work();\n}"
        );
    }

    #[test]
    fn treats_directives_as_non_variable_siblings() {
        assert_eq!(
            format("function f(){'use strict';const value=1;}"),
            "function f(){\n  'use strict';\n\n  const value=1;\n}"
        );
    }

    #[test]
    fn preserves_comments_bom_crlf_semicolons_and_eof_shape_for_variables() {
        let source =
            "\u{feff}function f(){const a=1; // trailing\r\n// leading\r\nlet b=2}\r\nconst c=3";
        let output = format(source);
        assert_eq!(
            output,
            "\u{feff}function f(){const a=1; // trailing\r\n// leading\r\nlet b=2}\r\n\r\nconst c=3"
        );
        assert!(!output.replace("\r\n", "").contains('\n'));
        assert!(!output.ends_with('\n'));
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

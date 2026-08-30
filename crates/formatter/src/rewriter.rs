use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrayAssignmentTarget, ArrayExpression, ArrayExpressionElement, ArrayPattern,
    ArrowFunctionExpression, BlockStatement, CallExpression, ChainElement, ClassBody, ClassElement,
    Comment, CommentKind, CommentPosition, Directive, ExportFromDeclaration,
    ExportNamedDeclaration, Expression, FormalParameters, FunctionBody, ImportDeclaration,
    NewExpression, ObjectAssignmentTarget, ObjectExpression, ObjectPattern, Program, Statement,
    StaticBlock, SwitchCase, SwitchStatement, TSEnumBody, TSExternalModuleDeclaration,
    TSGlobalDeclaration, TSInterfaceBody, TSModuleBlock, TSNamespaceDeclaration, TSTupleElement,
    TSTupleType, TSTypeLiteral, TSTypeParameterDeclaration, VariableDeclarationKind, WithClause,
};
use oxc_ast_visit::{
    Visit,
    walk::{
        walk_accessor_property, walk_array_assignment_target, walk_array_expression,
        walk_array_pattern, walk_arrow_function_expression, walk_block_statement,
        walk_call_expression, walk_class_body, walk_directive, walk_export_from_declaration,
        walk_export_named_declaration, walk_formal_parameters, walk_function_body,
        walk_import_declaration, walk_method_definition, walk_new_expression,
        walk_object_assignment_target, walk_object_expression, walk_object_pattern, walk_program,
        walk_property_definition, walk_statement, walk_static_block, walk_switch_case,
        walk_switch_statement, walk_ts_call_signature_declaration,
        walk_ts_construct_signature_declaration, walk_ts_enum_body,
        walk_ts_external_module_declaration, walk_ts_global_declaration, walk_ts_index_signature,
        walk_ts_interface_body, walk_ts_mapped_type, walk_ts_method_signature,
        walk_ts_module_block, walk_ts_namespace_declaration, walk_ts_property_signature,
        walk_ts_tuple_type, walk_ts_type_literal, walk_ts_type_parameter_declaration,
        walk_with_clause,
    },
};
use oxc_parser::{Kind, ParseOptions, Parser, Token, config::TokensParserConfig};
use oxc_span::{ContentEq, FileExtension, GetSpan, SourceType, Span};

use crate::{
    FormatError, ResolvedConfig, SemicolonMode, StatementSpacingMode, TrailingCommaMode,
    TypeMemberSemicolonConfig,
};

const BOM: char = '\u{feff}';

#[cfg(test)]
thread_local! {
    static SPAN_LOOKUP_COMPARISONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static INDENT_RESOLUTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static IMPORT_MULTILINE_SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static TYPE_ALIAS_MULTILINE_SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static VARIABLE_MULTILINE_SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static TOKEN_PREFLIGHT_PARSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static TOKEN_PARSER_RUNS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static LINE_BREAK_INDEX_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static LINE_BREAK_QUERIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static LINE_START_INDEX_QUERIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static RAW_LINE_START_SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static PARENTHESIS_INDEX_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static PARENTHESIS_LOOKUPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static DEFERRED_IMPORT_BOUNDARY_LOOKUPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
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

#[inline]
fn import_is_multiline(source: &str, span: Span) -> bool {
    #[cfg(test)]
    IMPORT_MULTILINE_SCANS.set(IMPORT_MULTILINE_SCANS.get() + 1);
    source_slice(source, span).is_ok_and(contains_line_break)
}

#[inline]
fn variable_is_multiline(source: &str, span: Span) -> bool {
    #[cfg(test)]
    VARIABLE_MULTILINE_SCANS.set(VARIABLE_MULTILINE_SCANS.get() + 1);
    source_slice(source, span).is_ok_and(contains_line_break)
}

#[inline]
fn type_alias_is_multiline(source: &str, span: Span) -> bool {
    #[cfg(test)]
    TYPE_ALIAS_MULTILINE_SCANS.set(TYPE_ALIAS_MULTILINE_SCANS.get() + 1);
    source_slice(source, span).is_ok_and(contains_line_break)
}

#[inline]
fn contains_deferred_import_boundary(boundaries: &HashSet<u32>, offset: u32) -> bool {
    #[cfg(test)]
    DEFERRED_IMPORT_BOUNDARY_LOOKUPS.set(DEFERRED_IMPORT_BOUNDARY_LOOKUPS.get() + 1);
    boundaries.contains(&offset)
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

#[cfg(test)]
pub(crate) fn corrupt_next_rewrite_for_test() {
    CORRUPT_REWRITE_FOR_TEST.set(true);
}

/// Formats one JavaScript, TypeScript, JSX, or TSX source using an already resolved source type.
///
/// # Errors
///
/// Returns a [`FormatError`] when the source type is unsupported, parsing fails, or semantic
/// verification finds that a rewrite changed the program AST.
pub(crate) fn format_script(
    file_name: &Path,
    source_text: &str,
    source_type: SourceType,
    newline_hint: Option<&'static str>,
    config: &ResolvedConfig,
) -> Result<Option<String>, FormatError> {
    let (bom, source) = source_text
        .strip_prefix(BOM)
        .map_or(("", source_text), |text| ("\u{feff}", text));
    let allocator = Allocator::default();
    let parsed = parse_with_tokens(&allocator, source, source_type)?;

    if parsed.is_flow_language {
        return Err(FormatError::UnsupportedSource {
            message: "Flow is not supported".to_owned(),
        });
    }
    let type_member_semicolons = if source_type.is_typescript() {
        config.type_member_semicolons()
    } else {
        TypeMemberSemicolonConfig::off()
    };
    if !config.import_layout_enabled()
        && config.interface_layout_threshold().is_none()
        && !config.object_property_spacing_enabled()
        && config.control_flow_statement_spacing() == StatementSpacingMode::Off
        && config.import_spacing() == StatementSpacingMode::Off
        && config.multiline_call_statement_spacing() == StatementSpacingMode::Off
        && config.return_statement_spacing() == StatementSpacingMode::Off
        && config.type_alias_spacing() == StatementSpacingMode::Off
        && config.variable_declaration_spacing() == StatementSpacingMode::Off
        && config.trailing_commas() == TrailingCommaMode::Off
        && config.statement_semicolons() == SemicolonMode::Off
        && config.class_member_semicolons() == SemicolonMode::Off
        && type_member_semicolons.is_off()
    {
        return Ok(None);
    }

    let newline = detect_newline(source, newline_hint);
    let single_arrow_comma = single_arrow_comma_rule(source_type);
    let edits = rewrite_edits(
        source,
        &parsed.program,
        &parsed.tokens,
        config.line_width(),
        newline,
        RewriteRules {
            import_layout: config.import_layout_enabled(),
            interface_layout_threshold: config.interface_layout_threshold(),
            object_property_spacing: config.object_property_spacing_enabled(),
            control_flow_spacing: config.control_flow_statement_spacing(),
            import_spacing: config.import_spacing(),
            multiline_call_spacing: config.multiline_call_statement_spacing(),
            return_spacing: config.return_statement_spacing(),
            type_alias_spacing: config.type_alias_spacing(),
            variable_spacing: if source_type.is_typescript_definition() {
                StatementSpacingMode::Off
            } else {
                config.variable_declaration_spacing()
            },
            statement_semicolons: config.statement_semicolons(),
            trailing_commas: config.trailing_commas(),
            single_arrow_comma,
        },
    )?;
    let mut rewritten = if edits.is_empty() {
        None
    } else {
        Some(apply_edits(source, &edits)?)
    };
    rewritten = apply_always_trailing_commas(
        source,
        rewritten,
        &parsed,
        source_type,
        config.trailing_commas(),
        single_arrow_comma,
    )?;

    rewritten = apply_semicolons(
        source,
        rewritten,
        &parsed,
        source_type,
        config.statement_semicolons(),
        config.class_member_semicolons(),
        type_member_semicolons,
    )?;

    let Some(mut rewritten) = rewritten else {
        return Ok(None);
    };
    maybe_corrupt_rewrite_for_test(&mut rewritten);
    if rewritten == source {
        return Ok(None);
    }
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

fn apply_always_trailing_commas(
    source: &str,
    rewritten: Option<String>,
    parsed: &oxc_parser::ParserReturn<'_>,
    source_type: SourceType,
    mode: TrailingCommaMode,
    single_arrow_comma: SingleArrowCommaRule,
) -> Result<Option<String>, FormatError> {
    if mode != TrailingCommaMode::Always {
        return Ok(rewritten);
    }
    let current = rewritten.as_deref().unwrap_or(source);
    let comma_edits = if rewritten.is_none() {
        trailing_comma_edits(
            current,
            &parsed.program,
            &parsed.tokens,
            mode,
            single_arrow_comma,
            false,
        )
    } else {
        let allocator = Allocator::default();
        let parsed = parse_tokens(&allocator, current, source_type)?;
        trailing_comma_edits(
            current,
            &parsed.program,
            &parsed.tokens,
            mode,
            single_arrow_comma,
            false,
        )
    };
    if comma_edits.is_empty() {
        Ok(rewritten)
    } else {
        Ok(Some(apply_edits(current, &comma_edits)?))
    }
}

fn apply_semicolons(
    source: &str,
    rewritten: Option<String>,
    parsed: &oxc_parser::ParserReturn<'_>,
    source_type: SourceType,
    statements: SemicolonMode,
    class_members: SemicolonMode,
    type_members: TypeMemberSemicolonConfig,
) -> Result<Option<String>, FormatError> {
    if statements == SemicolonMode::Off
        && class_members == SemicolonMode::Off
        && type_members.is_off()
    {
        return Ok(rewritten);
    }
    let current = rewritten.as_deref().unwrap_or(source);
    let edits = if rewritten.is_none() {
        semicolon_edits(
            current,
            &parsed.program,
            &parsed.tokens,
            statements,
            class_members,
            type_members,
        )
    } else {
        let allocator = Allocator::default();
        let parsed = parse_tokens(&allocator, current, source_type)?;
        semicolon_edits(
            current,
            &parsed.program,
            &parsed.tokens,
            statements,
            class_members,
            type_members,
        )
    };
    if edits.is_empty() {
        Ok(rewritten)
    } else {
        Ok(Some(apply_edits(current, &edits)?))
    }
}

#[derive(Clone, Copy, Debug)]
enum SingleArrowCommaRule {
    Optional,
    RequiredWithoutConstraint,
    RequiredWithoutConstraintOrDefault,
}

fn single_arrow_comma_rule(source_type: SourceType) -> SingleArrowCommaRule {
    if source_type.is_jsx() {
        SingleArrowCommaRule::RequiredWithoutConstraintOrDefault
    } else if matches!(
        source_type.extension(),
        Some(FileExtension::Mts | FileExtension::Cts)
    ) {
        SingleArrowCommaRule::RequiredWithoutConstraint
    } else {
        SingleArrowCommaRule::Optional
    }
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
    #[cfg(test)]
    TOKEN_PREFLIGHT_PARSES.set(TOKEN_PREFLIGHT_PARSES.get() + 1);
    let preflight_allocator = Allocator::default();
    parse(&preflight_allocator, source, source_type)?;

    parse_tokens(allocator, source, source_type)
}

fn parse_tokens<'a>(
    allocator: &'a Allocator,
    source: &'a str,
    source_type: SourceType,
) -> Result<oxc_parser::ParserReturn<'a>, FormatError> {
    #[cfg(test)]
    TOKEN_PARSER_RUNS.set(TOKEN_PARSER_RUNS.get() + 1);
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

fn semicolon_edits(
    source: &str,
    program: &Program<'_>,
    tokens: &[Token],
    statements: SemicolonMode,
    class_members: SemicolonMode,
    type_members: TypeMemberSemicolonConfig,
) -> Vec<Edit> {
    let mut collector = SemicolonCollector {
        source,
        tokens,
        statements,
        class_members,
        type_members,
        type_member_line_breaks: None,
        current_type_member_mode: None,
        class_index_signatures: HashSet::new(),
        preserved_semicolons: HashSet::new(),
        edits: Vec::new(),
    };
    collector.visit_program(program);
    collector.edits.sort_by_key(|edit| (edit.start, edit.end));
    collector.edits
}

struct SemicolonCollector<'s> {
    source: &'s str,
    tokens: &'s [Token],
    statements: SemicolonMode,
    class_members: SemicolonMode,
    type_members: TypeMemberSemicolonConfig,
    type_member_line_breaks: Option<LineBreakIndex>,
    current_type_member_mode: Option<SemicolonMode>,
    class_index_signatures: HashSet<u32>,
    preserved_semicolons: HashSet<u32>,
    edits: Vec<Edit>,
}

impl SemicolonCollector<'_> {
    fn record(&mut self, span: Span, mode: SemicolonMode) {
        if mode == SemicolonMode::Off {
            return;
        }
        let Some(last) = tokens_in_span(self.tokens, span).last() else {
            return;
        };
        match (mode, last.kind()) {
            (SemicolonMode::Always, Kind::Semicolon | Kind::Comma) => {}
            (SemicolonMode::Always, _) => self.edits.push(Edit {
                start: span.end,
                end: span.end,
                replacement: ";".to_owned(),
            }),
            (SemicolonMode::AsNeeded, Kind::Semicolon)
                if !self.preserved_semicolons.contains(&last.start())
                    && can_remove_trailing_semicolon(self.source, self.tokens, last.span()) =>
            {
                self.edits.push(Edit {
                    start: last.start(),
                    end: last.end(),
                    replacement: String::new(),
                });
            }
            (SemicolonMode::AsNeeded | SemicolonMode::Off, _) => {}
        }
    }

    fn record_mapped_type_member(&mut self, span: Span, mode: SemicolonMode) {
        if mode == SemicolonMode::Off {
            return;
        }
        let tokens = tokens_in_span(self.tokens, span);
        let Some(close_index) = tokens
            .iter()
            .rposition(|token| token.kind() == Kind::RCurly)
        else {
            return;
        };
        let Some(last) = close_index
            .checked_sub(1)
            .and_then(|index| tokens.get(index))
        else {
            return;
        };
        self.record(Span::new(span.start, last.end()), mode);
    }

    fn type_member_mode(&mut self, span: Span) -> SemicolonMode {
        if self.type_members.single_line == self.type_members.multiline {
            return self.type_members.single_line;
        }
        let source = self.source;
        let line_breaks = self
            .type_member_line_breaks
            .get_or_insert_with(|| LineBreakIndex::new(source));
        if line_breaks.contains(span) {
            self.type_members.multiline
        } else {
            self.type_members.single_line
        }
    }

    fn effective_type_member_mode(&mut self, span: Span) -> SemicolonMode {
        self.current_type_member_mode
            .unwrap_or_else(|| self.type_member_mode(span))
    }

    fn append_statement_guards(&mut self, directives: &[Directive<'_>], body: &[Statement<'_>]) {
        if self.statements != SemicolonMode::AsNeeded {
            return;
        }
        for (index, statement) in body.iter().enumerate() {
            if !statement_starts_hazardously(statement, self.tokens) {
                continue;
            }
            let previous = index
                .checked_sub(1)
                .and_then(|index| body.get(index))
                .map(|statement| (statement.span(), statement_semicolon_eligible(statement)))
                .or_else(|| directives.last().map(|directive| (directive.span, true)));
            let Some((previous_span, previous_is_candidate)) = previous else {
                continue;
            };
            self.append_guard(previous_span, previous_is_candidate, statement.span().start);
        }
    }

    fn append_class_guards(&mut self, body: &ClassBody<'_>) {
        if self.class_members != SemicolonMode::AsNeeded {
            return;
        }
        for pair in body.body.windows(2) {
            let [previous, current] = pair else {
                unreachable!("windows(2) always contains two class elements")
            };
            let current_span = current.span();
            if first_token_in_span(self.tokens, current_span)
                .is_none_or(|token| !matches!(token.kind(), Kind::LBrack | Kind::Star))
            {
                continue;
            }
            self.append_guard(
                previous.span(),
                class_element_semicolon_eligible(previous),
                current_span.start,
            );
        }
    }

    fn append_guard(
        &mut self,
        previous_span: Span,
        previous_is_candidate: bool,
        current_start: u32,
    ) {
        let boundary = Span::new(previous_span.end, current_start);
        if tokens_in_span(self.tokens, boundary)
            .iter()
            .any(|token| token.kind() == Kind::Semicolon)
        {
            return;
        }
        let Some(last) = tokens_in_span(self.tokens, previous_span).last() else {
            return;
        };
        if !previous_is_candidate {
            if last.kind() == Kind::Semicolon {
                self.preserved_semicolons.insert(last.start());
            }
            return;
        }
        if last.kind() == Kind::Semicolon
            && !can_remove_trailing_semicolon(self.source, self.tokens, last.span())
        {
            return;
        }
        self.edits.push(Edit {
            start: current_start,
            end: current_start,
            replacement: ";".to_owned(),
        });
    }
}

impl<'a> Visit<'a> for SemicolonCollector<'_> {
    fn visit_program(&mut self, program: &Program<'a>) {
        self.append_statement_guards(&program.directives, &program.body);
        walk_program(self, program);
    }

    fn visit_function_body(&mut self, body: &FunctionBody<'a>) {
        self.append_statement_guards(&body.directives, &body.statements);
        walk_function_body(self, body);
    }

    fn visit_block_statement(&mut self, block: &BlockStatement<'a>) {
        self.append_statement_guards(&[], &block.body);
        walk_block_statement(self, block);
    }

    fn visit_static_block(&mut self, block: &StaticBlock<'a>) {
        self.append_statement_guards(&[], &block.body);
        walk_static_block(self, block);
    }

    fn visit_ts_module_block(&mut self, block: &TSModuleBlock<'a>) {
        self.append_statement_guards(&block.directives, &block.body);
        walk_ts_module_block(self, block);
    }

    fn visit_switch_case(&mut self, case: &SwitchCase<'a>) {
        self.append_statement_guards(&[], &case.consequent);
        walk_switch_case(self, case);
    }

    fn visit_directive(&mut self, directive: &Directive<'a>) {
        self.record(directive.span, self.statements);
        walk_directive(self, directive);
    }

    fn visit_statement(&mut self, statement: &Statement<'a>) {
        if statement_semicolon_eligible(statement) {
            self.record(statement.span(), self.statements);
        }
        walk_statement(self, statement);
    }

    fn visit_class_body(&mut self, body: &ClassBody<'a>) {
        self.append_class_guards(body);
        for element in &body.body {
            if let ClassElement::TSIndexSignature(signature) = element {
                self.class_index_signatures.insert(signature.span.start);
            }
        }
        walk_class_body(self, body);
    }

    fn visit_ts_interface_body(&mut self, body: &TSInterfaceBody<'a>) {
        let mode = self.type_member_mode(body.span);
        let previous = self.current_type_member_mode.replace(mode);
        walk_ts_interface_body(self, body);
        self.current_type_member_mode = previous;
    }

    fn visit_ts_type_literal(&mut self, literal: &TSTypeLiteral<'a>) {
        let mode = self.type_member_mode(literal.span);
        let previous = self.current_type_member_mode.replace(mode);
        walk_ts_type_literal(self, literal);
        self.current_type_member_mode = previous;
    }

    fn visit_property_definition(&mut self, property: &oxc_ast::ast::PropertyDefinition<'a>) {
        self.record(property.span, self.class_members);
        walk_property_definition(self, property);
    }

    fn visit_accessor_property(&mut self, property: &oxc_ast::ast::AccessorProperty<'a>) {
        self.record(property.span, self.class_members);
        walk_accessor_property(self, property);
    }

    fn visit_method_definition(&mut self, method: &oxc_ast::ast::MethodDefinition<'a>) {
        if method.value.body.is_none() {
            self.record(method.span, self.class_members);
        }
        walk_method_definition(self, method);
    }

    fn visit_ts_property_signature(&mut self, signature: &oxc_ast::ast::TSPropertySignature<'a>) {
        let mode = self.effective_type_member_mode(signature.span);
        self.record(signature.span, mode);
        walk_ts_property_signature(self, signature);
    }

    fn visit_ts_index_signature(&mut self, signature: &oxc_ast::ast::TSIndexSignature<'a>) {
        let mode = if self.class_index_signatures.contains(&signature.span.start) {
            self.class_members
        } else {
            self.effective_type_member_mode(signature.span)
        };
        self.record(signature.span, mode);
        walk_ts_index_signature(self, signature);
    }

    fn visit_ts_call_signature_declaration(
        &mut self,
        signature: &oxc_ast::ast::TSCallSignatureDeclaration<'a>,
    ) {
        let mode = self.effective_type_member_mode(signature.span);
        self.record(signature.span, mode);
        walk_ts_call_signature_declaration(self, signature);
    }

    fn visit_ts_construct_signature_declaration(
        &mut self,
        signature: &oxc_ast::ast::TSConstructSignatureDeclaration<'a>,
    ) {
        let mode = self.effective_type_member_mode(signature.span);
        self.record(signature.span, mode);
        walk_ts_construct_signature_declaration(self, signature);
    }

    fn visit_ts_method_signature(&mut self, signature: &oxc_ast::ast::TSMethodSignature<'a>) {
        let mode = self.effective_type_member_mode(signature.span);
        self.record(signature.span, mode);
        walk_ts_method_signature(self, signature);
    }

    fn visit_ts_mapped_type(&mut self, mapped: &oxc_ast::ast::TSMappedType<'a>) {
        let mode = self.type_member_mode(mapped.span);
        self.record_mapped_type_member(mapped.span, mode);
        walk_ts_mapped_type(self, mapped);
    }
}

fn statement_semicolon_eligible(statement: &Statement<'_>) -> bool {
    match statement {
        Statement::VariableDeclaration(_)
        | Statement::ExpressionStatement(_)
        | Statement::DoWhileStatement(_)
        | Statement::BreakStatement(_)
        | Statement::ContinueStatement(_)
        | Statement::ReturnStatement(_)
        | Statement::ThrowStatement(_)
        | Statement::DebuggerStatement(_)
        | Statement::ImportDeclaration(_)
        | Statement::ExportNamedDeclaration(_)
        | Statement::ExportFromDeclaration(_)
        | Statement::ExportAllDeclaration(_)
        | Statement::TSTypeAliasDeclaration(_)
        | Statement::TSImportEqualsDeclaration(_)
        | Statement::TSExportAssignment(_)
        | Statement::TSNamespaceExportDeclaration(_) => true,
        Statement::FunctionDeclaration(function) => function.body.is_none(),
        Statement::TSExternalModuleDeclaration(declaration) => declaration.body.is_none(),
        Statement::ExportDefaultDeclaration(declaration) => declaration.declaration.is_expression(),
        Statement::ExportDeclaration(declaration) => {
            statement_semicolon_eligible(declaration.declaration.as_statement())
        }
        _ => false,
    }
}

fn class_element_semicolon_eligible(element: &ClassElement<'_>) -> bool {
    match element {
        ClassElement::PropertyDefinition(_)
        | ClassElement::AccessorProperty(_)
        | ClassElement::TSIndexSignature(_) => true,
        ClassElement::MethodDefinition(method) => method.value.body.is_none(),
        ClassElement::StaticBlock(_) => false,
    }
}

fn statement_starts_hazardously(statement: &Statement<'_>, tokens: &[Token]) -> bool {
    matches!(statement, Statement::ExpressionStatement(_))
        && first_token_in_span(tokens, statement.span()).is_some_and(|token| {
            matches!(
                token.kind(),
                Kind::LParen
                    | Kind::LBrack
                    | Kind::Plus
                    | Kind::Minus
                    | Kind::RegExp
                    | Kind::NoSubstitutionTemplate
                    | Kind::TemplateHead
                    | Kind::LAngle
            )
        })
}

fn first_token_in_span(tokens: &[Token], span: Span) -> Option<&Token> {
    tokens_in_span(tokens, span).first()
}

fn can_remove_trailing_semicolon(source: &str, tokens: &[Token], semicolon: Span) -> bool {
    let next_index = tokens.partition_point(|token| token.start() < semicolon.end);
    let Some(next) = tokens.get(next_index) else {
        return true;
    };
    matches!(next.kind(), Kind::RCurly | Kind::Eof)
        || source
            .get(semicolon.end as usize..next.start() as usize)
            .is_some_and(contains_line_break)
}

fn trailing_comma_edits(
    source: &str,
    program: &Program<'_>,
    tokens: &[Token],
    mode: TrailingCommaMode,
    single_arrow_comma: SingleArrowCommaRule,
    skip_static_imports: bool,
) -> Vec<Edit> {
    let line_breaks = (mode == TrailingCommaMode::Always).then(|| LineBreakIndex::new(source));
    let mut collector = TrailingCommaCollector {
        tokens,
        mode,
        single_arrow_comma,
        skip_static_imports,
        required_type_parameters: None,
        line_breaks,
        parentheses: ParenthesisIndex::new(tokens),
        edits: Vec::new(),
    };
    collector.visit_program(program);
    collector.edits.sort_by_key(|edit| (edit.start, edit.end));
    collector.edits
}

struct TrailingCommaCollector<'s> {
    tokens: &'s [Token],
    mode: TrailingCommaMode,
    single_arrow_comma: SingleArrowCommaRule,
    skip_static_imports: bool,
    required_type_parameters: Option<Span>,
    line_breaks: Option<LineBreakIndex>,
    parentheses: ParenthesisIndex,
    edits: Vec<Edit>,
}

impl TrailingCommaCollector<'_> {
    fn record_exact_list(
        &mut self,
        span: Span,
        open_kind: Kind,
        close_kind: Kind,
        can_add: bool,
        required: bool,
    ) {
        let Some((open, close)) = exact_delimiters(self.tokens, span, open_kind, close_kind) else {
            return;
        };
        self.record_list(open, close, can_add, required);
    }

    fn record_surrounded_list(
        &mut self,
        span: Span,
        open_kind: Kind,
        close_kind: Kind,
        first_item: Span,
        last_item: Span,
        can_add: bool,
    ) {
        let Some((open, close)) = surrounding_delimiters(
            self.tokens,
            span,
            open_kind,
            close_kind,
            first_item,
            last_item,
        ) else {
            return;
        };
        self.record_list(open, close, can_add, false);
    }

    fn record_parenthesized_list(&mut self, span: Span, item_count: usize) {
        let Some((open, close)) = self.parentheses.final_delimiters(self.tokens, span) else {
            return;
        };
        let can_add = item_count > 1
            || self.line_breaks.as_ref().is_some_and(|line_breaks| {
                let Some(list) = list_tokens(self.tokens, open, close) else {
                    return false;
                };
                let tail_start = list.trailing_comma.map_or(list.last.end, |comma| comma.end);
                line_breaks.contains(Span::new(open.end, list.first.start))
                    || line_breaks.contains(Span::new(tail_start, close.start))
            });
        self.record_list(open, close, can_add, false);
    }

    fn record_list(&mut self, open: Span, close: Span, can_add: bool, required: bool) {
        let Some(list) = list_tokens(self.tokens, open, close) else {
            return;
        };
        let multiline = can_add
            && self
                .line_breaks
                .as_ref()
                .is_some_and(|line_breaks| line_breaks.contains(Span::new(open.start, close.end)));
        let should_have_comma =
            required || (can_add && self.mode == TrailingCommaMode::Always && multiline);

        match (list.trailing_comma, should_have_comma) {
            (None, true) => self.edits.push(Edit {
                start: list.last.end,
                end: list.last.end,
                replacement: ",".to_owned(),
            }),
            (Some(comma), false) => self.edits.push(Edit {
                start: comma.start,
                end: comma.end,
                replacement: String::new(),
            }),
            _ => {}
        }
    }
}

impl<'a> Visit<'a> for TrailingCommaCollector<'_> {
    fn visit_array_expression(&mut self, expression: &ArrayExpression<'a>) {
        if let Some(last) = expression.elements.last()
            && !matches!(last, ArrayExpressionElement::Elision(_))
        {
            self.record_exact_list(expression.span, Kind::LBrack, Kind::RBrack, true, false);
        }
        walk_array_expression(self, expression);
    }

    fn visit_object_expression(&mut self, expression: &ObjectExpression<'a>) {
        if !expression.properties.is_empty() {
            self.record_exact_list(expression.span, Kind::LCurly, Kind::RCurly, true, false);
        }
        walk_object_expression(self, expression);
    }

    fn visit_array_pattern(&mut self, pattern: &ArrayPattern<'a>) {
        if pattern.rest.is_some() {
            self.record_exact_list(pattern.span, Kind::LBrack, Kind::RBrack, false, false);
        } else if matches!(pattern.elements.last(), Some(Some(_))) {
            self.record_exact_list(pattern.span, Kind::LBrack, Kind::RBrack, true, false);
        }
        walk_array_pattern(self, pattern);
    }

    fn visit_object_pattern(&mut self, pattern: &ObjectPattern<'a>) {
        if pattern.rest.is_some() {
            self.record_exact_list(pattern.span, Kind::LCurly, Kind::RCurly, false, false);
        } else if !pattern.properties.is_empty() {
            self.record_exact_list(pattern.span, Kind::LCurly, Kind::RCurly, true, false);
        }
        walk_object_pattern(self, pattern);
    }

    fn visit_array_assignment_target(&mut self, target: &ArrayAssignmentTarget<'a>) {
        if target.rest.is_some() {
            self.record_exact_list(target.span, Kind::LBrack, Kind::RBrack, false, false);
        } else if matches!(target.elements.last(), Some(Some(_))) {
            self.record_exact_list(target.span, Kind::LBrack, Kind::RBrack, true, false);
        }
        walk_array_assignment_target(self, target);
    }

    fn visit_object_assignment_target(&mut self, target: &ObjectAssignmentTarget<'a>) {
        if target.rest.is_some() {
            self.record_exact_list(target.span, Kind::LCurly, Kind::RCurly, false, false);
        } else if !target.properties.is_empty() {
            self.record_exact_list(target.span, Kind::LCurly, Kind::RCurly, true, false);
        }
        walk_object_assignment_target(self, target);
    }

    fn visit_formal_parameters(&mut self, parameters: &FormalParameters<'a>) {
        if parameters.rest.is_some() {
            self.record_exact_list(parameters.span, Kind::LParen, Kind::RParen, false, false);
        } else if !parameters.items.is_empty() {
            self.record_exact_list(parameters.span, Kind::LParen, Kind::RParen, true, false);
        } else if let Some((open, close)) =
            exact_delimiters(self.tokens, parameters.span, Kind::LParen, Kind::RParen)
            && list_tokens(self.tokens, open, close).is_some()
        {
            self.record_list(open, close, true, false);
        }
        walk_formal_parameters(self, parameters);
    }

    fn visit_call_expression(&mut self, expression: &CallExpression<'a>) {
        if !expression.arguments.is_empty() {
            self.record_parenthesized_list(expression.span, expression.arguments.len());
        }
        walk_call_expression(self, expression);
    }

    fn visit_new_expression(&mut self, expression: &NewExpression<'a>) {
        if !expression.arguments.is_empty() {
            self.record_parenthesized_list(expression.span, expression.arguments.len());
        }
        walk_new_expression(self, expression);
    }

    fn visit_import_declaration(&mut self, declaration: &ImportDeclaration<'a>) {
        if self.skip_static_imports {
            return;
        }
        let named_specifiers = declaration.specifiers.as_ref().and_then(|specifiers| {
            let first = specifiers.iter().find(|specifier| {
                matches!(
                    specifier,
                    oxc_ast::ast::ImportDeclarationSpecifier::ImportSpecifier(_)
                )
            })?;
            let last = specifiers.iter().rev().find(|specifier| {
                matches!(
                    specifier,
                    oxc_ast::ast::ImportDeclarationSpecifier::ImportSpecifier(_)
                )
            })?;
            Some((first.span(), last.span()))
        });
        if let Some((first, last)) = named_specifiers {
            self.record_surrounded_list(
                declaration.span,
                Kind::LCurly,
                Kind::RCurly,
                first,
                last,
                true,
            );
        }
        walk_import_declaration(self, declaration);
    }

    fn visit_export_named_declaration(&mut self, declaration: &ExportNamedDeclaration<'a>) {
        if let (Some(first), Some(last)) = (
            declaration.specifiers.first(),
            declaration.specifiers.last(),
        ) {
            self.record_surrounded_list(
                declaration.span,
                Kind::LCurly,
                Kind::RCurly,
                first.span(),
                last.span(),
                true,
            );
        }
        walk_export_named_declaration(self, declaration);
    }

    fn visit_export_from_declaration(&mut self, declaration: &ExportFromDeclaration<'a>) {
        if let (Some(first), Some(last)) = (
            declaration.specifiers.first(),
            declaration.specifiers.last(),
        ) {
            self.record_surrounded_list(
                declaration.span,
                Kind::LCurly,
                Kind::RCurly,
                first.span(),
                last.span(),
                true,
            );
        }
        walk_export_from_declaration(self, declaration);
    }

    fn visit_with_clause(&mut self, clause: &WithClause<'a>) {
        if let (Some(first), Some(last)) = (clause.with_entries.first(), clause.with_entries.last())
        {
            self.record_surrounded_list(
                clause.span,
                Kind::LCurly,
                Kind::RCurly,
                first.span(),
                last.span(),
                true,
            );
        }
        walk_with_clause(self, clause);
    }

    fn visit_ts_enum_body(&mut self, body: &TSEnumBody<'a>) {
        if !body.members.is_empty() {
            self.record_exact_list(body.span, Kind::LCurly, Kind::RCurly, true, false);
        }
        walk_ts_enum_body(self, body);
    }

    fn visit_ts_tuple_type(&mut self, tuple: &TSTupleType<'a>) {
        if let Some(last) = tuple.element_types.last() {
            self.record_exact_list(
                tuple.span,
                Kind::LBrack,
                Kind::RBrack,
                !tuple_element_is_rest(last),
                false,
            );
        }
        walk_ts_tuple_type(self, tuple);
    }

    fn visit_ts_type_parameter_declaration(
        &mut self,
        declaration: &TSTypeParameterDeclaration<'a>,
    ) {
        if !declaration.params.is_empty() {
            let required = self.required_type_parameters == Some(declaration.span);
            self.record_exact_list(declaration.span, Kind::LAngle, Kind::RAngle, true, required);
        }
        walk_ts_type_parameter_declaration(self, declaration);
    }

    fn visit_arrow_function_expression(&mut self, expression: &ArrowFunctionExpression<'a>) {
        let previous = self.required_type_parameters;
        self.required_type_parameters = expression
            .type_parameters
            .as_ref()
            .filter(|parameters| self.single_arrow_comma.is_required(parameters))
            .map(|parameters| parameters.span);
        walk_arrow_function_expression(self, expression);
        self.required_type_parameters = previous;
    }
}

impl SingleArrowCommaRule {
    fn is_required(self, parameters: &TSTypeParameterDeclaration<'_>) -> bool {
        let [parameter] = parameters.params.as_slice() else {
            return false;
        };
        match self {
            Self::Optional => false,
            Self::RequiredWithoutConstraint => parameter.constraint.is_none(),
            Self::RequiredWithoutConstraintOrDefault => {
                parameter.constraint.is_none() && parameter.default.is_none()
            }
        }
    }
}

struct LineBreakIndex {
    offsets: Vec<u32>,
}

impl LineBreakIndex {
    fn new(source: &str) -> Self {
        #[cfg(test)]
        LINE_BREAK_INDEX_BUILDS.set(LINE_BREAK_INDEX_BUILDS.get() + 1);
        let offsets = source
            .bytes()
            .enumerate()
            .filter(|(_, byte)| matches!(byte, b'\n' | b'\r'))
            .map(|(index, _)| u32::try_from(index).unwrap())
            .collect();
        Self { offsets }
    }

    fn contains(&self, span: Span) -> bool {
        #[cfg(test)]
        LINE_BREAK_QUERIES.set(LINE_BREAK_QUERIES.get() + 1);
        let index = self.offsets.partition_point(|offset| *offset < span.start);
        self.offsets
            .get(index)
            .is_some_and(|offset| *offset < span.end)
    }

    fn line_start(&self, offset: u32) -> u32 {
        #[cfg(test)]
        LINE_START_INDEX_QUERIES.set(LINE_START_INDEX_QUERIES.get() + 1);
        let index = self
            .offsets
            .partition_point(|line_break| *line_break < offset);
        index
            .checked_sub(1)
            .and_then(|index| self.offsets.get(index))
            .map_or(0, |line_break| line_break.saturating_add(1))
    }
}

struct ParenthesisIndex {
    open_by_close: HashMap<u32, Span>,
}

impl ParenthesisIndex {
    fn new(tokens: &[Token]) -> Self {
        #[cfg(test)]
        PARENTHESIS_INDEX_BUILDS.set(PARENTHESIS_INDEX_BUILDS.get() + 1);
        let mut stack = Vec::new();
        let mut open_by_close = HashMap::new();
        for token in tokens {
            match token.kind() {
                Kind::LParen => stack.push(token.span()),
                Kind::RParen => {
                    if let Some(open) = stack.pop() {
                        open_by_close.insert(token.start(), open);
                    }
                }
                _ => {}
            }
        }
        Self { open_by_close }
    }

    fn final_delimiters(&self, tokens: &[Token], span: Span) -> Option<(Span, Span)> {
        #[cfg(test)]
        PARENTHESIS_LOOKUPS.set(PARENTHESIS_LOOKUPS.get() + 1);
        let close = tokens_in_span(tokens, span)
            .last()
            .filter(|token| token.kind() == Kind::RParen)?
            .span();
        let open = *self.open_by_close.get(&close.start)?;
        Some((open, close))
    }
}

struct ListTokens {
    first: Span,
    last: Span,
    trailing_comma: Option<Span>,
}

fn list_tokens(tokens: &[Token], open: Span, close: Span) -> Option<ListTokens> {
    let tokens = tokens_in_span(tokens, Span::new(open.end, close.start));
    let first = tokens.first()?.span();
    let trailing_comma = tokens
        .last()
        .filter(|token| token.kind() == Kind::Comma)
        .map(Token::span);
    let last = if trailing_comma.is_some() {
        tokens.get(tokens.len().checked_sub(2)?)?.span()
    } else {
        tokens.last()?.span()
    };
    Some(ListTokens {
        first,
        last,
        trailing_comma,
    })
}

fn exact_delimiters(
    tokens: &[Token],
    span: Span,
    open_kind: Kind,
    close_kind: Kind,
) -> Option<(Span, Span)> {
    let tokens = tokens_in_span(tokens, span);
    let open = tokens.first().filter(|token| token.kind() == open_kind)?;
    let close = tokens.last().filter(|token| token.kind() == close_kind)?;
    Some((open.span(), close.span()))
}

fn surrounding_delimiters(
    tokens: &[Token],
    span: Span,
    open_kind: Kind,
    close_kind: Kind,
    first_item: Span,
    last_item: Span,
) -> Option<(Span, Span)> {
    let tokens = tokens_in_span(tokens, span);
    let open = tokens
        .iter()
        .rev()
        .find(|token| token.kind() == open_kind && token.end() <= first_item.start)?;
    let close = tokens
        .iter()
        .find(|token| token.kind() == close_kind && token.start() >= last_item.end)?;
    Some((open.span(), close.span()))
}

fn tuple_element_is_rest(element: &TSTupleElement<'_>) -> bool {
    match element {
        TSTupleElement::TSRestType(_) => true,
        TSTupleElement::TSNamedTupleMember(member) => tuple_element_is_rest(&member.element_type),
        _ => false,
    }
}

#[derive(Clone, Copy, Debug)]
struct StatementShape {
    span: Span,
    target: StatementTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StatementTarget {
    Other,
    ControlFlow {
        spacing: StatementSpacingMode,
    },
    Import {
        multiline: bool,
        spacing: StatementSpacingMode,
    },
    MultilineCall {
        multiline: bool,
        spacing: StatementSpacingMode,
    },
    Return {
        spacing: StatementSpacingMode,
    },
    TypeAlias {
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
    interface_layout_threshold: Option<u32>,
    object_property_spacing: bool,
    control_flow_spacing: StatementSpacingMode,
    import_spacing: StatementSpacingMode,
    multiline_call_spacing: StatementSpacingMode,
    return_spacing: StatementSpacingMode,
    type_alias_spacing: StatementSpacingMode,
    variable_spacing: StatementSpacingMode,
    statement_semicolons: SemicolonMode,
    trailing_commas: TrailingCommaMode,
    single_arrow_comma: SingleArrowCommaRule,
}

#[derive(Clone, Copy, Debug)]
enum ParentItem {
    Statement {
        list_index: usize,
        item_index: usize,
    },
    ObjectProperty {
        object_index: usize,
        item_index: usize,
    },
    SwitchCaseLabel {
        switch_index: usize,
        list_index: Option<usize>,
    },
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum ExpandedLayout {
    List(usize),
    Switch(usize),
    Interface(usize),
    Object(usize),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum LayoutExpansionCause {
    DirectStatementSpacing,
    Cascading,
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
    unfolded_items: HashSet<usize>,
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

#[derive(Debug)]
struct InterfaceBodyLayout {
    open: Span,
    close: Span,
    members: Vec<Span>,
    layout_parent: Option<ExpandedLayout>,
    parent_item: Option<ParentItem>,
}

#[derive(Debug)]
struct ObjectItemLayout {
    span: Span,
    boundary_end: u32,
    multiline: bool,
}

#[derive(Debug)]
struct ObjectLayout {
    open: Span,
    close: Span,
    items: Vec<ObjectItemLayout>,
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
    let initial_import_layout = FinalImportLayout::default();
    let mut import_rewrites = format_import_edits(
        source,
        program,
        tokens,
        line_width,
        newline,
        rules,
        &initial_import_layout,
    )?;
    let mut layout_edits = collect_layout_edits(
        source,
        program,
        tokens,
        newline,
        rules,
        &import_rewrites.formatted_imports,
    )?;
    let final_import_layout = FinalImportLayout {
        line_break_after: if rules.statement_semicolons == SemicolonMode::AsNeeded {
            layout_edits
                .iter()
                .filter(|edit| contains_line_break(&edit.replacement))
                .map(|edit| edit.start)
                .collect()
        } else {
            HashSet::new()
        },
        base_indents: layout_import_base_indents(&layout_edits, &import_rewrites),
    };
    let import_indent_changed = final_import_layout
        .base_indents
        .iter()
        .any(|(start, indent)| line_indent_at(source, *start) != *indent);
    let import_semicolon_changed = import_rewrites
        .deferred_semicolon_ends
        .iter()
        .any(|offset| {
            contains_deferred_import_boundary(&final_import_layout.line_break_after, *offset)
        });
    // A layout-added line break can change an import's final indentation or make its semicolon
    // removable. Recompute both import and layout edits once with that final boundary shape.
    if import_indent_changed || import_semicolon_changed {
        import_rewrites = format_import_edits(
            source,
            program,
            tokens,
            line_width,
            newline,
            rules,
            &final_import_layout,
        )?;
        layout_edits = collect_layout_edits(
            source,
            program,
            tokens,
            newline,
            rules,
            &import_rewrites.formatted_imports,
        )?;
    }

    let mut edits = import_rewrites.edits;
    edits.extend(layout_edits);
    append_never_comma_edits(source, program, tokens, rules, &mut edits);
    edits.sort_by_key(|edit| (edit.start, edit.end));
    Ok(edits)
}

#[derive(Default)]
struct ImportRewrites {
    edits: Vec<Edit>,
    formatted_imports: HashMap<u32, bool>,
    deferred_semicolon_ends: HashSet<u32>,
}

#[derive(Default)]
struct FinalImportLayout {
    line_break_after: HashSet<u32>,
    base_indents: HashMap<u32, String>,
}

fn layout_import_base_indents(
    layout_edits: &[Edit],
    import_rewrites: &ImportRewrites,
) -> HashMap<u32, String> {
    layout_edits
        .iter()
        .filter_map(|edit| {
            if !import_rewrites.formatted_imports.contains_key(&edit.end) {
                return None;
            }
            let (_, indent) = edit.replacement.rsplit_once('\n')?;
            indent
                .chars()
                .all(|character| matches!(character, ' ' | '\t'))
                .then(|| (edit.end, indent.to_owned()))
        })
        .collect()
}

fn format_import_edits(
    source: &str,
    program: &Program<'_>,
    tokens: &[Token],
    line_width: u32,
    newline: &str,
    rules: RewriteRules,
    final_layout: &FinalImportLayout,
) -> Result<ImportRewrites, FormatError> {
    let mut rewrites = ImportRewrites::default();
    if !rules.import_layout {
        return Ok(rewrites);
    }

    for statement in &program.body {
        let Statement::ImportDeclaration(declaration) = statement else {
            continue;
        };
        let span = statement_syntax_span(source, tokens, statement.span());
        let declaration_tokens = tokens_in_span(tokens, span);
        let declaration_comments = comments_in_span(&program.comments, span);
        let trailing_semicolon = declaration_tokens
            .last()
            .filter(|token| token.kind() == Kind::Semicolon);
        let original_semicolon = trailing_semicolon.is_some();
        let final_semicolon = match rules.statement_semicolons {
            SemicolonMode::Off => original_semicolon,
            SemicolonMode::Always => true,
            SemicolonMode::AsNeeded => trailing_semicolon.is_some_and(|token| {
                !can_remove_trailing_semicolon(source, tokens, token.span())
                    && !final_layout.line_break_after.contains(&token.end())
            }),
        };
        let semicolon_shape = ImportSemicolonShape {
            original: original_semicolon,
            formatted: final_semicolon,
        };
        if rules.statement_semicolons == SemicolonMode::AsNeeded
            && original_semicolon
            && final_semicolon
            && let Some(semicolon) = trailing_semicolon
        {
            rewrites.deferred_semicolon_ends.insert(semicolon.end());
        }
        let base_indent = final_layout
            .base_indents
            .get(&span.start)
            .cloned()
            .unwrap_or_else(|| line_indent_at(source, span.start));
        let formatted = format_import(
            declaration,
            span,
            source,
            &base_indent,
            declaration_tokens,
            declaration_comments,
            line_width,
            newline,
            rules.trailing_commas,
            semicolon_shape,
        )?;
        let original = source_slice(source, span)?;
        if formatted.text != original {
            rewrites.edits.push(Edit {
                start: span.start,
                end: span.end,
                replacement: formatted.text,
            });
        }
        rewrites
            .formatted_imports
            .insert(span.start, formatted.multiline);
    }

    Ok(rewrites)
}

fn collect_layout_edits(
    source: &str,
    program: &Program<'_>,
    tokens: &[Token],
    newline: &str,
    rules: RewriteRules,
    formatted_imports: &HashMap<u32, bool>,
) -> Result<Vec<Edit>, FormatError> {
    if rules.import_spacing == StatementSpacingMode::Off
        && rules.control_flow_spacing == StatementSpacingMode::Off
        && rules.multiline_call_spacing == StatementSpacingMode::Off
        && rules.return_spacing == StatementSpacingMode::Off
        && rules.type_alias_spacing == StatementSpacingMode::Off
        && rules.variable_spacing == StatementSpacingMode::Off
        && rules.interface_layout_threshold.is_none()
        && !rules.object_property_spacing
    {
        return Ok(Vec::new());
    }

    let line_breaks = (rules.object_property_spacing
        || rules.multiline_call_spacing != StatementSpacingMode::Off)
        .then(|| LineBreakIndex::new(source));
    let mut collector = StatementCollector {
        source,
        tokens,
        formatted_imports,
        line_breaks: line_breaks.as_ref(),
        interface_layout_threshold: rules.interface_layout_threshold,
        object_property_spacing: rules.object_property_spacing,
        control_flow_spacing: rules.control_flow_spacing,
        import_spacing: rules.import_spacing,
        multiline_call_spacing: rules.multiline_call_spacing,
        return_spacing: rules.return_spacing,
        type_alias_spacing: rules.type_alias_spacing,
        variable_spacing: rules.variable_spacing,
        ambient_depth: 0,
        current_list: None,
        current_layout: None,
        current_item: None,
        current_switch: None,
        lists: Vec::new(),
        switches: Vec::new(),
        interfaces: Vec::new(),
        objects: Vec::new(),
    };
    collector.visit_program(program);
    mark_expanded_layouts(
        &mut collector.lists,
        &mut collector.switches,
        &collector.interfaces,
        &mut collector.objects,
    );
    let mut edits = Vec::new();
    append_layout_edits(
        source,
        &program.comments,
        newline,
        &collector.lists,
        &collector.switches,
        &collector.interfaces,
        &collector.objects,
        line_breaks.as_ref(),
        &mut edits,
    )?;
    Ok(edits)
}

fn append_never_comma_edits(
    source: &str,
    program: &Program<'_>,
    tokens: &[Token],
    rules: RewriteRules,
    edits: &mut Vec<Edit>,
) {
    if rules.trailing_commas != TrailingCommaMode::Never {
        return;
    }
    edits.extend(trailing_comma_edits(
        source,
        program,
        tokens,
        TrailingCommaMode::Never,
        rules.single_arrow_comma,
        rules.import_layout,
    ));
}

struct StatementCollector<'s> {
    source: &'s str,
    tokens: &'s [Token],
    formatted_imports: &'s HashMap<u32, bool>,
    line_breaks: Option<&'s LineBreakIndex>,
    interface_layout_threshold: Option<u32>,
    object_property_spacing: bool,
    control_flow_spacing: StatementSpacingMode,
    import_spacing: StatementSpacingMode,
    multiline_call_spacing: StatementSpacingMode,
    return_spacing: StatementSpacingMode,
    type_alias_spacing: StatementSpacingMode,
    variable_spacing: StatementSpacingMode,
    ambient_depth: usize,
    current_list: Option<usize>,
    current_layout: Option<ExpandedLayout>,
    current_item: Option<ParentItem>,
    current_switch: Option<usize>,
    lists: Vec<StatementList>,
    switches: Vec<SwitchLayout>,
    interfaces: Vec<InterfaceBodyLayout>,
    objects: Vec<ObjectLayout>,
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
                span: statement_syntax_span(self.source, self.tokens, directive.span()),
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
            unfolded_items: HashSet::new(),
        });
        Some(list_index)
    }

    fn statement_shape(&self, statement: &Statement<'_>) -> StatementShape {
        let is_type_alias = self.type_alias_spacing != StatementSpacingMode::Off
            && matches!(
                statement,
                Statement::TSTypeAliasDeclaration(declaration) if !declaration.declare
            );
        let span = if is_type_alias {
            statement.span()
        } else {
            statement_syntax_span(self.source, self.tokens, statement.span())
        };
        let target = if self.import_spacing != StatementSpacingMode::Off
            && matches!(statement, Statement::ImportDeclaration(_))
        {
            StatementTarget::Import {
                multiline: self
                    .formatted_imports
                    .get(&span.start)
                    .copied()
                    .unwrap_or_else(|| import_is_multiline(self.source, span)),
                spacing: self.import_spacing,
            }
        } else if self.control_flow_spacing != StatementSpacingMode::Off
            && is_control_flow_statement(statement)
        {
            StatementTarget::ControlFlow {
                spacing: self.control_flow_spacing,
            }
        } else if self.multiline_call_spacing != StatementSpacingMode::Off
            && let Some(call_span) = direct_call_expression_span(statement)
        {
            StatementTarget::MultilineCall {
                multiline: self
                    .line_breaks
                    .is_some_and(|line_breaks| line_breaks.contains(call_span)),
                spacing: self.multiline_call_spacing,
            }
        } else if self.return_spacing != StatementSpacingMode::Off
            && matches!(statement, Statement::ReturnStatement(_))
        {
            StatementTarget::Return {
                spacing: self.return_spacing,
            }
        } else if is_type_alias {
            StatementTarget::TypeAlias {
                multiline: type_alias_is_multiline(self.source, span),
                spacing: self.type_alias_spacing,
            }
        } else if self.variable_spacing != StatementSpacingMode::Off
            && self.ambient_depth == 0
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
                multiline: variable_is_multiline(self.source, span),
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
        let statement_span = statement_syntax_span(self.source, self.tokens, statement.span());
        let items = &self.lists[list_index].items;
        let item_index = items
            .binary_search_by_key(&statement_span.start, |item| item.span.start)
            .ok()?;
        let item_span = items[item_index].span;
        if item_span.start != statement_span.start || item_span.end != statement_span.end {
            return None;
        }
        Some(ParentItem::Statement {
            list_index,
            item_index,
        })
    }
}

fn direct_call_expression_span(statement: &Statement<'_>) -> Option<Span> {
    let Statement::ExpressionStatement(statement) = statement else {
        return None;
    };
    let mut expression = &statement.expression;
    loop {
        expression = expression.get_inner_expression();
        match expression {
            Expression::AwaitExpression(await_expression) => {
                expression = &await_expression.argument;
            }
            Expression::CallExpression(call_expression) => return Some(call_expression.span),
            Expression::ChainExpression(chain_expression) => match &chain_expression.expression {
                ChainElement::CallExpression(call_expression) => return Some(call_expression.span),
                ChainElement::TSNonNullExpression(non_null_expression) => {
                    expression = &non_null_expression.expression;
                }
                _ => return None,
            },
            _ => return None,
        }
    }
}

fn is_control_flow_statement(statement: &Statement<'_>) -> bool {
    matches!(
        statement,
        Statement::DoWhileStatement(_)
            | Statement::ForInStatement(_)
            | Statement::ForOfStatement(_)
            | Statement::ForStatement(_)
            | Statement::IfStatement(_)
            | Statement::SwitchStatement(_)
            | Statement::TryStatement(_)
            | Statement::WhileStatement(_)
    )
}

fn statement_syntax_span(source: &str, tokens: &[Token], span: Span) -> Span {
    let tokens = tokens_in_span(tokens, span);
    let [.., previous, semicolon] = tokens else {
        return span;
    };
    if semicolon.kind() == Kind::Semicolon
        && source
            .get(previous.end() as usize..semicolon.start() as usize)
            .is_some_and(contains_line_break)
    {
        Span::new(span.start, previous.end())
    } else {
        span
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

    fn visit_object_expression(&mut self, expression: &ObjectExpression<'a>) {
        if !self.object_property_spacing || expression.properties.is_empty() {
            walk_object_expression(self, expression);
            return;
        }
        let Some((open, close)) = brace_tokens(self.tokens, expression.span) else {
            walk_object_expression(self, expression);
            return;
        };
        let original_multiline = self
            .line_breaks
            .is_some_and(|line_breaks| line_breaks.contains(expression.span));
        let object_index = self.objects.len();
        let mut items = expression
            .properties
            .iter()
            .map(|property| {
                let span = property.span();
                ObjectItemLayout {
                    span,
                    boundary_end: span.end,
                    multiline: self
                        .line_breaks
                        .is_some_and(|line_breaks| line_breaks.contains(span)),
                }
            })
            .collect::<Vec<_>>();
        for item_index in 0..items.len() {
            let boundary_limit = items
                .get(item_index + 1)
                .map_or(close.start, |next| next.span.start);
            items[item_index].boundary_end = tokens_in_span(
                self.tokens,
                Span::new(items[item_index].span.end, boundary_limit),
            )
            .iter()
            .find(|token| token.kind() == Kind::Comma)
            .map_or(items[item_index].span.end, Token::end);
        }
        self.objects.push(ObjectLayout {
            open,
            close,
            items,
            layout_parent: self.current_layout,
            parent_item: self.current_item,
            original_multiline,
            expanded: expression.properties.len() >= 2 && !original_multiline,
        });

        let previous_layout = self
            .current_layout
            .replace(ExpandedLayout::Object(object_index));
        for (item_index, property) in expression.properties.iter().enumerate() {
            let previous_item = self.current_item.replace(ParentItem::ObjectProperty {
                object_index,
                item_index,
            });
            self.visit_object_property_kind(property);
            self.current_item = previous_item;
        }
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
        let case_list = if !case.consequent.is_empty()
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

        let previous_item = self.current_item;
        if let Some(test) = &case.test {
            self.current_item =
                self.current_switch
                    .map(|switch_index| ParentItem::SwitchCaseLabel {
                        switch_index,
                        list_index: case_list,
                    });
            self.visit_expression(test);
            self.current_item = previous_item;
        }

        self.current_list = case_list;
        self.current_layout = self.current_list.map(ExpandedLayout::List);
        self.visit_statements(&case.consequent);
        self.current_list = previous_list;
        self.current_layout = previous_layout;
        self.current_item = previous_item;
    }

    fn visit_ts_interface_body(&mut self, body: &TSInterfaceBody<'a>) {
        if let Some(threshold) = self.interface_layout_threshold
            && let Ok(threshold) = usize::try_from(threshold)
            && body.body.len() > threshold
            && let Some((open, close)) = brace_tokens(self.tokens, body.span)
        {
            self.interfaces.push(InterfaceBodyLayout {
                open,
                close,
                members: body.body.iter().map(GetSpan::span).collect(),
                layout_parent: self.current_layout,
                parent_item: self.current_item,
            });
        }
        walk_ts_interface_body(self, body);
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

#[allow(
    clippy::too_many_lines,
    reason = "the single cascade queue handles every layout kind without repeated ancestor passes"
)]
fn mark_expanded_layouts(
    lists: &mut [StatementList],
    switches: &mut [SwitchLayout],
    interfaces: &[InterfaceBodyLayout],
    objects: &mut [ObjectLayout],
) {
    let mut pending = VecDeque::new();
    let mut queued = HashSet::new();
    for (list_index, list) in lists.iter_mut().enumerate() {
        let cause = initial_list_expansion_cause(list);
        list.expanded = cause.is_some();
        if let Some(cause) = cause {
            enqueue_layout_expansion(
                &mut pending,
                &mut queued,
                ExpandedLayout::List(list_index),
                cause,
            );
        }
    }
    for interface_index in 0..interfaces.len() {
        enqueue_layout_expansion(
            &mut pending,
            &mut queued,
            ExpandedLayout::Interface(interface_index),
            LayoutExpansionCause::Cascading,
        );
    }
    for (object_index, object) in objects.iter().enumerate() {
        if object.expanded {
            enqueue_layout_expansion(
                &mut pending,
                &mut queued,
                ExpandedLayout::Object(object_index),
                LayoutExpansionCause::Cascading,
            );
        }
    }

    while let Some((layout, cause)) = pending.pop_front() {
        let (parent, parent_item) = match layout {
            ExpandedLayout::List(list_index) => (
                lists[list_index].layout_parent,
                lists[list_index].parent_item,
            ),
            ExpandedLayout::Switch(switch_index) => (
                switches[switch_index].layout_parent,
                switches[switch_index].parent_item,
            ),
            ExpandedLayout::Interface(interface_index) => (
                interfaces[interface_index].layout_parent,
                interfaces[interface_index].parent_item,
            ),
            ExpandedLayout::Object(object_index) => (
                objects[object_index].layout_parent,
                objects[object_index].parent_item,
            ),
        };
        if let Some(parent_item) = parent_item {
            mark_parent_item_multiline(lists, objects, parent_item);
            if let ParentItem::SwitchCaseLabel {
                list_index: Some(list_index),
                ..
            } = parent_item
                && !lists[list_index].original_multiline
            {
                lists[list_index].expanded = true;
                enqueue_layout_expansion(
                    &mut pending,
                    &mut queued,
                    ExpandedLayout::List(list_index),
                    LayoutExpansionCause::Cascading,
                );
            }
        }
        match parent {
            Some(ExpandedLayout::List(list_index)) => {
                let parent_list = &mut lists[list_index];
                if cause == LayoutExpansionCause::DirectStatementSpacing
                    && is_lone_direct_statement_spacing_list(parent_list)
                {
                    enqueue_layout_expansion(
                        &mut pending,
                        &mut queued,
                        ExpandedLayout::List(list_index),
                        LayoutExpansionCause::Cascading,
                    );
                    continue;
                }
                let participates = if parent_list.original_multiline {
                    matches!(
                        parent_item,
                        Some(ParentItem::Statement {
                            list_index: parent_list_index,
                            item_index,
                        }) if parent_list_index == list_index
                            && parent_list.unfolded_items.insert(item_index)
                    )
                } else {
                    parent_list.expanded = true;
                    true
                };
                if participates {
                    enqueue_layout_expansion(
                        &mut pending,
                        &mut queued,
                        ExpandedLayout::List(list_index),
                        cause,
                    );
                }
            }
            Some(ExpandedLayout::Switch(switch_index)) => {
                let parent_switch = &mut switches[switch_index];
                if !parent_switch.original_multiline {
                    parent_switch.expanded = true;
                    enqueue_layout_expansion(
                        &mut pending,
                        &mut queued,
                        ExpandedLayout::Switch(switch_index),
                        cause,
                    );
                }
            }
            Some(ExpandedLayout::Object(object_index)) => {
                enqueue_layout_expansion(
                    &mut pending,
                    &mut queued,
                    ExpandedLayout::Object(object_index),
                    LayoutExpansionCause::Cascading,
                );
            }
            Some(ExpandedLayout::Interface(_)) | None => {}
        }
    }
}

fn mark_parent_item_multiline(
    lists: &mut [StatementList],
    objects: &mut [ObjectLayout],
    parent: ParentItem,
) {
    match parent {
        ParentItem::Statement {
            list_index,
            item_index,
        } => match &mut lists[list_index].items[item_index].target {
            StatementTarget::Import { multiline, .. }
            | StatementTarget::MultilineCall { multiline, .. }
            | StatementTarget::TypeAlias { multiline, .. }
            | StatementTarget::Variable { multiline, .. } => *multiline = true,
            StatementTarget::ControlFlow { .. }
            | StatementTarget::Other
            | StatementTarget::Return { .. } => {}
        },
        ParentItem::ObjectProperty {
            object_index,
            item_index,
        } => objects[object_index].items[item_index].multiline = true,
        ParentItem::SwitchCaseLabel { .. } => {}
    }
}

fn initial_list_expansion_cause(list: &StatementList) -> Option<LayoutExpansionCause> {
    if list.original_multiline || list.items.len() < 2 {
        return None;
    }

    let mut contains_direct_statement_spacing = false;
    for item in &list.items {
        match item.target {
            StatementTarget::ControlFlow { spacing } | StatementTarget::Return { spacing }
                if spacing != StatementSpacingMode::Off =>
            {
                contains_direct_statement_spacing = true;
            }
            StatementTarget::TypeAlias { spacing, .. }
            | StatementTarget::Variable { spacing, .. }
                if spacing != StatementSpacingMode::Off =>
            {
                return Some(LayoutExpansionCause::Cascading);
            }
            StatementTarget::ControlFlow { .. }
            | StatementTarget::Other
            | StatementTarget::Import { .. }
            | StatementTarget::MultilineCall { .. }
            | StatementTarget::Return { .. }
            | StatementTarget::TypeAlias { .. }
            | StatementTarget::Variable { .. } => {}
        }
    }
    contains_direct_statement_spacing.then_some(LayoutExpansionCause::DirectStatementSpacing)
}

fn is_lone_direct_statement_spacing_list(list: &StatementList) -> bool {
    list.items.len() == 1
        && matches!(
            list.items[0].target,
            StatementTarget::ControlFlow { .. } | StatementTarget::Return { .. }
        )
}

fn enqueue_layout_expansion(
    pending: &mut VecDeque<(ExpandedLayout, LayoutExpansionCause)>,
    queued: &mut HashSet<(ExpandedLayout, LayoutExpansionCause)>,
    layout: ExpandedLayout,
    cause: LayoutExpansionCause,
) {
    if queued.insert((layout, cause)) {
        pending.push_back((layout, cause));
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "layout edit orchestration shares all collected container kinds"
)]
fn append_layout_edits(
    source: &str,
    comments: &[Comment],
    newline: &str,
    lists: &[StatementList],
    switches: &[SwitchLayout],
    interfaces: &[InterfaceBodyLayout],
    objects: &[ObjectLayout],
    object_line_breaks: Option<&LineBreakIndex>,
    edits: &mut Vec<Edit>,
) -> Result<(), FormatError> {
    let mut indents = LayoutIndents::new(lists.len(), switches.len(), objects.len());
    let directive_target_lines = typescript_directive_target_lines(source, comments)?;
    append_list_layout_edits(
        source,
        comments,
        newline,
        &directive_target_lines,
        lists,
        switches,
        objects,
        &mut indents,
        edits,
    )?;
    append_switch_layout_edits(
        source,
        comments,
        newline,
        lists,
        switches,
        objects,
        &mut indents,
        edits,
    )?;
    append_interface_layout_edits(
        source,
        comments,
        newline,
        &directive_target_lines,
        lists,
        switches,
        objects,
        interfaces,
        &mut indents,
        edits,
    )?;
    append_object_layout_edits(
        source,
        comments,
        newline,
        lists,
        switches,
        objects,
        object_line_breaks,
        &mut indents,
        edits,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "statement boundaries share the collected layout and directive context"
)]
fn append_list_layout_edits(
    source: &str,
    comments: &[Comment],
    newline: &str,
    directive_target_lines: &HashSet<u32>,
    lists: &[StatementList],
    switches: &[SwitchLayout],
    objects: &[ObjectLayout],
    indents: &mut LayoutIndents,
    edits: &mut Vec<Edit>,
) -> Result<(), FormatError> {
    for (list_index, list) in lists.iter().enumerate() {
        let expanded_item_indent = list
            .expanded
            .then(|| indents.item_indent(source, lists, switches, objects, list_index));

        let opening_boundary = list_opening_boundary(list);
        let unfold_first = !list.expanded
            && list.unfolded_items.contains(&0)
            && opening_boundary.is_some_and(|span| boundary_is_inline(source, span));
        if (list.expanded || unfold_first)
            && let Some(span) = opening_boundary
        {
            let item_indent = expanded_item_indent.clone().unwrap_or_else(|| {
                indents.existing_item_indent(source, lists, switches, objects, list_index, 0)
            });
            append_boundary_edit(source, comments, span, newline, &item_indent, edits)?;
        }

        let mut fallback_indent = String::new();
        for (pair_index, pair) in list.items.windows(2).enumerate() {
            let [previous, next] = pair else {
                unreachable!("windows(2) always contains two statements")
            };
            let span = Span::new(previous.span.end, next.span.start);
            let unfold_boundary = !list.expanded
                && (list.unfolded_items.contains(&pair_index)
                    || list.unfolded_items.contains(&(pair_index + 1)))
                && boundary_is_inline(source, span);
            let Some(blank_line) = boundary_blank_line(
                previous.target,
                next.target,
                list.expanded || unfold_boundary,
            ) else {
                continue;
            };
            if spans_share_directive_target_line(
                source,
                previous.span,
                next.span,
                directive_target_lines,
            ) {
                continue;
            }
            let separator = if blank_line {
                newline.repeat(2)
            } else {
                newline.to_owned()
            };
            let indent = if list.expanded {
                expanded_item_indent.clone().unwrap()
            } else if unfold_boundary {
                indents.existing_item_indent(
                    source,
                    lists,
                    switches,
                    objects,
                    list_index,
                    pair_index + 1,
                )
            } else {
                let previous_indent = line_indent_at(source, previous.span.start);
                if !previous_indent.is_empty() {
                    fallback_indent.clone_from(&previous_indent);
                } else if fallback_indent.is_empty() {
                    fallback_indent = indents.existing_item_indent(
                        source, lists, switches, objects, list_index, pair_index,
                    );
                }
                existing_boundary_indent(
                    source,
                    comments,
                    span,
                    &previous_indent,
                    next.span.start,
                    &fallback_indent,
                )
            };
            append_boundary_edit(source, comments, span, &separator, &indent, edits)?;
        }

        let last_index = list.items.len() - 1;
        let closing_boundary = list_closing_boundary(list);
        let unfold_last = !list.expanded
            && list.unfolded_items.contains(&last_index)
            && closing_boundary.is_some_and(|span| boundary_is_inline(source, span));
        if (list.expanded || unfold_last)
            && let Some(span) = closing_boundary
        {
            let base_indent =
                indents.list_base_indent(source, lists, switches, objects, list_index);
            append_boundary_edit(source, comments, span, newline, &base_indent, edits)?;
        }
    }

    Ok(())
}

fn list_opening_boundary(list: &StatementList) -> Option<Span> {
    let start = match list.container {
        ListContainer::Program { .. } => return None,
        ListContainer::Braced { open, .. } => open.end,
        ListContainer::SwitchCase { colon, .. } => colon.end,
    };
    Some(Span::new(start, list.items[0].span.start))
}

fn list_closing_boundary(list: &StatementList) -> Option<Span> {
    let ListContainer::Braced { close, .. } = list.container else {
        return None;
    };
    Some(Span::new(list.items.last()?.span.end, close.start))
}

#[allow(
    clippy::too_many_arguments,
    reason = "switch indentation can depend on every collected parent container kind"
)]
fn append_switch_layout_edits(
    source: &str,
    comments: &[Comment],
    newline: &str,
    lists: &[StatementList],
    switches: &[SwitchLayout],
    objects: &[ObjectLayout],
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
        let switch_indent = indents.switch_indent(source, lists, switches, objects, switch_index);
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

#[allow(
    clippy::too_many_arguments,
    reason = "interface boundaries share the collected layout and source context"
)]
fn append_interface_layout_edits(
    source: &str,
    comments: &[Comment],
    newline: &str,
    directive_target_lines: &HashSet<u32>,
    lists: &[StatementList],
    switches: &[SwitchLayout],
    objects: &[ObjectLayout],
    interfaces: &[InterfaceBodyLayout],
    indents: &mut LayoutIndents,
    edits: &mut Vec<Edit>,
) -> Result<(), FormatError> {
    for interface in interfaces {
        let Some(first) = interface.members.first() else {
            continue;
        };
        let base_indent =
            indents.interface_base_indent(source, lists, switches, objects, interface);
        let member_indent = format!("{base_indent}  ");
        append_boundary_edit(
            source,
            comments,
            Span::new(interface.open.end, first.start),
            newline,
            &member_indent,
            edits,
        )?;
        for pair in interface.members.windows(2) {
            if spans_share_directive_target_line(source, pair[0], pair[1], directive_target_lines) {
                continue;
            }
            append_boundary_edit(
                source,
                comments,
                Span::new(pair[0].end, pair[1].start),
                newline,
                &member_indent,
                edits,
            )?;
        }
        append_boundary_edit(
            source,
            comments,
            Span::new(interface.members.last().unwrap().end, interface.close.start),
            newline,
            &base_indent,
            edits,
        )?;
    }

    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "object boundaries share the collected layout and source context"
)]
fn append_object_layout_edits(
    source: &str,
    comments: &[Comment],
    newline: &str,
    lists: &[StatementList],
    switches: &[SwitchLayout],
    objects: &[ObjectLayout],
    line_breaks: Option<&LineBreakIndex>,
    indents: &mut LayoutIndents,
    edits: &mut Vec<Edit>,
) -> Result<(), FormatError> {
    if objects.is_empty() {
        return Ok(());
    }
    let line_breaks = line_breaks
        .ok_or_else(|| FormatError::internal("object layout line index was not available"))?;
    for (object_index, object) in objects.iter().enumerate() {
        let [first, .., last] = object.items.as_slice() else {
            continue;
        };
        let base_indent =
            indents.object_base_indent(source, lists, switches, objects, object_index);
        let canonical_item_indent = format!("{base_indent}  ");
        let opening = Span::new(object.open.end, first.span.start);
        let opening_indent = if object.expanded {
            canonical_item_indent.clone()
        } else {
            existing_boundary_indent(
                source,
                comments,
                opening,
                "",
                first.span.start,
                &canonical_item_indent,
            )
        };
        append_object_boundary_edit(
            source,
            comments,
            opening,
            newline,
            &opening_indent,
            None,
            Some(first.span.start),
            line_breaks,
            edits,
        )?;

        let mut fallback_indent = opening_indent;
        for pair in object.items.windows(2) {
            let [previous, next] = pair else {
                unreachable!("windows(2) always contains two object items")
            };
            let span = Span::new(previous.boundary_end, next.span.start);
            let separator = if previous.multiline || next.multiline {
                newline.repeat(2)
            } else {
                newline.to_owned()
            };
            let next_indent = if object.expanded {
                canonical_item_indent.clone()
            } else {
                existing_boundary_indent(
                    source,
                    comments,
                    span,
                    &line_indent_at(source, previous.span.start),
                    next.span.start,
                    &fallback_indent,
                )
            };
            if !next_indent.is_empty() {
                fallback_indent.clone_from(&next_indent);
            }
            append_object_boundary_edit(
                source,
                comments,
                span,
                &separator,
                &next_indent,
                Some(previous.span.end),
                Some(next.span.start),
                line_breaks,
                edits,
            )?;
        }

        let closing = Span::new(last.boundary_end, object.close.start);
        append_object_boundary_edit(
            source,
            comments,
            closing,
            newline,
            &base_indent,
            Some(last.span.end),
            None,
            line_breaks,
            edits,
        )?;
    }
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "object comment attachment needs both neighboring syntax anchors"
)]
fn append_object_boundary_edit(
    source: &str,
    comments: &[Comment],
    span: Span,
    separator: &str,
    indent: &str,
    previous_end: Option<u32>,
    next_start: Option<u32>,
    line_breaks: &LineBreakIndex,
    edits: &mut Vec<Edit>,
) -> Result<(), FormatError> {
    let mut boundary_comments = comments_in_span(comments, span).to_vec();
    if !boundary_comments.is_empty() {
        let previous_line = previous_end.map(|offset| line_breaks.line_start(offset));
        for comment in &mut boundary_comments {
            let comment_line = line_breaks.line_start(comment.span.start);
            if previous_line.is_some_and(|previous_line| comment_line == previous_line) {
                comment.position = CommentPosition::Trailing;
            } else if next_start.is_some() {
                comment.position = CommentPosition::Leading;
            }
        }
    }
    let original = source_slice(source, span)?;
    let formatted = format_boundary_separator(source, span, &boundary_comments, separator, indent)?;
    if original != formatted {
        edits.push(Edit {
            start: span.start,
            end: span.end,
            replacement: formatted,
        });
    }
    Ok(())
}

fn typescript_directive_target_lines(
    source: &str,
    comments: &[Comment],
) -> Result<HashSet<u32>, FormatError> {
    let mut target_lines = HashSet::new();
    for comment in comments {
        if comment.kind != CommentKind::Line
            || !is_line_scoped_typescript_directive(source_slice(source, comment.span)?)
        {
            continue;
        }
        if let Some(line_start) = next_line_start(source, comment.span.end) {
            target_lines.insert(line_start);
        }
    }
    Ok(target_lines)
}

fn is_line_scoped_typescript_directive(comment: &str) -> bool {
    let Some(comment) = comment.strip_prefix("//") else {
        return false;
    };
    let comment = comment.trim_start_matches(['/', ' ', '\t']);
    ["@ts-ignore", "@ts-expect-error"]
        .into_iter()
        .any(|directive| {
            comment.strip_prefix(directive).is_some_and(|suffix| {
                suffix
                    .chars()
                    .next()
                    .is_none_or(|character| character.is_whitespace() || character == ':')
            })
        })
}

fn next_line_start(source: &str, offset: u32) -> Option<u32> {
    let offset = usize::try_from(offset).ok()?;
    let relative_newline = source.get(offset..)?.find('\n')?;
    u32::try_from(offset.checked_add(relative_newline)?.checked_add(1)?).ok()
}

fn line_start(source: &str, offset: u32) -> Option<u32> {
    #[cfg(test)]
    RAW_LINE_START_SCANS.set(RAW_LINE_START_SCANS.get() + 1);
    let offset = usize::try_from(offset).ok()?;
    let start = source
        .get(..offset)?
        .rfind('\n')
        .map_or(0, |newline| newline + 1);
    u32::try_from(start).ok()
}

fn spans_share_directive_target_line(
    source: &str,
    previous: Span,
    next: Span,
    directive_target_lines: &HashSet<u32>,
) -> bool {
    if directive_target_lines.is_empty() {
        return false;
    }
    let Some(previous_line) = line_start(source, previous.start) else {
        return false;
    };
    line_start(source, next.start).is_some_and(|next_line| {
        next_line == previous_line && directive_target_lines.contains(&next_line)
    })
}

fn boundary_is_inline(source: &str, span: Span) -> bool {
    source_slice(source, span).is_ok_and(|boundary| !contains_line_break(boundary))
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
        StatementTarget::Other
        | StatementTarget::MultilineCall {
            multiline: false, ..
        } => return None,
        StatementTarget::ControlFlow { spacing }
        | StatementTarget::Import { spacing, .. }
        | StatementTarget::MultilineCall { spacing, .. }
        | StatementTarget::Return { spacing }
        | StatementTarget::TypeAlias { spacing, .. }
        | StatementTarget::Variable { spacing, .. } => spacing,
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
            StatementTarget::TypeAlias {
                multiline: false,
                ..
            },
            StatementTarget::TypeAlias {
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
    previous_indent: &str,
    next_start: u32,
    fallback_indent: &str,
) -> String {
    let anchor = comments_in_span(comments, span)
        .iter()
        .find(|comment| comment.position == CommentPosition::Leading)
        .map_or(next_start, |comment| comment.span.start);
    if let Some(indent) = line_indent_if_standalone(source, anchor)
        .or_else(|| line_indent_before_guard(source, anchor))
    {
        return indent;
    }
    if previous_indent.is_empty() {
        fallback_indent.to_owned()
    } else {
        previous_indent.to_owned()
    }
}

struct LayoutIndents {
    list_bases: Vec<Option<String>>,
    list_items: Vec<Option<String>>,
    switches: Vec<Option<String>>,
    object_bases: Vec<Option<String>>,
    object_items: Vec<Option<String>>,
}

impl LayoutIndents {
    fn new(list_count: usize, switch_count: usize, object_count: usize) -> Self {
        Self {
            list_bases: vec![None; list_count],
            list_items: vec![None; list_count],
            switches: vec![None; switch_count],
            object_bases: vec![None; object_count],
            object_items: vec![None; object_count],
        }
    }

    fn existing_item_indent(
        &mut self,
        source: &str,
        lists: &[StatementList],
        switches: &[SwitchLayout],
        objects: &[ObjectLayout],
        list_index: usize,
        item_index: usize,
    ) -> String {
        for index in (0..=item_index)
            .rev()
            .chain(item_index + 1..lists[list_index].items.len())
        {
            if let Some(indent) =
                line_indent_if_standalone(source, lists[list_index].items[index].span.start)
            {
                return indent;
            }
        }
        self.item_indent(source, lists, switches, objects, list_index)
    }

    fn item_indent(
        &mut self,
        source: &str,
        lists: &[StatementList],
        switches: &[SwitchLayout],
        objects: &[ObjectLayout],
        list_index: usize,
    ) -> String {
        if let Some(indent) = self.list_items[list_index].clone() {
            return indent;
        }
        indent_resolution();
        let indent = match lists[list_index].container {
            ListContainer::Program { .. } => {
                line_indent_at(source, lists[list_index].items[0].span.start)
            }
            ListContainer::Braced { .. } => {
                format!(
                    "{}  ",
                    self.list_base_indent(source, lists, switches, objects, list_index)
                )
            }
            ListContainer::SwitchCase { switch_index, .. } => {
                format!(
                    "{}    ",
                    self.switch_indent(source, lists, switches, objects, switch_index)
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
        objects: &[ObjectLayout],
        list_index: usize,
    ) -> String {
        if let Some(indent) = self.list_bases[list_index].clone() {
            return indent;
        }
        indent_resolution();
        let indent = match lists[list_index].parent_item {
            Some(parent) => self.parent_item_indent(source, lists, switches, objects, parent),
            None => match lists[list_index].container {
                ListContainer::Program { .. } => String::new(),
                ListContainer::Braced { open, .. } => line_indent_at(source, open.start),
                ListContainer::SwitchCase { switch_index, .. } => {
                    self.switch_indent(source, lists, switches, objects, switch_index)
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
        objects: &[ObjectLayout],
        switch_index: usize,
    ) -> String {
        if let Some(indent) = self.switches[switch_index].clone() {
            return indent;
        }
        indent_resolution();
        let switch = &switches[switch_index];
        let indent = if let Some(parent) = switch.parent_item {
            self.parent_item_indent(source, lists, switches, objects, parent)
        } else {
            line_indent_at(source, switch.span.start)
        };
        self.switches[switch_index] = Some(indent.clone());
        indent
    }

    fn interface_base_indent(
        &mut self,
        source: &str,
        lists: &[StatementList],
        switches: &[SwitchLayout],
        objects: &[ObjectLayout],
        interface: &InterfaceBodyLayout,
    ) -> String {
        indent_resolution();
        if let Some(parent) = interface.parent_item {
            self.parent_item_indent(source, lists, switches, objects, parent)
        } else {
            line_indent_at(source, interface.open.start)
        }
    }

    fn object_base_indent(
        &mut self,
        source: &str,
        lists: &[StatementList],
        switches: &[SwitchLayout],
        objects: &[ObjectLayout],
        object_index: usize,
    ) -> String {
        if let Some(indent) = self.object_bases[object_index].clone() {
            return indent;
        }
        indent_resolution();
        let object = &objects[object_index];
        let indent = if object.original_multiline
            && let Some(indent) = line_indent_if_standalone(source, object.close.start)
        {
            indent
        } else {
            let source_indent = line_leading_indent_at(source, object.open.start);
            match object.parent_item {
                Some(parent @ ParentItem::ObjectProperty { .. }) => {
                    self.parent_item_indent(source, lists, switches, objects, parent)
                }
                Some(parent) if source_indent.is_empty() => {
                    self.parent_item_indent(source, lists, switches, objects, parent)
                }
                Some(_) | None => source_indent,
            }
        };
        self.object_bases[object_index] = Some(indent.clone());
        indent
    }

    fn object_item_indent(
        &mut self,
        source: &str,
        lists: &[StatementList],
        switches: &[SwitchLayout],
        objects: &[ObjectLayout],
        object_index: usize,
        item_index: usize,
    ) -> String {
        if let Some(indent) =
            line_indent_if_standalone(source, objects[object_index].items[item_index].span.start)
        {
            return indent;
        }
        if let Some(indent) = self.object_items[object_index].clone() {
            return indent;
        }
        indent_resolution();
        let object = &objects[object_index];
        let base = self.object_base_indent(source, lists, switches, objects, object_index);
        let indent = if object.expanded || object.original_multiline {
            format!("{base}  ")
        } else {
            base
        };
        self.object_items[object_index] = Some(indent.clone());
        indent
    }

    fn parent_item_indent(
        &mut self,
        source: &str,
        lists: &[StatementList],
        switches: &[SwitchLayout],
        objects: &[ObjectLayout],
        parent: ParentItem,
    ) -> String {
        match parent {
            ParentItem::Statement {
                list_index,
                item_index,
            } => {
                if lists[list_index].expanded {
                    self.item_indent(source, lists, switches, objects, list_index)
                } else {
                    self.existing_item_indent(
                        source, lists, switches, objects, list_index, item_index,
                    )
                }
            }
            ParentItem::ObjectProperty {
                object_index,
                item_index,
            } => {
                self.object_item_indent(source, lists, switches, objects, object_index, item_index)
            }
            ParentItem::SwitchCaseLabel { switch_index, .. } => format!(
                "{}  ",
                self.switch_indent(source, lists, switches, objects, switch_index)
            ),
        }
    }
}

fn line_indent_at(source: &str, offset: u32) -> String {
    line_indent_if_standalone(source, offset).unwrap_or_default()
}

fn line_leading_indent_at(source: &str, offset: u32) -> String {
    let prefix = source.get(..offset as usize).unwrap_or(source);
    let line = prefix.rsplit_once('\n').map_or(prefix, |(_, line)| line);
    line.chars()
        .take_while(|character| matches!(character, ' ' | '\t' | '\r'))
        .filter(|character| *character != '\r')
        .collect()
}

fn line_indent_if_standalone(source: &str, offset: u32) -> Option<String> {
    let prefix = source.get(..offset as usize).unwrap_or(source);
    let line = prefix.rsplit_once('\n').map_or(prefix, |(_, line)| line);
    if line
        .chars()
        .all(|character| matches!(character, ' ' | '\t' | '\r'))
    {
        Some(line.trim_end_matches('\r').to_owned())
    } else {
        None
    }
}

fn line_indent_before_guard(source: &str, offset: u32) -> Option<String> {
    let prefix = source.get(..offset as usize).unwrap_or(source);
    let line = prefix.rsplit_once('\n').map_or(prefix, |(_, line)| line);
    let before_guard = line
        .trim_end_matches('\r')
        .trim_end_matches([' ', '\t'])
        .strip_suffix(';')?;
    before_guard
        .chars()
        .all(|character| matches!(character, ' ' | '\t'))
        .then(|| before_guard.to_owned())
}

fn semicolon_after_trailing_comment_end(
    source: &str,
    trailing_end: u32,
    leading_start: u32,
) -> Result<Option<u32>, FormatError> {
    let suffix = source_slice(source, Span::new(trailing_end, leading_start))?;
    if !suffix
        .chars()
        .filter(|character| !character.is_whitespace())
        .eq([';'])
    {
        return Ok(None);
    }
    let Some(relative_end) = suffix.find(';').and_then(|start| start.checked_add(1)) else {
        return Ok(None);
    };
    let relative_end = u32::try_from(relative_end)
        .map_err(|_| FormatError::internal("statement boundary exceeded source spans"))?;
    trailing_end
        .checked_add(relative_end)
        .ok_or_else(|| FormatError::internal("statement boundary exceeded source spans"))
        .map(Some)
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
        let original = source_slice(source, span)?;
        return if original.chars().all(char::is_whitespace) {
            Ok(format!("{statement_separator}{indent}"))
        } else if original
            .chars()
            .filter(|character| !character.is_whitespace())
            .eq([';'])
        {
            Ok(format!(";{statement_separator}{indent}"))
        } else {
            Ok(original.to_owned())
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

    if let Some(trailing) = boundary_comments
        .iter()
        .rev()
        .find(|comment| comment.position == CommentPosition::Trailing)
    {
        let prefix = source_slice(source, Span::new(span.start, trailing.span.start))?;
        if prefix
            .chars()
            .filter(|character| !character.is_whitespace())
            .eq([';'])
            && let Some(relative_start) = prefix.find(';')
        {
            let relative_start = u32::try_from(relative_start)
                .map_err(|_| FormatError::internal("statement boundary exceeded source spans"))?;
            let semicolon_start = span
                .start
                .checked_add(relative_start)
                .ok_or_else(|| FormatError::internal("statement boundary exceeded source spans"))?;
            let mut output =
                source_slice(source, Span::new(semicolon_start, trailing.span.end))?.to_owned();
            output.push_str(statement_separator);
            output.push_str(indent);
            append_leading_comments_and_destination(
                &mut output,
                source,
                boundary_comments,
                leading_start,
                span.end,
                statement_separator,
                indent,
            )?;
            return Ok(output);
        }
    }

    if let Some(semicolon_end) =
        semicolon_after_trailing_comment_end(source, trailing_end, leading_start)?
    {
        let mut output = source_slice(source, Span::new(span.start, semicolon_end))?.to_owned();
        output.push_str(statement_separator);
        output.push_str(indent);
        append_leading_comments_and_destination(
            &mut output,
            source,
            boundary_comments,
            leading_start,
            span.end,
            statement_separator,
            indent,
        )?;
        return Ok(output);
    }

    let mut output = String::new();
    output.push_str(source_slice(source, Span::new(span.start, trailing_end))?);
    output.push_str(statement_separator);
    output.push_str(indent);
    append_leading_comments_and_destination(
        &mut output,
        source,
        boundary_comments,
        leading_start,
        span.end,
        statement_separator,
        indent,
    )?;
    Ok(output)
}

#[allow(
    clippy::too_many_arguments,
    reason = "leading comments need the complete boundary and target indentation"
)]
fn append_leading_comments_and_destination(
    output: &mut String,
    source: &str,
    comments: &[Comment],
    leading_start: u32,
    destination_start: u32,
    separator: &str,
    indent: &str,
) -> Result<(), FormatError> {
    let Some(last_leading_comment) = comments
        .iter()
        .rev()
        .find(|comment| comment.position == CommentPosition::Leading)
    else {
        return Ok(());
    };
    output.push_str(source_slice(
        source,
        Span::new(leading_start, last_leading_comment.span.end),
    )?);
    append_comment_gap(
        output,
        source_slice(
            source,
            Span::new(last_leading_comment.span.end, destination_start),
        )?,
        separator,
        indent,
    );
    Ok(())
}

fn append_comment_gap(output: &mut String, gap: &str, separator: &str, indent: &str) {
    if contains_line_break(gap) && gap.chars().all(char::is_whitespace) {
        let newline = detect_newline(separator, None);
        output.push_str(newline);
        if has_blank_line(gap) {
            output.push_str(newline);
        }
        output.push_str(indent);
    } else {
        output.push_str(gap);
    }
}

fn has_blank_line(text: &str) -> bool {
    let mut line_breaks = 0;
    let mut bytes = text.bytes().peekable();
    while let Some(byte) = bytes.next() {
        match byte {
            b'\r' => {
                line_breaks += 1;
                if bytes.peek() == Some(&b'\n') {
                    bytes.next();
                }
            }
            b'\n' => line_breaks += 1,
            _ => continue,
        }
        if line_breaks >= 2 {
            return true;
        }
    }
    false
}

struct FormattedImport {
    text: String,
    multiline: bool,
}

#[derive(Clone, Copy)]
struct ImportSemicolonShape {
    original: bool,
    formatted: bool,
}

impl ImportSemicolonShape {
    const fn adjust_width(self, width: usize) -> usize {
        match (self.original, self.formatted) {
            (true, false) => width.saturating_sub(1),
            (false, true) => width.saturating_add(1),
            _ => width,
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "the import formatter needs both AST and effective source-span evidence"
)]
fn format_import(
    declaration: &ImportDeclaration<'_>,
    span: Span,
    source: &str,
    base_indent: &str,
    tokens: &[Token],
    comments: &[Comment],
    line_width: u32,
    newline: &str,
    trailing_commas: TrailingCommaMode,
    semicolon_shape: ImportSemicolonShape,
) -> Result<FormattedImport, FormatError> {
    let omitted_attribute_comma = (trailing_commas == TrailingCommaMode::Never)
        .then(|| import_attribute_trailing_comma(declaration, tokens))
        .flatten();
    let named_braces = named_braces(declaration, tokens);
    let text = if let Some((left_brace, right_brace)) = named_braces {
        let flat = format_named_import(
            span,
            left_brace,
            right_brace,
            source,
            tokens,
            comments,
            newline,
            false,
            base_indent,
            trailing_commas,
            omitted_attribute_comma,
        )?;
        let effective_width = semicolon_shape
            .adjust_width(flat.chars().count())
            .saturating_add(base_indent.chars().count());
        if contains_line_break(&flat) || effective_width > line_width as usize {
            format_named_import(
                span,
                left_brace,
                right_brace,
                source,
                tokens,
                comments,
                newline,
                true,
                base_indent,
                trailing_commas,
                omitted_attribute_comma,
            )?
        } else {
            flat
        }
    } else {
        canonicalize_range(
            span,
            source,
            tokens,
            comments,
            newline,
            false,
            omitted_attribute_comma,
        )?
        .text
    };

    Ok(FormattedImport {
        multiline: contains_line_break(&text),
        text,
    })
}

fn import_attribute_trailing_comma(
    declaration: &ImportDeclaration<'_>,
    tokens: &[Token],
) -> Option<Span> {
    let clause = declaration.with_clause.as_ref()?;
    let first = clause.with_entries.first()?;
    let last = clause.with_entries.last()?;
    let (open, close) = surrounding_delimiters(
        tokens,
        clause.span,
        Kind::LCurly,
        Kind::RCurly,
        first.span(),
        last.span(),
    )?;
    list_tokens(tokens, open, close)?.trailing_comma
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
    base_indent: &str,
    trailing_commas: TrailingCommaMode,
    omitted_attribute_comma: Option<Span>,
) -> Result<String, FormatError> {
    let prefix = canonicalize_range(
        Span::new(declaration_span.start, left_brace.start),
        source,
        tokens,
        comments,
        newline,
        false,
        None,
    )?;
    let suffix = canonicalize_range(
        Span::new(right_brace.end, declaration_span.end),
        source,
        tokens,
        comments,
        newline,
        false,
        omitted_attribute_comma,
    )?;
    let ranges = named_segments(left_brace.end, right_brace.start, tokens);
    let preserve_trailing_comma = trailing_commas != TrailingCommaMode::Never
        && tokens_in_span(tokens, Span::new(left_brace.end, right_brace.start))
            .last()
            .is_some_and(|token| token.kind() == Kind::Comma);
    let last_token_segment = ranges
        .iter()
        .rposition(|range| range_has_token(*range, tokens));
    let mut segments = Vec::new();
    for (index, range) in ranges.into_iter().enumerate() {
        let has_token = range_has_token(range, tokens);
        let add_comma = has_token
            && last_token_segment
                .is_some_and(|last| index < last || (index == last && preserve_trailing_comma));
        let segment =
            canonicalize_range(range, source, tokens, comments, newline, add_comma, None)?;
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
        let item_indent = format!("{base_indent}  ");
        output.push_str(newline);
        for segment in &segments {
            output.push_str(&indent_lines(&segment.text, &item_indent));
            output.push_str(newline);
        }
        output.push_str(base_indent);
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
    omitted_token: Option<Span>,
) -> Result<CanonicalText, FormatError> {
    let tokens = tokens_in_span(tokens, range);
    let comments = comments_in_span(comments, range);
    let mut items = Vec::new();
    for token in tokens
        .iter()
        .filter(|token| omitted_token != Some(token.span()))
    {
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

pub(crate) fn detect_newline(source: &str, fallback: Option<&'static str>) -> &'static str {
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
    fallback.unwrap_or("\n")
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
/// Returns the same parse and source-type errors as [`crate::format_text`].
pub fn benchmark_parse(file_name: &Path, source: &str) -> Result<(), FormatError> {
    let allocator = Allocator::default();
    let source_type = crate::document::script_source_type(file_name)
        .ok_or_else(|| FormatError::unsupported_source(file_name))?;
    parse(&allocator, source, source_type)?;
    Ok(())
}

#[cfg(feature = "benchmarking")]
/// Runs source rewriting with the supplied configuration.
///
/// # Errors
///
/// Returns the same errors as [`crate::format_text`].
pub fn benchmark_rewrite(
    file_name: &Path,
    source: &str,
    config: &ResolvedConfig,
) -> Result<Option<String>, FormatError> {
    crate::format_text(file_name, source, config)
}

#[cfg(feature = "benchmarking")]
/// Parses benchmark input and verifies it against a second parse.
///
/// # Errors
///
/// Returns the same parse, source-type, and verification errors as [`crate::format_text`].
pub fn benchmark_verify(file_name: &Path, source: &str) -> Result<(), FormatError> {
    let source_type = crate::document::script_source_type(file_name)
        .ok_or_else(|| FormatError::unsupported_source(file_name))?;
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
        CORRUPT_REWRITE_FOR_TEST, DEFERRED_IMPORT_BOUNDARY_LOOKUPS, IMPORT_MULTILINE_SCANS,
        INDENT_RESOLUTIONS, LINE_BREAK_INDEX_BUILDS, LINE_BREAK_QUERIES, LINE_START_INDEX_QUERIES,
        PARENTHESIS_INDEX_BUILDS, PARENTHESIS_LOOKUPS, RAW_LINE_START_SCANS,
        SPAN_LOOKUP_COMPARISONS, TOKEN_PARSER_RUNS, TOKEN_PREFLIGHT_PARSES,
        TYPE_ALIAS_MULTILINE_SCANS, VARIABLE_MULTILINE_SCANS, parse, verify,
    };
    use crate::{
        FormatConfig, InterfaceLayoutMode, InterfaceLayoutRule, RulesConfig, SemicolonConfig,
        SemicolonMode, StatementSpacingConfig, StatementSpacingMode, TrailingCommaMode,
        TypeMemberSemicolonConfig, TypeMemberSemicolonRule, format_text, resolve_config,
    };

    fn format(source: &str) -> String {
        format_with_semicolons_off(source, FormatConfig::default())
    }

    fn format_with_semicolons_off(source: &str, mut config: FormatConfig) -> String {
        config.rules.semicolons = semicolons_off();
        format_file_with("sample.ts", source, config)
    }

    fn format_file_with(file_name: &str, source: &str, config: FormatConfig) -> String {
        let config = resolve_config(config).unwrap();
        format_text(Path::new(file_name), source, &config)
            .unwrap()
            .unwrap_or_else(|| source.to_owned())
    }

    fn semicolons_off() -> SemicolonConfig {
        SemicolonConfig {
            statements: SemicolonMode::Off,
            class_members: SemicolonMode::Off,
            type_members: SemicolonMode::Off.into(),
        }
    }

    fn object_spacing_config(enabled: bool) -> FormatConfig {
        FormatConfig {
            rules: RulesConfig {
                import_layout: false,
                interface_layout: InterfaceLayoutRule::Mode(InterfaceLayoutMode::Off),
                object_property_spacing: enabled,
                statement_spacing: StatementSpacingConfig {
                    control_flow_statements: StatementSpacingMode::Off,
                    imports: StatementSpacingMode::Off,
                    multiline_call_statements: StatementSpacingMode::Off,
                    return_statements: StatementSpacingMode::Off,
                    type_aliases: StatementSpacingMode::Off,
                    variable_declarations: StatementSpacingMode::Off,
                },
                semicolons: semicolons_off(),
                trailing_commas: TrailingCommaMode::Off,
            },
            ..FormatConfig::default()
        }
    }

    fn format_object(source: &str) -> String {
        format_file_with("sample.ts", source, object_spacing_config(true))
    }

    #[test]
    fn spaces_object_properties_from_their_final_multiline_shape() {
        for (source, expected) in [
            (
                "const value = {\n  first: 1,\n\n\n  second: 2\n}",
                "const value = {\n  first: 1,\n  second: 2\n}",
            ),
            (
                "const value = {\n  first: 1,\n  second: [\n    2\n  ]\n}",
                "const value = {\n  first: 1,\n\n  second: [\n    2\n  ]\n}",
            ),
            (
                "const value = {\n  first: [\n    1\n  ],\n  second: 2\n}",
                "const value = {\n  first: [\n    1\n  ],\n\n  second: 2\n}",
            ),
            (
                "const value = {\n  first: [\n    1\n  ],\n\n\n  second: [\n    2\n  ]\n}",
                "const value = {\n  first: [\n    1\n  ],\n\n  second: [\n    2\n  ]\n}",
            ),
        ] {
            let output = format_object(source);
            assert_eq!(output, expected, "{source}");
            assert_eq!(format_object(&output), output, "{source}");
        }
    }

    #[test]
    fn expands_inline_objects_recursively_in_one_run() {
        assert_eq!(
            format_object("const value = { first: 1, second: 2 }"),
            "const value = {\n  first: 1,\n  second: 2\n}"
        );
        assert_eq!(
            format_object("const empty = {}; const one = { value: 1 }"),
            "const empty = {}; const one = { value: 1 }"
        );
        assert_eq!(
            format_object("const value = { first: { nested: 1, sibling: 2 }, second: 3 }"),
            "const value = {\n  first: {\n    nested: 1,\n    sibling: 2\n  },\n\n  second: 3\n}"
        );
        assert_eq!(
            format_object("function run(){const value={first:1,second:2};done();}"),
            "function run(){\n  const value={\n    first:1,\n    second:2\n  };\n  done();\n}"
        );
    }

    #[test]
    fn preserves_host_indentation_for_expanded_objects() {
        for (source, expected) in [
            (
                "call(\n  {first:1,second:2}\n)",
                "call(\n  {\n    first:1,\n    second:2\n  }\n)",
            ),
            (
                "class Example {\n  value = {first:1,second:2}\n}",
                "class Example {\n  value = {\n    first:1,\n    second:2\n  }\n}",
            ),
        ] {
            let output = format_object(source);
            assert_eq!(output, expected, "{source}");
            assert_eq!(format_object(&output), output, "{source}");
        }
    }

    #[test]
    fn formats_every_direct_object_element_kind_together() {
        let source = "const value = { regular: 1, shorthand, [key]: 2, method() { return 1 }, get result() { return 1 }, set result(next) { value = next }, ...rest }";
        assert_eq!(
            format_object(source),
            "const value = {\n  regular: 1,\n  shorthand,\n  [key]: 2,\n  method() { return 1 },\n  get result() { return 1 },\n  set result(next) { value = next },\n  ...rest\n}"
        );
    }

    #[test]
    fn leaves_non_object_expression_families_unchanged() {
        let source = "const { a, b } = input; ({ a, b } = input); type Shape = { a: 1; b: 2 }; interface Pair { a: 1; b: 2 } class Pair { a = 1; b = 2 } import { a, b } from 'pkg'; export { a, b };";
        assert_eq!(format_object(source), source);
    }

    #[test]
    fn disabled_object_property_spacing_is_a_complete_no_op() {
        let source = "const value = { first: 1, second: { nested: 2, sibling: 3 } };";
        let config = resolve_config(object_spacing_config(false)).unwrap();
        assert!(
            format_text(Path::new("sample.ts"), source, &config)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn cascades_statement_layout_into_object_property_spacing() {
        let source = "const value={first:1,method(){before();if(ok)work();after();},last:3}";
        let mut config = object_spacing_config(true);
        config.rules.statement_spacing.control_flow_statements = StatementSpacingMode::Separate;
        assert_eq!(
            format_file_with("sample.ts", source, config),
            "const value={\n  first:1,\n\n  method(){\n    before();\n\n    if(ok)work();\n\n    after();\n  },\n\n  last:3\n}"
        );
    }

    #[test]
    fn propagates_lone_statement_layout_into_object_property_spacing() {
        let source = "const value={first(){return (()=>{work();return value})()},second:2}";
        let mut config = object_spacing_config(true);
        config.rules.statement_spacing.return_statements = StatementSpacingMode::Separate;
        assert_eq!(
            format_file_with("sample.ts", source, config),
            "const value={\n  first(){return (()=>{\n      work();\n\n      return value\n    })()},\n\n  second:2\n}"
        );
    }

    #[test]
    fn indents_expanded_objects_in_switch_case_labels() {
        let source = "switch(value){case {first:1,second:2}:work();break;default:break;}";
        assert_eq!(
            format_object(source),
            "switch(value){\n  case {\n    first:1,\n    second:2\n  }:\n    work();\n    break;\n  default:break;\n}"
        );
    }

    #[test]
    fn keeps_object_boundary_comments_and_typescript_directives_attached() {
        let source = "const value={/* first */ first:1, // first trailing\n// @ts-ignore\nsecond:2, // second trailing\n/* third leading */ third:[\n  3\n] /* third trailing */}";
        assert_eq!(
            format_object(source),
            "const value={\n  /* first */ first:1, // first trailing\n// @ts-ignore\nsecond:2, // second trailing\n\n/* third leading */ third:[\n  3\n] /* third trailing */\n}"
        );
    }

    #[test]
    fn preserves_detached_comments_between_object_properties() {
        let source = "const value={first:1,\n  /** section */\n\n\n  second:2}";
        let expected = "const value={\n  first:1,\n  /** section */\n\n  second:2\n}";
        let output = format_object(source);

        assert_eq!(output, expected);
        assert_eq!(format_object(&output), output);
    }

    #[test]
    fn preserves_object_indentation_after_multiline_boundary_comments() {
        let source = "const value = { first: 1, /* second\nlead */ second: 2 }";
        let expected = "const value = {\n  first: 1, /* second\nlead */\n  second: 2\n}";
        let output = format_object(source);
        assert_eq!(output, expected);
        assert_eq!(format_object(&output), output);
    }

    #[test]
    fn keeps_object_spacing_independent_from_optional_trailing_commas() {
        for (mode, source, expected) in [
            (
                TrailingCommaMode::Never,
                "const value={first:1,second:2,};",
                "const value={\n  first:1,\n  second:2\n};",
            ),
            (
                TrailingCommaMode::Always,
                "const value={first:1,second:2};",
                "const value={\n  first:1,\n  second:2,\n};",
            ),
            (
                TrailingCommaMode::Off,
                "const value={first:1,second:2,};",
                "const value={\n  first:1,\n  second:2,\n};",
            ),
        ] {
            let mut config = object_spacing_config(true);
            config.rules.trailing_commas = mode;
            assert_eq!(format_file_with("sample.ts", source, config), expected);
        }
    }

    #[test]
    fn keeps_object_spacing_independent_from_statement_semicolon_modes() {
        for (mode, source, expected) in [
            (
                SemicolonMode::Off,
                "const value={first:1,second:2};",
                "const value={\n  first:1,\n  second:2\n};",
            ),
            (
                SemicolonMode::AsNeeded,
                "const value={first:1,second:2};",
                "const value={\n  first:1,\n  second:2\n}",
            ),
            (
                SemicolonMode::Always,
                "const value={first:1,second:2}",
                "const value={\n  first:1,\n  second:2\n};",
            ),
        ] {
            let mut config = object_spacing_config(true);
            config.rules.semicolons.statements = mode;
            assert_eq!(format_file_with("sample.ts", source, config), expected);
        }
    }

    #[test]
    fn preserves_bom_newlines_eof_shape_and_vue_host_indentation() {
        assert_eq!(
            format_file_with(
                "sample.ts",
                "\u{feff}const value={first:1,second:2}",
                object_spacing_config(true),
            ),
            "\u{feff}const value={\n  first:1,\n  second:2\n}"
        );
        assert_eq!(
            format_file_with(
                "sample.ts",
                "const value={first:1,second:2}\r\n",
                object_spacing_config(true),
            ),
            "const value={\r\n  first:1,\r\n  second:2\r\n}\r\n"
        );
        assert_eq!(
            format_file_with(
                "sample.vue",
                "<script>\n  const value={first:1,second:2}\n</script>\n",
                object_spacing_config(true),
            ),
            "<script>\n  const value={\n    first:1,\n    second:2\n  }\n</script>\n"
        );
    }

    #[test]
    fn verifies_object_layout_ast_and_is_idempotent() {
        let source = "const value={first:{nested:1,sibling:2},second:3}";
        let config = resolve_config(object_spacing_config(true)).unwrap();
        let output = format_text(Path::new("sample.ts"), source, &config)
            .unwrap()
            .unwrap();
        assert!(
            format_text(Path::new("sample.ts"), &output, &config)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn nested_object_indent_resolution_stays_linear() {
        let depth = 128;
        let mut source = String::from("const value=");
        for _ in 0..depth {
            source.push_str("{nested:");
        }
        source.push('0');
        for index in 0..depth {
            write!(source, ",sibling:{index}}}").unwrap();
        }
        let mut config = object_spacing_config(true);
        config.verify_ast = false;
        let config = resolve_config(config).unwrap();

        INDENT_RESOLUTIONS.set(0);
        format_text(Path::new("nested-objects.ts"), &source, &config).unwrap();
        let resolutions = INDENT_RESOLUTIONS.get();
        assert!(resolutions >= depth);
        assert!(
            resolutions < depth * 4,
            "indent resolution performed {resolutions} steps for {depth} nested objects"
        );
    }

    #[test]
    fn object_line_break_queries_stay_indexed() {
        let depth = 128;
        let mut nested = String::from("const value=");
        for _ in 0..depth {
            nested.push_str("{nested:");
        }
        nested.push('0');
        for _ in 0..depth {
            nested.push('}');
        }
        let mut config = object_spacing_config(true);
        config.verify_ast = false;
        let config = resolve_config(config).unwrap();

        LINE_BREAK_INDEX_BUILDS.set(0);
        LINE_BREAK_QUERIES.set(0);
        assert!(
            format_text(Path::new("nested-objects.ts"), &nested, &config)
                .unwrap()
                .is_none()
        );
        assert_eq!(LINE_BREAK_INDEX_BUILDS.get(), 1);
        assert_eq!(LINE_BREAK_QUERIES.get(), depth * 2);

        let property_count = 256;
        let mut commented = String::from("const value={");
        for index in 0..property_count {
            if index > 0 {
                commented.push_str(",/* next */");
            }
            write!(commented, "p{index}:{index}").unwrap();
        }
        commented.push('}');

        LINE_START_INDEX_QUERIES.set(0);
        RAW_LINE_START_SCANS.set(0);
        format_text(Path::new("commented-object.ts"), &commented, &config).unwrap();
        assert_eq!(RAW_LINE_START_SCANS.get(), 0);
        assert_eq!(LINE_START_INDEX_QUERIES.get(), (property_count - 1) * 2);
    }

    fn format_semicolons(file_name: &str, source: &str, semicolons: SemicolonConfig) -> String {
        format_file_with(
            file_name,
            source,
            FormatConfig {
                rules: RulesConfig {
                    import_layout: false,
                    interface_layout: InterfaceLayoutRule::Mode(InterfaceLayoutMode::Off),
                    object_property_spacing: false,
                    statement_spacing: StatementSpacingConfig {
                        control_flow_statements: StatementSpacingMode::Off,
                        imports: StatementSpacingMode::Off,
                        multiline_call_statements: StatementSpacingMode::Off,
                        return_statements: StatementSpacingMode::Off,
                        type_aliases: StatementSpacingMode::Off,
                        variable_declarations: StatementSpacingMode::Off,
                    },
                    semicolons,
                    trailing_commas: TrailingCommaMode::Off,
                },
                ..FormatConfig::default()
            },
        )
    }

    fn format_with_rules(
        source: &str,
        import_layout: bool,
        imports: StatementSpacingMode,
        variable_declarations: StatementSpacingMode,
    ) -> String {
        format_with_statement_spacing(
            source,
            import_layout,
            imports,
            StatementSpacingMode::Off,
            variable_declarations,
        )
    }

    fn format_with_statement_spacing(
        source: &str,
        import_layout: bool,
        imports: StatementSpacingMode,
        type_aliases: StatementSpacingMode,
        variable_declarations: StatementSpacingMode,
    ) -> String {
        format_with_semicolons_off(
            source,
            FormatConfig {
                rules: RulesConfig {
                    import_layout,
                    statement_spacing: StatementSpacingConfig {
                        control_flow_statements: StatementSpacingMode::Off,
                        imports,
                        multiline_call_statements: StatementSpacingMode::Off,
                        return_statements: StatementSpacingMode::Off,
                        type_aliases,
                        variable_declarations,
                    },
                    ..RulesConfig::default()
                },
                ..FormatConfig::default()
            },
        )
    }

    fn format_with_return_spacing(source: &str, return_statements: StatementSpacingMode) -> String {
        format_with_semicolons_off(
            source,
            FormatConfig {
                rules: RulesConfig {
                    import_layout: false,
                    interface_layout: InterfaceLayoutRule::Mode(InterfaceLayoutMode::Off),
                    object_property_spacing: false,
                    statement_spacing: StatementSpacingConfig {
                        control_flow_statements: StatementSpacingMode::Off,
                        imports: StatementSpacingMode::Off,
                        multiline_call_statements: StatementSpacingMode::Off,
                        return_statements,
                        type_aliases: StatementSpacingMode::Off,
                        variable_declarations: StatementSpacingMode::Off,
                    },
                    trailing_commas: TrailingCommaMode::Off,
                    ..RulesConfig::default()
                },
                ..FormatConfig::default()
            },
        )
    }

    fn format_with_control_flow_spacing(
        source: &str,
        control_flow_statements: StatementSpacingMode,
    ) -> String {
        format_with_semicolons_off(
            source,
            FormatConfig {
                rules: RulesConfig {
                    import_layout: false,
                    interface_layout: InterfaceLayoutRule::Mode(InterfaceLayoutMode::Off),
                    object_property_spacing: false,
                    statement_spacing: StatementSpacingConfig {
                        control_flow_statements,
                        imports: StatementSpacingMode::Off,
                        multiline_call_statements: StatementSpacingMode::Off,
                        return_statements: StatementSpacingMode::Off,
                        type_aliases: StatementSpacingMode::Off,
                        variable_declarations: StatementSpacingMode::Off,
                    },
                    trailing_commas: TrailingCommaMode::Off,
                    ..RulesConfig::default()
                },
                ..FormatConfig::default()
            },
        )
    }

    fn format_with_multiline_call_spacing(
        file_name: &str,
        source: &str,
        mode: StatementSpacingMode,
    ) -> String {
        let mut config = object_spacing_config(false);
        config.rules.statement_spacing.multiline_call_statements = mode;
        format_file_with(file_name, source, config)
    }

    fn format_trailing(source: &str, mode: TrailingCommaMode) -> String {
        format_with_semicolons_off(
            source,
            FormatConfig {
                rules: RulesConfig {
                    import_layout: false,
                    interface_layout: InterfaceLayoutRule::Mode(InterfaceLayoutMode::Off),
                    object_property_spacing: false,
                    statement_spacing: StatementSpacingConfig {
                        control_flow_statements: StatementSpacingMode::Off,
                        imports: StatementSpacingMode::Off,
                        multiline_call_statements: StatementSpacingMode::Off,
                        return_statements: StatementSpacingMode::Off,
                        type_aliases: StatementSpacingMode::Off,
                        variable_declarations: StatementSpacingMode::Off,
                    },
                    semicolons: semicolons_off(),
                    trailing_commas: mode,
                },
                ..FormatConfig::default()
            },
        )
    }

    fn format_trailing_file(file_name: &str, source: &str, mode: TrailingCommaMode) -> String {
        format_file_with(
            file_name,
            source,
            FormatConfig {
                rules: RulesConfig {
                    import_layout: false,
                    interface_layout: InterfaceLayoutRule::Mode(InterfaceLayoutMode::Off),
                    object_property_spacing: false,
                    statement_spacing: StatementSpacingConfig {
                        control_flow_statements: StatementSpacingMode::Off,
                        imports: StatementSpacingMode::Off,
                        multiline_call_statements: StatementSpacingMode::Off,
                        return_statements: StatementSpacingMode::Off,
                        type_aliases: StatementSpacingMode::Off,
                        variable_declarations: StatementSpacingMode::Off,
                    },
                    semicolons: semicolons_off(),
                    trailing_commas: mode,
                },
                ..FormatConfig::default()
            },
        )
    }

    fn format_interface_layout(
        file_name: &str,
        source: &str,
        interface_layout: InterfaceLayoutRule,
        type_members: SemicolonMode,
    ) -> String {
        format_file_with(
            file_name,
            source,
            FormatConfig {
                rules: RulesConfig {
                    import_layout: false,
                    interface_layout,
                    object_property_spacing: false,
                    statement_spacing: StatementSpacingConfig {
                        control_flow_statements: StatementSpacingMode::Off,
                        imports: StatementSpacingMode::Off,
                        multiline_call_statements: StatementSpacingMode::Off,
                        return_statements: StatementSpacingMode::Off,
                        type_aliases: StatementSpacingMode::Off,
                        variable_declarations: StatementSpacingMode::Off,
                    },
                    semicolons: SemicolonConfig {
                        statements: SemicolonMode::Off,
                        class_members: SemicolonMode::Off,
                        type_members: type_members.into(),
                    },
                    trailing_commas: TrailingCommaMode::Off,
                },
                ..FormatConfig::default()
            },
        )
    }

    #[test]
    fn formats_interfaces_only_above_the_configured_member_threshold() {
        let off = InterfaceLayoutRule::Mode(InterfaceLayoutMode::Off);
        let one_member = "interface One { value: string; }";
        assert_eq!(
            format_interface_layout("sample.ts", one_member, off, SemicolonMode::Off),
            one_member
        );
        assert_eq!(
            format_interface_layout(
                "sample.ts",
                one_member,
                InterfaceLayoutRule::Threshold(1),
                SemicolonMode::Off,
            ),
            one_member
        );
        assert_eq!(
            format_interface_layout(
                "sample.ts",
                "interface Empty {}",
                InterfaceLayoutRule::Threshold(0),
                SemicolonMode::Off,
            ),
            "interface Empty {}"
        );
        assert_eq!(
            format_interface_layout(
                "sample.ts",
                "interface Two { value: string; run(): void, }",
                InterfaceLayoutRule::Threshold(1),
                SemicolonMode::Off,
            ),
            "interface Two {\n  value: string;\n  run(): void,\n}"
        );
    }

    #[test]
    fn keeps_interface_layout_independent_from_line_width() {
        let source = "interface Shape { value: string; }";
        let expected = "interface Shape {\n  value: string;\n}";
        for line_width in [1, 1_000] {
            let output = format_file_with(
                "sample.ts",
                source,
                FormatConfig {
                    line_width,
                    rules: RulesConfig {
                        import_layout: false,
                        interface_layout: InterfaceLayoutRule::Threshold(0),
                        object_property_spacing: false,
                        statement_spacing: StatementSpacingConfig {
                            control_flow_statements: StatementSpacingMode::Off,
                            imports: StatementSpacingMode::Off,
                            multiline_call_statements: StatementSpacingMode::Off,
                            return_statements: StatementSpacingMode::Off,
                            type_aliases: StatementSpacingMode::Off,
                            variable_declarations: StatementSpacingMode::Off,
                        },
                        semicolons: semicolons_off(),
                        trailing_commas: TrailingCommaMode::Off,
                    },
                    ..FormatConfig::default()
                },
            );
            assert_eq!(output, expected);
        }
    }

    #[test]
    fn counts_and_places_every_interface_member_kind_on_its_own_line() {
        let source = "interface Shape { value: string; run(): void; [key: string]: unknown; (): string; new (): Shape; }";
        let output = format_interface_layout(
            "sample.ts",
            source,
            InterfaceLayoutRule::Threshold(4),
            SemicolonMode::Off,
        );
        assert_eq!(
            output,
            "interface Shape {\n  value: string;\n  run(): void;\n  [key: string]: unknown;\n  (): string;\n  new (): Shape;\n}"
        );
    }

    #[test]
    fn canonicalizes_triggered_interface_boundaries_without_collapsing_small_interfaces() {
        let source = "interface Shape {\n    first: string; second: number;\n  }";
        assert_eq!(
            format_interface_layout(
                "sample.ts",
                source,
                InterfaceLayoutRule::Threshold(1),
                SemicolonMode::Off,
            ),
            "interface Shape {\n  first: string;\n  second: number;\n}"
        );
        assert_eq!(
            format_interface_layout(
                "sample.ts",
                source,
                InterfaceLayoutRule::Threshold(2),
                SemicolonMode::Off,
            ),
            source
        );
    }

    #[test]
    fn preserves_line_scoped_typescript_directives_when_expanding_interfaces() {
        for directive in ["@ts-ignore", "@ts-expect-error"] {
            let source = format!(
                "interface Shape {{\n  first: string; // {directive}\n  second: MissingOne; third: MissingTwo;\n}}"
            );
            let expected = format!(
                "interface Shape {{\n  first: string // {directive}\n  second: MissingOne; third: MissingTwo\n}}"
            );
            let output = format_interface_layout(
                "sample.ts",
                &source,
                InterfaceLayoutRule::Threshold(0),
                SemicolonMode::AsNeeded,
            );

            assert_eq!(output, expected);
            assert_eq!(
                format_interface_layout(
                    "sample.ts",
                    &output,
                    InterfaceLayoutRule::Threshold(0),
                    SemicolonMode::AsNeeded,
                ),
                output
            );
        }
    }

    #[test]
    fn normalizes_interface_member_indentation_after_leading_comments() {
        for (source, expected) in [
            (
                "interface Shape { first: string;\n// second\nsecond: number; }",
                "interface Shape {\n  first: string;\n  // second\n  second: number;\n}",
            ),
            (
                "interface Shape {\n      /** value */\n        value: string;\n}",
                "interface Shape {\n  /** value */\n  value: string;\n}",
            ),
        ] {
            let output = format_interface_layout(
                "sample.ts",
                source,
                InterfaceLayoutRule::Threshold(0),
                SemicolonMode::Off,
            );

            assert_eq!(output, expected);
            assert_eq!(
                format_interface_layout(
                    "sample.ts",
                    &output,
                    InterfaceLayoutRule::Threshold(0),
                    SemicolonMode::Off,
                ),
                output
            );
        }
    }

    #[test]
    fn preserves_detached_comments_between_interface_members() {
        let source = "interface Shape { first: string;\n/** section */\n\n\nsecond: number; }";
        let expected =
            "interface Shape {\n  first: string;\n  /** section */\n\n  second: number;\n}";
        let output = format_interface_layout(
            "sample.ts",
            source,
            InterfaceLayoutRule::Threshold(0),
            SemicolonMode::Off,
        );

        assert_eq!(output, expected);
        assert_eq!(
            format_interface_layout(
                "sample.ts",
                &output,
                InterfaceLayoutRule::Threshold(0),
                SemicolonMode::Off,
            ),
            output
        );
    }

    #[test]
    fn cascades_interface_layout_through_inline_declaration_containers() {
        let source = "declare namespace Outer { namespace Inner { export interface Shape { value: string; run(): void; } const after=1; } }";
        let output = format_interface_layout(
            "sample.d.ts",
            source,
            InterfaceLayoutRule::Threshold(0),
            SemicolonMode::Off,
        );
        assert_eq!(
            output,
            "declare namespace Outer {\n  namespace Inner {\n    export interface Shape {\n      value: string;\n      run(): void;\n    }\n    const after=1;\n  }\n}"
        );
    }

    #[test]
    fn cascades_interface_layout_through_program_and_block_statement_lists() {
        let program = "before();interface Shape { value: string; }after();";
        assert_eq!(
            format_interface_layout(
                "sample.ts",
                program,
                InterfaceLayoutRule::Threshold(0),
                SemicolonMode::Off,
            ),
            "before();\ninterface Shape {\n  value: string;\n}\nafter();"
        );

        let block = "function scope() { before(); interface Local { value: string; } after(); }";
        assert_eq!(
            format_interface_layout(
                "sample.ts",
                block,
                InterfaceLayoutRule::Threshold(0),
                SemicolonMode::Off,
            ),
            "function scope() {\n  before();\n  interface Local {\n    value: string;\n  }\n  after();\n}"
        );
    }

    #[test]
    fn cascades_interface_layout_across_inline_boundaries_in_multiline_containers() {
        for (file_name, source, expected) in [
            (
                "sample.ts",
                "function scope() {\n  before(); interface Local { value: string; } after();\n}",
                "function scope() {\n  before();\n  interface Local {\n    value: string;\n  }\n  after();\n}",
            ),
            (
                "sample.d.ts",
                "namespace Scope {\n  type Before = string; interface Shape { value: string; } type After = number;\n}",
                "namespace Scope {\n  type Before = string;\n  interface Shape {\n    value: string;\n  }\n  type After = number;\n}",
            ),
        ] {
            let output = format_interface_layout(
                file_name,
                source,
                InterfaceLayoutRule::Threshold(0),
                SemicolonMode::Off,
            );

            assert_eq!(output, expected);
            assert_eq!(
                format_interface_layout(
                    file_name,
                    &output,
                    InterfaceLayoutRule::Threshold(0),
                    SemicolonMode::Off,
                ),
                output
            );
        }
    }

    #[test]
    fn applies_interface_layout_to_exported_ambient_and_definition_declarations() {
        let source = "export interface Public { value: string; }\ndeclare interface Ambient { run(): void; }";
        let output = format_interface_layout(
            "sample.d.ts",
            source,
            InterfaceLayoutRule::Threshold(0),
            SemicolonMode::Off,
        );
        assert_eq!(
            output,
            "export interface Public {\n  value: string;\n}\ndeclare interface Ambient {\n  run(): void;\n}"
        );
    }

    #[test]
    fn keeps_interface_layout_independent_from_type_member_semicolons() {
        let source = "interface Shape { value: string; run(): void; }";
        for (mode, expected) in [
            (
                SemicolonMode::Always,
                "interface Shape {\n  value: string;\n  run(): void;\n}",
            ),
            (
                SemicolonMode::AsNeeded,
                "interface Shape {\n  value: string\n  run(): void\n}",
            ),
            (
                SemicolonMode::Off,
                "interface Shape {\n  value: string;\n  run(): void;\n}",
            ),
        ] {
            assert_eq!(
                format_interface_layout(
                    "sample.ts",
                    source,
                    InterfaceLayoutRule::Threshold(0),
                    mode,
                ),
                expected
            );
        }
    }

    #[test]
    fn preserves_interface_comments_bom_crlf_and_eof_shape_idempotently() {
        let source = "\u{feff}interface Shape { /** value */ value: [\r\n    string,\r\n  ]; // run\r\nrun(): void; }";
        let raw_config = FormatConfig {
            rules: RulesConfig {
                import_layout: false,
                interface_layout: InterfaceLayoutRule::Threshold(0),
                object_property_spacing: false,
                statement_spacing: StatementSpacingConfig {
                    control_flow_statements: StatementSpacingMode::Off,
                    imports: StatementSpacingMode::Off,
                    multiline_call_statements: StatementSpacingMode::Off,
                    return_statements: StatementSpacingMode::Off,
                    type_aliases: StatementSpacingMode::Off,
                    variable_declarations: StatementSpacingMode::Off,
                },
                semicolons: semicolons_off(),
                trailing_commas: TrailingCommaMode::Never,
            },
            ..FormatConfig::default()
        };
        let config = resolve_config(raw_config).unwrap();
        let output = format_text(Path::new("sample.ts"), source, &config)
            .unwrap()
            .unwrap();
        assert_eq!(
            output,
            "\u{feff}interface Shape {\r\n  /** value */ value: [\r\n    string\r\n  ]; // run\r\n  run(): void;\r\n}"
        );
        assert!(
            format_text(Path::new("sample.ts"), &output, &config)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn does_not_apply_interface_layout_to_object_type_aliases() {
        let source = "type Shape = { value: string; run(): void; };";
        assert_eq!(
            format_interface_layout(
                "sample.ts",
                source,
                InterfaceLayoutRule::Threshold(0),
                SemicolonMode::Off,
            ),
            source
        );
    }

    #[test]
    fn formats_static_import_families_without_reordering() {
        let source = "import{z as local,type A,b}from\"pkg\";\nimport type{Foo,Bar as Baz}from'x'\nimport value,*as space from\"ns\";\nimport\"side\";";
        let expected = "import { z as local, type A, b } from \"pkg\";\nimport type { Foo, Bar as Baz } from 'x'\nimport value, * as space from \"ns\";\nimport \"side\";";
        assert_eq!(format(source), expected);
    }

    #[test]
    fn formats_statement_semicolons_in_all_modes_with_asi_guards() {
        let as_needed = SemicolonConfig {
            statements: SemicolonMode::AsNeeded,
            class_members: SemicolonMode::Off,
            type_members: SemicolonMode::Off.into(),
        };
        let source = "const value=1;\n[one,two].forEach(work);\n(foo)();\n+value;\n-value;\n/regex/.test(text);\n`template`;";
        let expected = "const value=1\n;[one,two].forEach(work)\n;(foo)()\n;+value\n;-value\n;/regex/.test(text)\n;`template`";
        assert_eq!(format_semicolons("sample.js", source, as_needed), expected);
        assert_eq!(
            format_semicolons("sample.js", expected, as_needed),
            expected
        );

        assert_eq!(
            format_semicolons("sample.js", "const first=1;const second=2;", as_needed),
            "const first=1;const second=2"
        );
        assert_eq!(
            format_semicolons(
                "sample.js",
                "const value=1\nwork()\ndebugger",
                SemicolonConfig {
                    statements: SemicolonMode::Always,
                    class_members: SemicolonMode::Off,
                    type_members: SemicolonMode::Off.into(),
                },
            ),
            "const value=1;\nwork();\ndebugger;"
        );

        let preserved = "const value=1;\n[one,two];";
        assert_eq!(
            format_semicolons("sample.js", preserved, semicolons_off()),
            preserved
        );
    }

    #[test]
    fn preserves_nested_guard_indentation_across_statement_spacing_passes() {
        let source = "function update() {\n        const node = value()\n        ;   (node as Mutable<Node>).value = 1;\n        return node;\n}";
        let output = format_file_with("sample.ts", source, FormatConfig::default());

        assert!(output.contains("\n        ;(node as Mutable<Node>).value = 1\n"));
        assert_eq!(
            format_file_with("sample.ts", &output, FormatConfig::default()),
            output
        );
    }

    #[test]
    fn preserves_wrapper_boundaries_and_only_guards_eligible_direct_siblings() {
        let as_needed = SemicolonConfig {
            statements: SemicolonMode::AsNeeded,
            class_members: SemicolonMode::AsNeeded,
            type_members: SemicolonMode::Off.into(),
        };
        let cases = [
            (
                "sample.js",
                "if (condition) work();\n[one].map(use);",
                "if (condition) work();\n[one].map(use)",
            ),
            (
                "sample.js",
                "if (condition) { work(); }\n[one].map(use);",
                "if (condition) { work() }\n[one].map(use)",
            ),
            (
                "sample.js",
                "class C {\n  method() {}\n  [key] = 1;\n}",
                "class C {\n  method() {}\n  [key] = 1\n}",
            ),
            (
                "sample.js",
                "class C {\n  field = value;\n  *gen() {}\n}",
                "class C {\n  field = value\n  ;*gen() {}\n}",
            ),
        ];

        for (file_name, source, expected) in cases {
            let output = format_semicolons(file_name, source, as_needed);
            assert_eq!(output, expected);
            assert_eq!(format_semicolons(file_name, &output, as_needed), output);
        }
    }

    #[test]
    fn configures_statement_class_and_type_member_semicolons_independently() {
        let source = "const runtime=1;\nabstract class Example {\n  field=1;\n  [key]=2;\n  accessor item=3;\n  abstract method(): void;\n}\ninterface Shape {\n  value: string;\n  method(): void,\n  other: number\n}\ntype Copy<T> = {\n  [K in keyof T]: T[K];\n};";
        let output = format_semicolons(
            "sample.ts",
            source,
            SemicolonConfig {
                statements: SemicolonMode::Off,
                class_members: SemicolonMode::AsNeeded,
                type_members: SemicolonMode::Always.into(),
            },
        );
        assert_eq!(
            output,
            "const runtime=1;\nabstract class Example {\n  field=1\n  ;[key]=2\n  accessor item=3\n  abstract method(): void\n}\ninterface Shape {\n  value: string;\n  method(): void,\n  other: number;\n}\ntype Copy<T> = {\n  [K in keyof T]: T[K];\n};"
        );

        let removed = format_semicolons(
            "sample.ts",
            source,
            SemicolonConfig {
                statements: SemicolonMode::AsNeeded,
                class_members: SemicolonMode::Off,
                type_members: SemicolonMode::AsNeeded.into(),
            },
        );
        assert_eq!(
            removed,
            "const runtime=1\nabstract class Example {\n  field=1;\n  [key]=2;\n  accessor item=3;\n  abstract method(): void;\n}\ninterface Shape {\n  value: string\n  method(): void,\n  other: number\n}\ntype Copy<T> = {\n  [K in keyof T]: T[K]\n}"
        );

        let always_source = "abstract class Always {\n  field=1\n  concrete() {}\n  [key: string]: unknown\n  abstract method(): void\n}";
        let always = format_semicolons(
            "sample.ts",
            always_source,
            SemicolonConfig {
                statements: SemicolonMode::Off,
                class_members: SemicolonMode::Always,
                type_members: SemicolonMode::Off.into(),
            },
        );
        assert_eq!(
            always,
            "abstract class Always {\n  field=1;\n  concrete() {}\n  [key: string]: unknown;\n  abstract method(): void;\n}"
        );
        assert_eq!(
            format_semicolons(
                "sample.ts",
                &always,
                SemicolonConfig {
                    statements: SemicolonMode::Off,
                    class_members: SemicolonMode::Always,
                    type_members: SemicolonMode::Off.into(),
                },
            ),
            always
        );
    }

    #[test]
    fn configures_type_member_semicolons_by_final_container_layout() {
        let semicolons = SemicolonConfig {
            statements: SemicolonMode::Off,
            class_members: SemicolonMode::Off,
            type_members: TypeMemberSemicolonRule::default(),
        };
        let source = "function test(): { a: number; b: string; } { throw new Error(); }\ninterface Inline { first: number; second(): void; }\ntype Nested = {\n  inline: { value: string; run(): void; };\n  block: {\n    item: number\n  }\n};\ntype Copy<T> = { [K in keyof T]: T[K]; };\ntype Expanded<T> = {\n  [K in keyof T]: T[K]\n};";
        let expected = "function test(): { a: number; b: string } { throw new Error(); }\ninterface Inline { first: number; second(): void }\ntype Nested = {\n  inline: { value: string; run(): void };\n  block: {\n    item: number;\n  };\n};\ntype Copy<T> = { [K in keyof T]: T[K] };\ntype Expanded<T> = {\n  [K in keyof T]: T[K];\n};";

        LINE_BREAK_INDEX_BUILDS.set(0);
        let output = format_semicolons("sample.ts", source, semicolons);
        assert_eq!(output, expected);
        assert_eq!(format_semicolons("sample.ts", &output, semicolons), output);
        assert_eq!(LINE_BREAK_INDEX_BUILDS.get(), 2);

        let inverted = SemicolonConfig {
            statements: SemicolonMode::Off,
            class_members: SemicolonMode::Off,
            type_members: TypeMemberSemicolonRule::Layout(TypeMemberSemicolonConfig {
                single_line: SemicolonMode::Always,
                multiline: SemicolonMode::AsNeeded,
            }),
        };
        assert_eq!(
            format_semicolons(
                "sample.ts",
                "type Inline = { value: string };\ntype Block = {\n  value: string;\n};",
                inverted,
            ),
            "type Inline = { value: string; };\ntype Block = {\n  value: string\n};"
        );

        LINE_BREAK_INDEX_BUILDS.set(0);
        let javascript = "const value=1;";
        assert_eq!(
            format_semicolons("sample.js", javascript, semicolons),
            javascript
        );
        assert_eq!(LINE_BREAK_INDEX_BUILDS.get(), 0);

        assert_eq!(
            format_semicolons("sample.ts", javascript, semicolons),
            javascript
        );
        assert_eq!(LINE_BREAK_INDEX_BUILDS.get(), 0);
    }

    #[test]
    fn skips_type_member_line_index_for_equal_modes() {
        let semicolons = SemicolonConfig {
            statements: SemicolonMode::Off,
            class_members: SemicolonMode::Off,
            type_members: SemicolonMode::Always.into(),
        };
        let source = "interface Shape {\n  value: string\n}";

        LINE_BREAK_INDEX_BUILDS.set(0);
        assert_eq!(
            format_semicolons("sample.ts", source, semicolons),
            "interface Shape {\n  value: string;\n}"
        );
        assert_eq!(LINE_BREAK_INDEX_BUILDS.get(), 0);
    }

    #[test]
    fn applies_type_member_semicolons_after_interface_layout() {
        let source = "interface Shape { first: number; second(): void; }";
        let output = format_file_with("sample.ts", source, FormatConfig::default());
        assert_eq!(
            output,
            "interface Shape {\n  first: number;\n  second(): void;\n}"
        );
        assert_eq!(
            format_file_with("sample.ts", &output, FormatConfig::default()),
            output
        );
    }

    #[test]
    fn preserves_type_member_comments_and_line_endings_by_layout() {
        let semicolons = SemicolonConfig {
            statements: SemicolonMode::Off,
            class_members: SemicolonMode::Off,
            type_members: TypeMemberSemicolonRule::default(),
        };
        for newline in ["\n", "\r\n"] {
            let source = format!(
                "type Inline = {{ value: string; /* note */ }};{newline}type Block = {{{newline}  value: string // note{newline}}};"
            );
            let expected = format!(
                "type Inline = {{ value: string /* note */ }};{newline}type Block = {{{newline}  value: string; // note{newline}}};"
            );
            let output = format_semicolons("sample.ts", &source, semicolons);
            assert_eq!(output, expected);
            assert_eq!(format_semicolons("sample.ts", &output, semicolons), output);
        }
    }

    #[test]
    fn defaults_to_type_member_semicolons_only() {
        let source = "const runtime=1;\nclass Example {\n  field=1;\n}\ninterface Shape {\n  value: string\n}\ntype Copy = {\n  value: string\n};";
        let output = format_file_with("sample.ts", source, FormatConfig::default());
        assert_eq!(
            output,
            "const runtime=1\n\nclass Example {\n  field=1\n}\ninterface Shape {\n  value: string;\n}\n\ntype Copy = {\n  value: string;\n}"
        );
        assert_eq!(
            format_file_with("sample.ts", &output, FormatConfig::default()),
            output
        );
    }

    #[test]
    fn formats_static_export_semicolons_in_both_active_modes() {
        let without_semicolons = "export { value }\nexport { item } from 'pkg'\nexport * from 'other'\nexport default create()";
        let with_semicolons = "export { value };\nexport { item } from 'pkg';\nexport * from 'other';\nexport default create();";
        let as_needed = SemicolonConfig {
            statements: SemicolonMode::AsNeeded,
            class_members: SemicolonMode::Off,
            type_members: SemicolonMode::Off.into(),
        };
        let always = SemicolonConfig {
            statements: SemicolonMode::Always,
            class_members: SemicolonMode::Off,
            type_members: SemicolonMode::Off.into(),
        };

        assert_eq!(
            format_semicolons("sample.js", with_semicolons, as_needed),
            without_semicolons
        );
        assert_eq!(
            format_semicolons("sample.js", without_semicolons, always),
            with_semicolons
        );
        assert_eq!(
            format_semicolons("sample.js", with_semicolons, always),
            with_semicolons
        );
    }

    #[test]
    fn detached_semicolons_do_not_overlap_layout_edits_or_change_spacing_shape() {
        let import_with_comment = "import{x}from'x'\n; // between\nconst y=1;";
        let expected_import = "import { x } from 'x' // between\n\nconst y=1";
        let output = format_file_with("sample.ts", import_with_comment, FormatConfig::default());
        assert_eq!(output, expected_import);
        assert_eq!(
            format_file_with("sample.ts", &output, FormatConfig::default()),
            output
        );

        let variables = "const first=1\n;\nconst second=2";
        let expected_variables = "const first=1\nconst second=2";
        let output = format_file_with("sample.ts", variables, FormatConfig::default());
        assert_eq!(output, expected_variables);
        assert_eq!(
            format_file_with("sample.ts", &output, FormatConfig::default()),
            output
        );
    }

    #[test]
    fn semicolon_rewrite_preserves_comments_bom_crlf_and_eof_shape() {
        let source = "\u{feff}const value=1; // trailing\r\n// leading\r\n[one,two];";
        let expected = "\u{feff}const value=1 // trailing\r\n// leading\r\n;[one,two]";
        let semicolons = SemicolonConfig {
            statements: SemicolonMode::AsNeeded,
            class_members: SemicolonMode::AsNeeded,
            type_members: SemicolonMode::AsNeeded.into(),
        };
        assert_eq!(format_semicolons("sample.ts", source, semicolons), expected);
        assert_eq!(
            format_semicolons("sample.ts", expected, semicolons),
            expected
        );
    }

    #[test]
    fn default_semicolons_run_after_layout_spacing_and_trailing_commas() {
        let source = "import{one,two,}from'long-package';const value={\n  item: true,\n};";
        let output = format_file_with("sample.ts", source, FormatConfig::default());
        assert_eq!(
            output,
            "import { one, two } from 'long-package'\n\nconst value={\n  item: true\n}"
        );
        assert_eq!(
            format_file_with("sample.ts", &output, FormatConfig::default()),
            output
        );

        let jsx_source = "const rendered=<View />;\n<View />;";
        let jsx_output = format_file_with("sample.tsx", jsx_source, FormatConfig::default());
        assert_eq!(jsx_output, "const rendered=<View />\n\n;<View />");
        assert_eq!(
            format_file_with("sample.tsx", &jsx_output, FormatConfig::default()),
            jsx_output
        );
    }

    #[test]
    fn applies_never_and_always_to_nested_objects_and_arrays() {
        let without_commas = "const localKeyResolver = createLocalJWKSet({\n  keys: [\n    {\n      ...publicJwk,\n      alg: 'RS256',\n      kid: KEY_ID,\n      use: 'sig'\n    }\n  ]\n});";
        let with_commas = "const localKeyResolver = createLocalJWKSet({\n  keys: [\n    {\n      ...publicJwk,\n      alg: 'RS256',\n      kid: KEY_ID,\n      use: 'sig',\n    },\n  ],\n});";

        assert_eq!(
            format_trailing(without_commas, TrailingCommaMode::Always),
            with_commas
        );
        assert_eq!(
            format_trailing(with_commas, TrailingCommaMode::Never),
            without_commas
        );
    }

    #[test]
    fn places_commas_after_parenthesized_list_items() {
        for (source, expected) in [
            (
                "const values = [(\n  value\n)];",
                "const values = [(\n  value\n),];",
            ),
            (
                "type Values = [(\n  Value\n)];",
                "type Values = [(\n  Value\n),];",
            ),
            (
                "call(\n  (\n    value\n  )\n);",
                "call(\n  (\n    value\n  ),\n);",
            ),
            (
                "new Box(\n  (\n    value\n  )\n);",
                "new Box(\n  (\n    value\n  ),\n);",
            ),
        ] {
            let output = format_trailing(source, TrailingCommaMode::Always);
            assert_eq!(output, expected);
            assert_eq!(format_trailing(&output, TrailingCommaMode::Always), output);
        }
    }

    #[test]
    fn keeps_an_attached_single_multiline_argument_without_a_comma() {
        for source in ["call((\n  value\n));", "new Box((\n  value\n));"] {
            assert_eq!(format_trailing(source, TrailingCommaMode::Always), source);
        }
    }

    #[test]
    fn formats_modern_javascript_trailing_comma_families() {
        let source = "import {\n  imported\n} from 'pkg';\nexport {\n  imported\n};\nconst array = [\n  imported\n];\nconst object = {\n  imported\n};\nconst [\n  arrayValue\n] = array;\nconst {\n  objectValue\n} = object;\n([\n  assignedArray\n] = array);\n({\n  assignedObject\n} = object);\nfunction declared(\n  value\n) {}\nconst arrow = (\n  value\n) => value;\nconst called = declared(\n  array\n);\nconst created = new Box(\n  object\n);";
        let output = format_trailing(source, TrailingCommaMode::Always);

        for expected in [
            "  imported,\n} from 'pkg'",
            "export {\n  imported,\n}",
            "const array = [\n  imported,\n]",
            "const object = {\n  imported,\n}",
            "  arrayValue,\n] = array",
            "  objectValue,\n} = object",
            "  assignedArray,\n] = array",
            "  assignedObject,\n} = object",
            "function declared(\n  value,\n)",
            "const arrow = (\n  value,\n)",
            "declared(\n  array,\n)",
            "new Box(\n  object,\n)",
        ] {
            assert!(
                output.contains(expected),
                "missing {expected:?} in {output:?}"
            );
        }
        assert_eq!(format_trailing(&output, TrailingCommaMode::Always), output);
    }

    #[test]
    fn formats_typescript_trailing_comma_families() {
        let source = "type Generic<\n  Value\n> = [\n  Value\n];\ntype Signature = (\n  value: Generic<string>\n) => void;\nenum Choice {\n  First\n}\nclass Box<Value> {\n  constructor(\n    value: Value\n  ) {}\n  method(\n    value: Value\n  ): Value { return value; }\n}";
        let output = format_trailing(source, TrailingCommaMode::Always);

        for expected in [
            "type Generic<\n  Value,\n>",
            "= [\n  Value,\n]",
            "type Signature = (\n  value: Generic<string>,\n)",
            "enum Choice {\n  First,\n}",
            "constructor(\n    value: Value,\n  )",
            "method(\n    value: Value,\n  )",
        ] {
            assert!(
                output.contains(expected),
                "missing {expected:?} in {output:?}"
            );
        }
    }

    #[test]
    fn formats_a_typescript_this_parameter() {
        let multiline = "function handle(\n  this: Context\n) {}";
        let with_comma = "function handle(\n  this: Context,\n) {}";
        assert_eq!(
            format_trailing(multiline, TrailingCommaMode::Always),
            with_comma
        );
        assert_eq!(
            format_trailing(with_comma, TrailingCommaMode::Never),
            multiline
        );
        assert_eq!(
            format_trailing(
                "function handle(this: Context,) {}",
                TrailingCommaMode::Never
            ),
            "function handle(this: Context) {}"
        );
    }

    #[test]
    fn formats_import_and_export_attributes() {
        let source = "import data from 'data.json' with {\n  type: 'json'\n};\nexport { data } from 'data.json' with {\n  type: 'json'\n};";
        let expected = "import data from 'data.json' with {\n  type: 'json',\n};\nexport { data } from 'data.json' with {\n  type: 'json',\n};";
        assert_eq!(format_trailing(source, TrailingCommaMode::Always), expected);
        assert_eq!(format_trailing(expected, TrailingCommaMode::Never), source);
    }

    #[test]
    fn formats_import_commas_in_every_mode_idempotently() {
        let source = "import{value,}from'data.json'with{type:'json',};";
        let without_commas = "import { value } from 'data.json' with { type: 'json' };";
        let with_commas = "import { value, } from 'data.json' with { type: 'json', };";

        for (mode, expected) in [
            (TrailingCommaMode::Always, without_commas),
            (TrailingCommaMode::Never, without_commas),
            (TrailingCommaMode::Off, with_commas),
        ] {
            let config = FormatConfig {
                rules: RulesConfig {
                    trailing_commas: mode,
                    ..RulesConfig::default()
                },
                ..FormatConfig::default()
            };
            let output = format_with_semicolons_off(source, config.clone());
            assert_eq!(output, expected);
            assert_eq!(format_with_semicolons_off(&output, config), output);
        }
    }

    #[test]
    fn keeps_excluded_or_semantic_commas_untouched() {
        let source = "const sparse = [\n  first,\n  ,\n];\nconst [\n  head,\n  ,\n] = sparse;\nfunction rest(\n  ...values\n) {}\nconst loaded = import(\n  'pkg'\n);\ntype Instantiated = Generic<\n  string,\n>;\ntype RestTuple = [\n  string,\n  ...number[]\n];";

        for mode in [TrailingCommaMode::Always, TrailingCommaMode::Never] {
            let output = format_trailing(source, mode);
            assert!(output.contains("  ,\n];"));
            assert!(output.contains("  ...values\n)"));
            assert!(output.contains("import(\n  'pkg'\n)"));
            assert!(output.contains("Generic<\n  string,\n>"));
            assert!(output.contains("  ...number[]\n]"));
        }
    }

    #[test]
    fn never_adds_a_comma_after_destructuring_rest() {
        let source = "const [\n  head,\n  ...tail\n] = values;\nconst {\n  key,\n  ...others\n} = value;\n([\n  assigned,\n  ...remaining\n] = values);\n({\n  key: assignedKey,\n  ...assignedRest\n} = value);";
        for mode in [TrailingCommaMode::Always, TrailingCommaMode::Never] {
            assert_eq!(format_trailing(source, mode), source);
        }
    }

    #[test]
    fn preserves_required_single_generic_arrow_commas() {
        let source = "const identity = <T,>(value: T) => value;";
        for file_name in ["sample.tsx", "sample.mts", "sample.cts"] {
            for mode in [
                TrailingCommaMode::Always,
                TrailingCommaMode::Never,
                TrailingCommaMode::Off,
            ] {
                assert_eq!(format_trailing_file(file_name, source, mode), source);
            }
        }

        let without_comma = "const identity = <T>(value: T) => value;";
        for mode in [TrailingCommaMode::Always, TrailingCommaMode::Never] {
            assert_eq!(
                format_trailing_file("sample.ts", source, mode),
                without_comma
            );
            assert_eq!(
                format_trailing_file("sample.ts", without_comma, mode),
                without_comma
            );
        }
        assert_eq!(
            format_trailing_file("sample.ts", source, TrailingCommaMode::Off),
            source
        );
        assert_eq!(
            format_trailing_file("sample.ts", without_comma, TrailingCommaMode::Off),
            without_comma
        );
    }

    #[test]
    fn treats_unambiguous_single_generic_arrows_as_optional() {
        let constrained = "const identity = <T extends unknown>(value: T) => value;";
        let constrained_with_comma = "const identity = <T extends unknown,>(value: T) => value;";
        for file_name in ["sample.tsx", "sample.mts", "sample.cts"] {
            for mode in [
                TrailingCommaMode::Always,
                TrailingCommaMode::Never,
                TrailingCommaMode::Off,
            ] {
                assert_eq!(
                    format_trailing_file(file_name, constrained, mode),
                    constrained
                );
            }
            for mode in [TrailingCommaMode::Always, TrailingCommaMode::Never] {
                assert_eq!(
                    format_trailing_file(file_name, constrained_with_comma, mode),
                    constrained
                );
            }
            assert_eq!(
                format_trailing_file(file_name, constrained_with_comma, TrailingCommaMode::Off),
                constrained_with_comma
            );
        }

        let defaulted = "const identity = <T = unknown>(value: T) => value;";
        let defaulted_with_comma = "const identity = <T = unknown,>(value: T) => value;";
        for mode in [
            TrailingCommaMode::Always,
            TrailingCommaMode::Never,
            TrailingCommaMode::Off,
        ] {
            assert_eq!(
                format_trailing_file("sample.tsx", defaulted, mode),
                defaulted
            );
        }
        for mode in [TrailingCommaMode::Always, TrailingCommaMode::Never] {
            assert_eq!(
                format_trailing_file("sample.tsx", defaulted_with_comma, mode),
                defaulted
            );
        }
        assert_eq!(
            format_trailing_file("sample.tsx", defaulted_with_comma, TrailingCommaMode::Off),
            defaulted_with_comma
        );

        let module_defaulted = "const identity = <T = unknown,>(value: T) => value;";
        for file_name in ["sample.mts", "sample.cts"] {
            for mode in [
                TrailingCommaMode::Always,
                TrailingCommaMode::Never,
                TrailingCommaMode::Off,
            ] {
                assert_eq!(
                    format_trailing_file(file_name, module_defaulted, mode),
                    module_defaulted
                );
            }
        }

        let multiline = "const identity = <\n  T extends unknown\n>(value: T) => value;";
        let with_comma = "const identity = <\n  T extends unknown,\n>(value: T) => value;";
        assert_eq!(
            format_trailing_file("sample.tsx", multiline, TrailingCommaMode::Always),
            with_comma
        );
        assert_eq!(
            format_trailing_file("sample.tsx", with_comma, TrailingCommaMode::Never),
            multiline
        );
    }

    #[test]
    fn inserts_before_trailing_comments_and_preserves_file_shape() {
        let source = "\u{feff}const value = {\r\n  key: true // keep\r\n}";
        let output = format_trailing(source, TrailingCommaMode::Always);
        assert_eq!(
            output,
            "\u{feff}const value = {\r\n  key: true, // keep\r\n}"
        );
        assert!(!output.ends_with('\n'));
    }

    #[test]
    fn always_removes_optional_single_line_commas() {
        let source = "const array = [value,]; const object = { value, }; call(value,);";
        assert_eq!(
            format_trailing(source, TrailingCommaMode::Always),
            "const array = [value]; const object = { value }; call(value);"
        );
    }

    #[test]
    fn off_preserves_import_commas_through_layout() {
        let source = "import{one,two,}from'a-very-long-package-name';";
        let output = format_with_semicolons_off(
            source,
            FormatConfig {
                line_width: 20,
                rules: RulesConfig {
                    trailing_commas: TrailingCommaMode::Off,
                    ..RulesConfig::default()
                },
                ..FormatConfig::default()
            },
        );
        assert_eq!(
            output,
            "import {\n  one,\n  two,\n} from 'a-very-long-package-name';"
        );

        let config = resolve_config(FormatConfig {
            rules: RulesConfig {
                import_layout: false,
                interface_layout: InterfaceLayoutRule::Mode(InterfaceLayoutMode::Off),
                object_property_spacing: false,
                statement_spacing: StatementSpacingConfig {
                    control_flow_statements: StatementSpacingMode::Off,
                    imports: StatementSpacingMode::Off,
                    multiline_call_statements: StatementSpacingMode::Off,
                    return_statements: StatementSpacingMode::Off,
                    type_aliases: StatementSpacingMode::Off,
                    variable_declarations: StatementSpacingMode::Off,
                },
                semicolons: SemicolonConfig {
                    statements: SemicolonMode::Off,
                    class_members: SemicolonMode::Off,
                    type_members: SemicolonMode::Off.into(),
                },
                trailing_commas: TrailingCommaMode::Off,
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
    fn trailing_commas_use_the_final_import_layout() {
        let expanded = format_with_semicolons_off(
            "import{one,two}from'a-very-long-package-name';",
            FormatConfig {
                line_width: 20,
                rules: RulesConfig {
                    trailing_commas: TrailingCommaMode::Always,
                    ..RulesConfig::default()
                },
                ..FormatConfig::default()
            },
        );
        assert_eq!(
            expanded,
            "import {\n  one,\n  two,\n} from 'a-very-long-package-name';"
        );

        let flattened = format_with_semicolons_off(
            "import {\n  one,\n  two,\n} from 'pkg';",
            FormatConfig {
                rules: RulesConfig {
                    trailing_commas: TrailingCommaMode::Always,
                    ..RulesConfig::default()
                },
                ..FormatConfig::default()
            },
        );
        assert_eq!(flattened, "import { one, two } from 'pkg';");
    }

    #[test]
    fn handles_nested_type_parameter_closers() {
        let source = "function convert<\n  Value extends Generic<string>\n>(value: Value) {}";
        assert_eq!(
            format_trailing(source, TrailingCommaMode::Always),
            "function convert<\n  Value extends Generic<string>,\n>(value: Value) {}"
        );
    }

    #[test]
    fn formats_default_and_named_imports() {
        let source = "import React,{useState,type ComponentType as Type}from'react';";
        let flat = "import React, { useState, type ComponentType as Type } from 'react';";
        assert_eq!(format(source), flat);

        let multiline = format_with_semicolons_off(
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
            format_with_semicolons_off(
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
        assert_eq!(format_with_semicolons_off(source, no_verify), expected);
    }

    #[test]
    fn breaks_named_imports_one_specifier_per_line() {
        let source = "import { one } from 'a-very-long-package-name'";
        let output = format_with_semicolons_off(
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
            format_with_semicolons_off(
                source,
                FormatConfig {
                    line_width: flat_width,
                    ..FormatConfig::default()
                },
            ),
            flat
        );
        assert_eq!(
            format_with_semicolons_off(
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
    fn import_width_uses_the_final_never_comma_shape() {
        let source = "import{a,b,}from'x'";
        let expected = "import { a, b } from 'x'";
        let config = resolve_config(FormatConfig {
            line_width: u32::try_from(expected.chars().count()).unwrap(),
            rules: RulesConfig {
                import_layout: true,
                interface_layout: InterfaceLayoutRule::Mode(InterfaceLayoutMode::Off),
                object_property_spacing: false,
                statement_spacing: StatementSpacingConfig {
                    control_flow_statements: StatementSpacingMode::Off,
                    imports: StatementSpacingMode::Off,
                    multiline_call_statements: StatementSpacingMode::Off,
                    return_statements: StatementSpacingMode::Off,
                    type_aliases: StatementSpacingMode::Off,
                    variable_declarations: StatementSpacingMode::Off,
                },
                semicolons: SemicolonConfig::default(),
                trailing_commas: TrailingCommaMode::Never,
            },
            ..FormatConfig::default()
        })
        .unwrap();

        let output = format_text(Path::new("sample.ts"), source, &config)
            .unwrap()
            .unwrap();
        assert_eq!(output, expected);
        assert!(
            format_text(Path::new("sample.ts"), &output, &config)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn import_width_uses_the_final_as_needed_semicolon_shape() {
        let import_shell = "import { one, two } from ''";
        let package = "x".repeat(120 - import_shell.len());
        let expected_import = format!("import {{ one, two }} from '{package}'");
        let source = format!("import{{one,two}}from'{package}';work();");
        let output = format_file_with(
            "sample.ts",
            &source,
            FormatConfig {
                line_width: 120,
                ..FormatConfig::default()
            },
        );
        let expected = format!("{expected_import}\n\nwork()");

        assert_eq!(output, expected);
        assert_eq!(
            format_file_with(
                "sample.ts",
                &output,
                FormatConfig {
                    line_width: 120,
                    ..FormatConfig::default()
                },
            ),
            output
        );
    }

    #[test]
    fn import_width_uses_the_final_always_semicolon_shape() {
        let import_shell = "import { one, two } from '';";
        let package = "x".repeat(120 - import_shell.len());
        let source = format!("import{{one,two}}from'{package}'");
        let config = || FormatConfig {
            line_width: 120,
            rules: RulesConfig {
                semicolons: SemicolonConfig {
                    statements: SemicolonMode::Always,
                    class_members: SemicolonMode::Off,
                    type_members: SemicolonMode::Off.into(),
                },
                ..RulesConfig::default()
            },
            ..FormatConfig::default()
        };

        let flat = format_file_with("sample.ts", &source, config());
        assert_eq!(flat, format!("import {{ one, two }} from '{package}';"));
        assert_eq!(format_file_with("sample.ts", &flat, config()), flat);

        let long_package = format!("{package}x");
        let long_source = format!("import{{one,two}}from'{long_package}'");
        let multiline = format_file_with("sample.ts", &long_source, config());
        assert_eq!(
            multiline,
            format!("import {{\n  one,\n  two\n}} from '{long_package}';")
        );
        assert_eq!(
            format_file_with("sample.ts", &multiline, config()),
            multiline
        );
    }

    #[test]
    fn applies_the_import_spacing_matrix_in_both_directions() {
        let source = "const before={raw:true};\n\n\nimport a from'a'\n\nimport{one,two}from'long-package'\nimport{three,four}from'other-long-package'\n\nimport b from'b'\n\nconst after=[1,2];";
        let output = format_with_semicolons_off(
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
        let without_layout = format_with_semicolons_off(
            source,
            FormatConfig {
                line_width: 20,
                rules: RulesConfig {
                    import_layout: false,
                    statement_spacing: StatementSpacingConfig {
                        control_flow_statements: StatementSpacingMode::Off,
                        imports: StatementSpacingMode::Separate,
                        multiline_call_statements: StatementSpacingMode::Off,
                        return_statements: StatementSpacingMode::Off,
                        type_aliases: StatementSpacingMode::Off,
                        variable_declarations: StatementSpacingMode::Off,
                    },
                    ..RulesConfig::default()
                },
                ..FormatConfig::default()
            },
        );
        assert_eq!(
            without_layout,
            "import{one,two}from'long-package';\nimport value from'x';"
        );

        let with_layout = format_with_semicolons_off(
            source,
            FormatConfig {
                line_width: 20,
                rules: RulesConfig {
                    import_layout: true,
                    statement_spacing: StatementSpacingConfig {
                        control_flow_statements: StatementSpacingMode::Off,
                        imports: StatementSpacingMode::Separate,
                        multiline_call_statements: StatementSpacingMode::Off,
                        return_statements: StatementSpacingMode::Off,
                        type_aliases: StatementSpacingMode::Off,
                        variable_declarations: StatementSpacingMode::Off,
                    },
                    ..RulesConfig::default()
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
        let mut config = FormatConfig::default();
        config.rules.object_property_spacing = false;
        let output = format_with_semicolons_off(source, config);
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
        let output = format_with_semicolons_off(
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
        let source = "import{a,b}from'x';interface Shape { value: string; }type Value={raw:true};const value={raw:true};function f(){work();return value;}";
        let config = resolve_config(FormatConfig {
            rules: RulesConfig {
                import_layout: false,
                interface_layout: InterfaceLayoutRule::Mode(InterfaceLayoutMode::Off),
                object_property_spacing: false,
                statement_spacing: StatementSpacingConfig {
                    control_flow_statements: StatementSpacingMode::Off,
                    imports: StatementSpacingMode::Off,
                    multiline_call_statements: StatementSpacingMode::Off,
                    return_statements: StatementSpacingMode::Off,
                    type_aliases: StatementSpacingMode::Off,
                    variable_declarations: StatementSpacingMode::Off,
                },
                semicolons: SemicolonConfig {
                    statements: SemicolonMode::Off,
                    class_members: SemicolonMode::Off,
                    type_members: SemicolonMode::Off.into(),
                },
                trailing_commas: TrailingCommaMode::Off,
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
        let config = resolve_config(FormatConfig {
            rules: RulesConfig {
                semicolons: semicolons_off(),
                ..RulesConfig::default()
            },
            ..FormatConfig::default()
        })
        .unwrap();
        assert!(
            format_text(Path::new("sample.ts"), &output, &config)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn never_reuses_the_initial_tokenized_parse() {
        let config = resolve_config(FormatConfig {
            verify_ast: false,
            rules: RulesConfig {
                semicolons: semicolons_off(),
                trailing_commas: TrailingCommaMode::Never,
                ..RulesConfig::default()
            },
            ..FormatConfig::default()
        })
        .unwrap();
        TOKEN_PREFLIGHT_PARSES.set(0);
        TOKEN_PARSER_RUNS.set(0);

        format_text(
            Path::new("sample.ts"),
            "import{one,two,}from'package';const value={\n  key: true,\n};",
            &config,
        )
        .unwrap();

        assert_eq!(TOKEN_PREFLIGHT_PARSES.get(), 1);
        assert_eq!(TOKEN_PARSER_RUNS.get(), 1);
    }

    #[test]
    fn always_reparses_a_rewritten_intermediate_without_a_second_preflight() {
        let config = resolve_config(FormatConfig {
            verify_ast: false,
            rules: RulesConfig {
                semicolons: semicolons_off(),
                trailing_commas: TrailingCommaMode::Always,
                ..RulesConfig::default()
            },
            ..FormatConfig::default()
        })
        .unwrap();
        TOKEN_PREFLIGHT_PARSES.set(0);
        TOKEN_PARSER_RUNS.set(0);

        format_text(
            Path::new("sample.ts"),
            "import{one,two}from'package';const value={\n  key: true\n};",
            &config,
        )
        .unwrap();

        assert_eq!(TOKEN_PREFLIGHT_PARSES.get(), 1);
        assert_eq!(TOKEN_PARSER_RUNS.get(), 2);
    }

    #[test]
    fn trailing_comma_indexes_stay_linear_for_nested_calls() {
        let depth = 64;
        let mut source = "call(\n".repeat(depth);
        source.push_str("value");
        source.push_str(&"\n)".repeat(depth));
        source.push(';');

        LINE_BREAK_INDEX_BUILDS.set(0);
        LINE_BREAK_QUERIES.set(0);
        PARENTHESIS_INDEX_BUILDS.set(0);
        PARENTHESIS_LOOKUPS.set(0);
        format_trailing(&source, TrailingCommaMode::Always);

        assert_eq!(LINE_BREAK_INDEX_BUILDS.get(), 1);
        assert!(LINE_BREAK_QUERIES.get() <= depth * 3);
        assert_eq!(PARENTHESIS_INDEX_BUILDS.get(), 1);
        assert_eq!(PARENTHESIS_LOOKUPS.get(), depth);
    }

    #[test]
    fn verification_rejects_a_different_program() {
        let file_name = Path::new("sample.ts");
        let source_type = crate::document::script_source_type(file_name).unwrap();
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
            comparisons < import_count * 256,
            "span lookups performed {comparisons} comparisons for {import_count} imports"
        );
    }

    #[test]
    fn deferred_import_boundary_lookups_stay_linear() {
        let item_count = 512;
        let mut source = String::new();
        for index in 0..item_count {
            write!(source, "import{{value{index}}}from'package-{index}';").unwrap();
        }
        source.push_str("\nfunction work(){");
        for index in 0..item_count {
            write!(source, "const value{index}={index};").unwrap();
        }
        source.push('}');
        let config = resolve_config(FormatConfig {
            verify_ast: false,
            rules: RulesConfig {
                statement_spacing: StatementSpacingConfig {
                    control_flow_statements: StatementSpacingMode::Off,
                    imports: StatementSpacingMode::Off,
                    multiline_call_statements: StatementSpacingMode::Off,
                    return_statements: StatementSpacingMode::Off,
                    type_aliases: StatementSpacingMode::Off,
                    variable_declarations: StatementSpacingMode::Compact,
                },
                ..RulesConfig::default()
            },
            ..FormatConfig::default()
        })
        .unwrap();

        DEFERRED_IMPORT_BOUNDARY_LOOKUPS.set(0);
        format_text(Path::new("deferred-imports.ts"), &source, &config).unwrap();
        let lookups = DEFERRED_IMPORT_BOUNDARY_LOOKUPS.get();
        assert!(lookups > 0);
        assert!(
            lookups <= item_count,
            "deferred import boundary lookup ran {lookups} times for {item_count} imports"
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
    fn disabled_spacing_categories_skip_multiline_scans() {
        let imports_only = "import{value}from'pkg';const outer=()=>{const inner=1;work();};";
        IMPORT_MULTILINE_SCANS.set(0);
        TYPE_ALIAS_MULTILINE_SCANS.set(0);
        VARIABLE_MULTILINE_SCANS.set(0);
        format_with_rules(
            imports_only,
            true,
            StatementSpacingMode::Off,
            StatementSpacingMode::Off,
        );
        assert_eq!(IMPORT_MULTILINE_SCANS.get(), 0);
        assert_eq!(TYPE_ALIAS_MULTILINE_SCANS.get(), 0);
        assert_eq!(VARIABLE_MULTILINE_SCANS.get(), 0);

        let import_spacing_only = "import value from'pkg';const outer=()=>{const inner=1;work();};";
        IMPORT_MULTILINE_SCANS.set(0);
        TYPE_ALIAS_MULTILINE_SCANS.set(0);
        VARIABLE_MULTILINE_SCANS.set(0);
        format_with_rules(
            import_spacing_only,
            false,
            StatementSpacingMode::Separate,
            StatementSpacingMode::Off,
        );
        assert_eq!(IMPORT_MULTILINE_SCANS.get(), 1);
        assert_eq!(TYPE_ALIAS_MULTILINE_SCANS.get(), 0);
        assert_eq!(VARIABLE_MULTILINE_SCANS.get(), 0);

        let variable_spacing_only = "import value from'pkg';const value=1;work();";
        IMPORT_MULTILINE_SCANS.set(0);
        TYPE_ALIAS_MULTILINE_SCANS.set(0);
        VARIABLE_MULTILINE_SCANS.set(0);
        format_with_rules(
            variable_spacing_only,
            false,
            StatementSpacingMode::Off,
            StatementSpacingMode::Separate,
        );
        assert_eq!(IMPORT_MULTILINE_SCANS.get(), 0);
        assert_eq!(TYPE_ALIAS_MULTILINE_SCANS.get(), 0);
        assert_eq!(VARIABLE_MULTILINE_SCANS.get(), 1);

        let type_alias_spacing_only = "type Value=1;run();";
        IMPORT_MULTILINE_SCANS.set(0);
        TYPE_ALIAS_MULTILINE_SCANS.set(0);
        VARIABLE_MULTILINE_SCANS.set(0);
        format_with_statement_spacing(
            type_alias_spacing_only,
            false,
            StatementSpacingMode::Off,
            StatementSpacingMode::Separate,
            StatementSpacingMode::Off,
        );
        assert_eq!(IMPORT_MULTILINE_SCANS.get(), 0);
        assert_eq!(TYPE_ALIAS_MULTILINE_SCANS.get(), 1);
        assert_eq!(VARIABLE_MULTILINE_SCANS.get(), 0);
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
    fn preserves_a_detached_glossary_after_formatting_an_import() {
        let source = "import type {\n  FilterDefinition,\n  FilterOperator,\n  FilterRule\n} from '~/types/filter';\n\n/**\n * Glossary\n */\n\ntype FilterBuilderStep = 'field' | 'operator' | 'value';";
        let expected = "import type { FilterDefinition, FilterOperator, FilterRule } from '~/types/filter';\n\n/**\n * Glossary\n */\n\ntype FilterBuilderStep = 'field' | 'operator' | 'value';";
        let output = format(source);

        assert_eq!(output, expected);
        assert_eq!(format(&output), output);
    }

    #[test]
    fn preserves_detached_comment_gaps_in_every_spacing_mode() {
        for mode in [
            StatementSpacingMode::Separate,
            StatementSpacingMode::Compact,
        ] {
            for newline in ["\n", "\r\n"] {
                let boundary = if mode == StatementSpacingMode::Separate {
                    newline.repeat(2)
                } else {
                    newline.to_owned()
                };
                for comment in ["// section", "/* section */", "/** section */"] {
                    let detached =
                        format!("run();{newline}{comment}{newline}{newline}{newline}type Value=1;");
                    let detached_expected =
                        format!("run();{boundary}{comment}{newline}{newline}type Value=1;");
                    let detached_output = format_with_statement_spacing(
                        &detached,
                        false,
                        StatementSpacingMode::Off,
                        mode,
                        StatementSpacingMode::Off,
                    );

                    assert_eq!(detached_output, detached_expected, "{mode:?} {comment}");
                    assert_eq!(
                        format_with_statement_spacing(
                            &detached_output,
                            false,
                            StatementSpacingMode::Off,
                            mode,
                            StatementSpacingMode::Off,
                        ),
                        detached_output,
                        "{mode:?} {comment}"
                    );

                    let attached = format!("run();{newline}{comment}{newline}type Value=1;");
                    let attached_expected =
                        format!("run();{boundary}{comment}{newline}type Value=1;");
                    assert_eq!(
                        format_with_statement_spacing(
                            &attached,
                            false,
                            StatementSpacingMode::Off,
                            mode,
                            StatementSpacingMode::Off,
                        ),
                        attached_expected,
                        "{mode:?} {comment}"
                    );
                }
            }
        }
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
    fn applies_the_type_alias_spacing_matrix_to_single_and_multiline_aliases() {
        let source = "type A=1;type B=2;\ntype Generic<\n T\n> = T;\ntype Object = {\n value:string\n};\ntype C=3;\nrun();";
        let expected = "type A=1;\ntype B=2;\n\ntype Generic<\n T\n> = T;\n\ntype Object = {\n value:string\n};\n\ntype C=3;\n\nrun();";
        let output = format_with_statement_spacing(
            source,
            false,
            StatementSpacingMode::Off,
            StatementSpacingMode::Separate,
            StatementSpacingMode::Off,
        );
        assert_eq!(output, expected);
        assert_eq!(
            format_with_statement_spacing(
                &output,
                false,
                StatementSpacingMode::Off,
                StatementSpacingMode::Separate,
                StatementSpacingMode::Off,
            ),
            output
        );
    }

    #[test]
    fn applies_all_type_alias_spacing_modes_and_preserves_alias_contents() {
        let source = "type LongName<T> = {  value:T, nested:[1,  2] };\n\n\nrun();";
        let compact = format_with_statement_spacing(
            source,
            false,
            StatementSpacingMode::Off,
            StatementSpacingMode::Compact,
            StatementSpacingMode::Off,
        );
        assert_eq!(
            compact,
            "type LongName<T> = {  value:T, nested:[1,  2] };\nrun();"
        );

        let off = format_with_statement_spacing(
            source,
            false,
            StatementSpacingMode::Off,
            StatementSpacingMode::Off,
            StatementSpacingMode::Off,
        );
        assert_eq!(off, source);

        let narrow = format_with_semicolons_off(
            "type LongName<T> = {  value:T, nested:[1,  2] };type Next=1;",
            FormatConfig {
                line_width: 1,
                rules: RulesConfig {
                    import_layout: false,
                    interface_layout: InterfaceLayoutRule::Mode(InterfaceLayoutMode::Off),
                    object_property_spacing: false,
                    statement_spacing: StatementSpacingConfig {
                        control_flow_statements: StatementSpacingMode::Off,
                        imports: StatementSpacingMode::Off,
                        multiline_call_statements: StatementSpacingMode::Off,
                        return_statements: StatementSpacingMode::Off,
                        type_aliases: StatementSpacingMode::Separate,
                        variable_declarations: StatementSpacingMode::Off,
                    },
                    trailing_commas: TrailingCommaMode::Off,
                    ..RulesConfig::default()
                },
                ..FormatConfig::default()
            },
        );
        assert_eq!(
            narrow,
            "type LongName<T> = {  value:T, nested:[1,  2] };\ntype Next=1;"
        );
    }

    #[test]
    fn combines_both_sides_of_type_alias_boundaries_by_priority() {
        for source in ["type Value=1;const value=1;", "const value=1;type Value=1;"] {
            let blank_line = source.replacen(';', ";\n\n", 1);
            let one_line = source.replacen(';', ";\n", 1);

            assert_eq!(
                format_with_statement_spacing(
                    source,
                    false,
                    StatementSpacingMode::Off,
                    StatementSpacingMode::Separate,
                    StatementSpacingMode::Compact,
                ),
                blank_line
            );
            assert_eq!(
                format_with_statement_spacing(
                    source,
                    false,
                    StatementSpacingMode::Off,
                    StatementSpacingMode::Compact,
                    StatementSpacingMode::Off,
                ),
                one_line
            );
            assert_eq!(
                format_with_statement_spacing(
                    source,
                    false,
                    StatementSpacingMode::Off,
                    StatementSpacingMode::Off,
                    StatementSpacingMode::Compact,
                ),
                one_line
            );
            assert_eq!(
                format_with_statement_spacing(
                    source,
                    false,
                    StatementSpacingMode::Off,
                    StatementSpacingMode::Off,
                    StatementSpacingMode::Off,
                ),
                source
            );
        }
    }

    #[test]
    fn formats_type_aliases_in_definitions_blocks_and_namespaces() {
        let definition = "type A=1;type B=2;";
        for file_name in ["types.d.ts", "types.d.mts", "types.d.cts"] {
            assert_eq!(
                format_file_with(
                    file_name,
                    definition,
                    FormatConfig {
                        rules: RulesConfig {
                            import_layout: false,
                            interface_layout: InterfaceLayoutRule::Mode(InterfaceLayoutMode::Off),
                            object_property_spacing: false,
                            statement_spacing: StatementSpacingConfig {
                                control_flow_statements: StatementSpacingMode::Off,
                                imports: StatementSpacingMode::Off,
                                multiline_call_statements: StatementSpacingMode::Off,
                                return_statements: StatementSpacingMode::Off,
                                type_aliases: StatementSpacingMode::Separate,
                                variable_declarations: StatementSpacingMode::Off,
                            },
                            semicolons: semicolons_off(),
                            trailing_commas: TrailingCommaMode::Off,
                        },
                        ..FormatConfig::default()
                    },
                ),
                "type A=1;\ntype B=2;"
            );
        }

        assert_eq!(
            format_with_statement_spacing(
                "function f(){type A=1;work();}",
                false,
                StatementSpacingMode::Off,
                StatementSpacingMode::Separate,
                StatementSpacingMode::Off,
            ),
            "function f(){\n  type A=1;\n\n  work();\n}"
        );
        assert_eq!(
            format_with_statement_spacing(
                "namespace Live { type A=1;type B=2; }",
                false,
                StatementSpacingMode::Off,
                StatementSpacingMode::Separate,
                StatementSpacingMode::Off,
            ),
            "namespace Live {\n  type A=1;\n  type B=2;\n}"
        );
        assert_eq!(
            format_with_statement_spacing(
                "declare namespace Ambient { type A=1;type B=2; }",
                false,
                StatementSpacingMode::Off,
                StatementSpacingMode::Separate,
                StatementSpacingMode::Off,
            ),
            "declare namespace Ambient {\n  type A=1;\n  type B=2;\n}"
        );
    }

    #[test]
    fn excludes_exported_and_explicitly_declared_type_aliases() {
        let source = "export type A=1;export type { B };declare type C=2;";
        assert_eq!(
            format_with_statement_spacing(
                source,
                false,
                StatementSpacingMode::Off,
                StatementSpacingMode::Separate,
                StatementSpacingMode::Off,
            ),
            source
        );
    }

    #[test]
    fn type_alias_spacing_cascades_nested_layouts_and_preserves_boundary_shape() {
        let nested = "function outer(){if(ok){type A=1;work();}finish();}";
        let expected =
            "function outer(){\n  if(ok){\n    type A=1;\n\n    work();\n  }\n  finish();\n}";
        assert_eq!(
            format_with_statement_spacing(
                nested,
                false,
                StatementSpacingMode::Off,
                StatementSpacingMode::Separate,
                StatementSpacingMode::Off,
            ),
            expected
        );

        let compact = "function f(){type A=1;work();}";
        assert_eq!(
            format_with_statement_spacing(
                compact,
                false,
                StatementSpacingMode::Off,
                StatementSpacingMode::Compact,
                StatementSpacingMode::Off,
            ),
            "function f(){\n  type A=1;\n  work();\n}"
        );

        let source = "\u{feff}type A=1; // trailing\r\n\r\n// leading\r\nrun();";
        let output = format_with_statement_spacing(
            source,
            false,
            StatementSpacingMode::Off,
            StatementSpacingMode::Compact,
            StatementSpacingMode::Off,
        );
        assert_eq!(
            output,
            "\u{feff}type A=1; // trailing\r\n// leading\r\nrun();"
        );
        assert!(!output.replace("\r\n", "").contains('\n'));
        assert!(!output.ends_with('\n'));
    }

    #[test]
    fn preserves_detached_type_alias_semicolons_and_counts_the_complete_declaration_shape() {
        for (source, expected) in [
            ("type A=1\n;type B=2;", "type A=1\n;\n\ntype B=2;"),
            (
                "type A=1 // tail\n;type B=2;",
                "type A=1 // tail\n;\n\ntype B=2;",
            ),
        ] {
            let output = format_with_statement_spacing(
                source,
                false,
                StatementSpacingMode::Off,
                StatementSpacingMode::Separate,
                StatementSpacingMode::Off,
            );
            assert_eq!(output, expected);
            assert_eq!(
                format_with_statement_spacing(
                    &output,
                    false,
                    StatementSpacingMode::Off,
                    StatementSpacingMode::Separate,
                    StatementSpacingMode::Off,
                ),
                output
            );
        }
    }

    #[test]
    fn spaces_the_supplied_multiline_awaited_call_idempotently() {
        let source = "async function submit() {\n  prepare()\n  await requestFetch('/api', {\n    body: formData\n  })\n  isSubmitted.value = true\n}";
        let expected = "async function submit() {\n  prepare()\n\n  await requestFetch('/api', {\n    body: formData\n  })\n\n  isSubmitted.value = true\n}";
        let output =
            format_with_multiline_call_spacing("sample.ts", source, StatementSpacingMode::Separate);

        assert_eq!(output, expected);
        assert_eq!(
            format_with_multiline_call_spacing(
                "sample.ts",
                &output,
                StatementSpacingMode::Separate,
            ),
            output
        );
    }

    #[test]
    fn applies_multiline_call_modes_without_padding_statement_list_edges() {
        let source = "function f() {\n  before()\n\n\n  call(\n    value\n  )\n\n\n  after()\n}";
        assert_eq!(
            format_with_multiline_call_spacing("sample.ts", source, StatementSpacingMode::Separate,),
            "function f() {\n  before()\n\n  call(\n    value\n  )\n\n  after()\n}"
        );
        assert_eq!(
            format_with_multiline_call_spacing("sample.ts", source, StatementSpacingMode::Compact,),
            "function f() {\n  before()\n  call(\n    value\n  )\n  after()\n}"
        );
        assert_eq!(
            format_with_multiline_call_spacing("sample.ts", source, StatementSpacingMode::Off,),
            source
        );

        for unchanged in [
            "function f() {\n  call(\n    value\n  )\n}",
            "function f() {\n  call(\n    value\n  )\n  after()\n}",
            "function f() {\n  before()\n  call(\n    value\n  )\n}",
        ] {
            let output = format_with_multiline_call_spacing(
                "sample.ts",
                unchanged,
                StatementSpacingMode::Separate,
            );
            if unchanged.contains("after()") {
                assert_eq!(
                    output,
                    "function f() {\n  call(\n    value\n  )\n\n  after()\n}"
                );
            } else if unchanged.contains("before()") {
                assert_eq!(
                    output,
                    "function f() {\n  before()\n\n  call(\n    value\n  )\n}"
                );
            } else {
                assert_eq!(output, unchanged);
            }
        }
    }

    #[test]
    fn recognizes_direct_optional_awaited_and_typescript_wrapped_calls() {
        let source = "async function f() {\n  before()\n  call(\n    value\n  )\n  client.call?.(\n    value\n  )\n  client.call?.(\n    value\n  )!\n  await (client.call<Type>(\n    value\n  ) as Promise<void>)\n  after()\n}";
        let expected = "async function f() {\n  before()\n\n  call(\n    value\n  )\n\n  client.call?.(\n    value\n  )\n\n  client.call?.(\n    value\n  )!\n\n  await (client.call<Type>(\n    value\n  ) as Promise<void>)\n\n  after()\n}";
        assert_eq!(
            format_with_multiline_call_spacing("sample.ts", source, StatementSpacingMode::Separate,),
            expected
        );

        let super_call = "class Child extends Parent {\n  constructor() {\n    before()\n    super(\n      value\n    )\n    after()\n  }\n}";
        assert_eq!(
            format_with_multiline_call_spacing(
                "sample.ts",
                super_call,
                StatementSpacingMode::Separate,
            ),
            "class Child extends Parent {\n  constructor() {\n    before()\n\n    super(\n      value\n    )\n\n    after()\n  }\n}"
        );
    }

    #[test]
    fn excludes_non_call_and_non_direct_expression_statements() {
        let source = "async function f() {\n  before()\n  const value = call(\n    input\n  )\n  result = call(\n    value\n  )\n  void call(\n    value\n  )\n  new Example(\n    value\n  )\n  await import(\n    path\n  )\n  tag`\n    value\n  `\n  return call(\n    value\n  )\n}\nfunction* g() {\n  before()\n  yield call(\n    value\n  )\n  after()\n}";
        assert_eq!(
            format_with_multiline_call_spacing("sample.ts", source, StatementSpacingMode::Separate,),
            source
        );
    }

    #[test]
    fn combines_multiline_calls_with_other_spacing_by_priority() {
        let source = "function f() {\n  const value=1\n  call(\n    value\n  )\n  after()\n}";
        let format = |call_spacing, variable_spacing| {
            let mut config = object_spacing_config(false);
            config.rules.statement_spacing.multiline_call_statements = call_spacing;
            config.rules.statement_spacing.variable_declarations = variable_spacing;
            format_file_with("sample.ts", source, config)
        };

        assert_eq!(
            format(
                StatementSpacingMode::Separate,
                StatementSpacingMode::Compact,
            ),
            "function f() {\n  const value=1\n\n  call(\n    value\n  )\n\n  after()\n}"
        );
        assert_eq!(
            format(
                StatementSpacingMode::Compact,
                StatementSpacingMode::Separate,
            ),
            "function f() {\n  const value=1\n\n  call(\n    value\n  )\n  after()\n}"
        );
    }

    #[test]
    fn spaces_calls_from_their_final_cascaded_layout() {
        let source = "function f(){before();run({first:1,second:2});after();}";
        let mut config = object_spacing_config(true);
        config.rules.statement_spacing.multiline_call_statements = StatementSpacingMode::Separate;
        let output = format_file_with("sample.ts", source, config.clone());
        let expected = "function f(){\n  before();\n\n  run({\n    first:1,\n    second:2\n  });\n\n  after();\n}";

        assert_eq!(output, expected);
        assert_eq!(format_file_with("sample.ts", &output, config), output);
    }

    #[test]
    fn preserves_multiline_call_comments_bom_crlf_and_eof_shape() {
        let source = "\u{feff}function f(){\r\n  before(); // trailing\r\n  // leading\r\n  call(\r\n    value\r\n  )\r\n  after()\r\n}";
        let output =
            format_with_multiline_call_spacing("sample.ts", source, StatementSpacingMode::Separate);

        assert_eq!(
            output,
            "\u{feff}function f(){\r\n  before(); // trailing\r\n\r\n  // leading\r\n  call(\r\n    value\r\n  )\r\n\r\n  after()\r\n}"
        );
        assert_eq!(output.matches("// trailing").count(), 1);
        assert_eq!(output.matches("// leading").count(), 1);
        assert!(!output.replace("\r\n", "").contains('\n'));
        assert!(!output.ends_with('\n'));
    }

    #[test]
    fn preserves_multiline_call_typescript_directive_scope() {
        for directive in ["@ts-ignore", "@ts-expect-error"] {
            let source = format!(
                "function f() {{\n  before()\n  // {directive}\n  missing(\n    value\n  )\n  after()\n}}"
            );
            let expected = format!(
                "function f() {{\n  before()\n\n  // {directive}\n  missing(\n    value\n  )\n\n  after()\n}}"
            );
            assert_eq!(
                format_with_multiline_call_spacing(
                    "sample.ts",
                    &source,
                    StatementSpacingMode::Separate,
                ),
                expected,
                "{directive}"
            );
        }
    }

    #[test]
    fn multiline_call_line_break_queries_stay_indexed_and_skip_off_mode() {
        let depth = 128;
        let mut source = String::new();
        for _ in 0..depth {
            source.push_str("call(()=>{");
        }
        source.push_str("work(\nvalue\n);");
        for _ in 0..depth {
            source.push_str("});");
        }
        let mut config = object_spacing_config(false);
        config.verify_ast = false;
        config.rules.statement_spacing.multiline_call_statements = StatementSpacingMode::Separate;
        let config = resolve_config(config).unwrap();

        LINE_BREAK_INDEX_BUILDS.set(0);
        LINE_BREAK_QUERIES.set(0);
        format_text(Path::new("nested-calls.ts"), &source, &config).unwrap();
        assert_eq!(LINE_BREAK_INDEX_BUILDS.get(), 1);
        assert_eq!(LINE_BREAK_QUERIES.get(), depth + 1);

        let disabled = resolve_config(object_spacing_config(false)).unwrap();
        LINE_BREAK_INDEX_BUILDS.set(0);
        LINE_BREAK_QUERIES.set(0);
        assert!(
            format_text(Path::new("nested-calls.ts"), &source, &disabled)
                .unwrap()
                .is_none()
        );
        assert_eq!(LINE_BREAK_INDEX_BUILDS.get(), 0);
        assert_eq!(LINE_BREAK_QUERIES.get(), 0);
    }

    #[test]
    fn irrelevant_single_line_call_boundaries_skip_raw_line_scans() {
        let statement_count = 512;
        let source = "call();".repeat(statement_count);
        let mut config = object_spacing_config(false);
        config.verify_ast = false;
        config.rules.statement_spacing.multiline_call_statements = StatementSpacingMode::Separate;
        let config = resolve_config(config).unwrap();

        RAW_LINE_START_SCANS.set(0);
        assert!(
            format_text(Path::new("single-line-calls.ts"), &source, &config)
                .unwrap()
                .is_none()
        );
        assert_eq!(RAW_LINE_START_SCANS.get(), 0);
    }

    #[test]
    fn separates_every_control_flow_statement_from_adjacent_siblings() {
        for statement in [
            "if(ok)work();",
            "switch(kind){case 1:work();break;default:break;}",
            "for(;;)work();",
            "for(const key in object)work();",
            "for(const value of values)work();",
            "while(ok)work();",
            "do work();while(ok);",
            "try{work();}catch(error){recover(error);}finally{cleanup();}",
        ] {
            let source = format!("function f(){{before();{statement}after();}}");
            let expected =
                format!("function f(){{\n  before();\n\n  {statement}\n\n  after();\n}}");
            let output = format_with_control_flow_spacing(&source, StatementSpacingMode::Separate);

            assert_eq!(output, expected, "{statement}");
            assert_eq!(
                format_with_control_flow_spacing(&output, StatementSpacingMode::Separate),
                output,
                "{statement}"
            );
        }

        let for_await =
            "async function f(){before();for await(const item of items)use(item);after();}";
        assert_eq!(
            format_with_control_flow_spacing(for_await, StatementSpacingMode::Separate),
            "async function f(){\n  before();\n\n  for await(const item of items)use(item);\n\n  after();\n}"
        );
    }

    #[test]
    fn applies_control_flow_modes_and_preserves_lone_and_labelled_statements() {
        let source = "function f(){before();if(ok)work();after();}";

        assert_eq!(
            format_with_control_flow_spacing(source, StatementSpacingMode::Separate),
            "function f(){\n  before();\n\n  if(ok)work();\n\n  after();\n}"
        );
        assert_eq!(
            format_with_control_flow_spacing(source, StatementSpacingMode::Compact),
            "function f(){\n  before();\n  if(ok)work();\n  after();\n}"
        );
        assert_eq!(
            format_with_control_flow_spacing(source, StatementSpacingMode::Off),
            source
        );

        for unchanged in [
            "function f(){if(ok)work();}",
            "function f(){before();label:for(;;)work();after();}",
        ] {
            assert_eq!(
                format_with_control_flow_spacing(unchanged, StatementSpacingMode::Separate),
                unchanged
            );
        }
    }

    #[test]
    fn preserves_lone_control_flow_parents_only_for_direct_statement_spacing() {
        let direct = "function f(){if(ok){before();while(ready)tick();after();}}";
        assert_eq!(
            format_with_control_flow_spacing(direct, StatementSpacingMode::Separate),
            "function f(){if(ok){\n    before();\n\n    while(ready)tick();\n\n    after();\n  }}"
        );

        let cascading = "function f(){if(ok){const value=1;work();}}";
        assert_eq!(
            format_with_semicolons_off(
                cascading,
                FormatConfig {
                    rules: RulesConfig {
                        import_layout: false,
                        interface_layout: InterfaceLayoutRule::Mode(InterfaceLayoutMode::Off),
                        object_property_spacing: false,
                        statement_spacing: StatementSpacingConfig {
                            control_flow_statements: StatementSpacingMode::Separate,
                            imports: StatementSpacingMode::Off,
                            multiline_call_statements: StatementSpacingMode::Off,
                            return_statements: StatementSpacingMode::Off,
                            type_aliases: StatementSpacingMode::Off,
                            variable_declarations: StatementSpacingMode::Separate,
                        },
                        trailing_commas: TrailingCommaMode::Off,
                        ..RulesConfig::default()
                    },
                    ..FormatConfig::default()
                },
            ),
            "function f(){\n  if(ok){\n    const value=1;\n\n    work();\n  }\n}"
        );
    }

    #[test]
    fn keeps_control_flow_owner_clauses_together_and_formats_nested_lists() {
        let owners = "function f(){before();if(ok){one();}else{two();}try{run();}catch(error){recover(error);}finally{cleanup();}after();}";
        let expected_owners = "function f(){\n  before();\n\n  if(ok){one();}else{two();}\n\n  try{run();}catch(error){recover(error);}finally{cleanup();}\n\n  after();\n}";
        assert_eq!(
            format_with_control_flow_spacing(owners, StatementSpacingMode::Separate),
            expected_owners
        );

        let nested = "function outer(){if(ok){prepare();while(ready)tick();finish();}switch(kind){case 1:work();for(;;)next();break;default:break;}}";
        let expected_nested = "function outer(){\n  if(ok){\n    prepare();\n\n    while(ready)tick();\n\n    finish();\n  }\n\n  switch(kind){\n    case 1:\n      work();\n\n      for(;;)next();\n\n      break;\n    default:break;\n  }\n}";
        let output = format_with_control_flow_spacing(nested, StatementSpacingMode::Separate);
        assert_eq!(output, expected_nested);
        assert_eq!(
            format_with_control_flow_spacing(&output, StatementSpacingMode::Separate),
            output
        );
    }

    #[test]
    fn combines_control_flow_with_other_spacing_by_priority() {
        let format = |source, control_flow_statements, return_statements, variable_declarations| {
            format_with_semicolons_off(
                source,
                FormatConfig {
                    rules: RulesConfig {
                        import_layout: false,
                        interface_layout: InterfaceLayoutRule::Mode(InterfaceLayoutMode::Off),
                        object_property_spacing: false,
                        statement_spacing: StatementSpacingConfig {
                            control_flow_statements,
                            imports: StatementSpacingMode::Off,
                            multiline_call_statements: StatementSpacingMode::Off,
                            return_statements,
                            type_aliases: StatementSpacingMode::Off,
                            variable_declarations,
                        },
                        trailing_commas: TrailingCommaMode::Off,
                        ..RulesConfig::default()
                    },
                    ..FormatConfig::default()
                },
            )
        };
        let source = "function f(){const value=1;if(value)work();return value;}";

        assert_eq!(
            format(
                source,
                StatementSpacingMode::Separate,
                StatementSpacingMode::Compact,
                StatementSpacingMode::Compact
            ),
            "function f(){\n  const value=1;\n\n  if(value)work();\n\n  return value;\n}"
        );
        assert_eq!(
            format(
                source,
                StatementSpacingMode::Compact,
                StatementSpacingMode::Separate,
                StatementSpacingMode::Compact
            ),
            "function f(){\n  const value=1;\n  if(value)work();\n\n  return value;\n}"
        );

        let format_module = |source, control_flow_statements, imports, type_aliases| {
            format_with_semicolons_off(
                source,
                FormatConfig {
                    rules: RulesConfig {
                        import_layout: false,
                        interface_layout: InterfaceLayoutRule::Mode(InterfaceLayoutMode::Off),
                        object_property_spacing: false,
                        statement_spacing: StatementSpacingConfig {
                            control_flow_statements,
                            imports,
                            multiline_call_statements: StatementSpacingMode::Off,
                            return_statements: StatementSpacingMode::Off,
                            type_aliases,
                            variable_declarations: StatementSpacingMode::Off,
                        },
                        trailing_commas: TrailingCommaMode::Off,
                        ..RulesConfig::default()
                    },
                    ..FormatConfig::default()
                },
            )
        };
        let module = "import'x';if(ok)work();type A=1;";

        assert_eq!(
            format_module(
                module,
                StatementSpacingMode::Compact,
                StatementSpacingMode::Separate,
                StatementSpacingMode::Compact
            ),
            "import'x';\n\nif(ok)work();\ntype A=1;"
        );
        assert_eq!(
            format_module(
                module,
                StatementSpacingMode::Compact,
                StatementSpacingMode::Compact,
                StatementSpacingMode::Separate
            ),
            "import'x';\nif(ok)work();\n\ntype A=1;"
        );
    }

    #[test]
    fn preserves_control_flow_comments_line_endings_and_do_while_semicolon() {
        let source = "\u{feff}function f(){\r\n  before(); // trailing\r\n  // leading\r\n  while(ok)work();\r\n}";
        let output = format_with_control_flow_spacing(source, StatementSpacingMode::Separate);
        assert_eq!(
            output,
            "\u{feff}function f(){\r\n  before(); // trailing\r\n\r\n  // leading\r\n  while(ok)work();\r\n}"
        );
        assert_eq!(output.matches("// trailing").count(), 1);
        assert_eq!(output.matches("// leading").count(), 1);
        assert!(!output.replace("\r\n", "").contains('\n'));
        assert!(!output.ends_with('\n'));

        let detached = "function f() {\n  do work()\n  while (ok)\n  ;\n  after()\n}";
        let normalized = format_with_control_flow_spacing(detached, StatementSpacingMode::Separate);
        assert_eq!(
            normalized,
            "function f() {\n  do work()\n  while (ok);\n\n  after()\n}"
        );
        assert_eq!(
            format_with_control_flow_spacing(&normalized, StatementSpacingMode::Separate),
            normalized
        );

        let commented_detached =
            "function f() {\n  do work()\n  while (ok) // keep\n  ;\n  after()\n}";
        let preserved =
            format_with_control_flow_spacing(commented_detached, StatementSpacingMode::Separate);
        assert_eq!(
            preserved,
            "function f() {\n  do work()\n  while (ok) // keep\n  ;\n\n  after()\n}"
        );
        assert_eq!(
            format_with_control_flow_spacing(&preserved, StatementSpacingMode::Separate),
            preserved
        );
    }

    #[test]
    fn places_html_close_comments_after_unicode_line_separators() {
        for separator in ['\u{2028}', '\u{2029}'] {
            let source = format!("if (ok) work();{separator}--> note\nnext();");

            assert_eq!(
                format_with_control_flow_spacing(&source, StatementSpacingMode::Separate),
                "if (ok) work();\n\n--> note\nnext();",
                "U+{:04X}",
                u32::from(separator)
            );
        }
    }

    #[test]
    fn preserves_control_flow_typescript_directive_scope() {
        for directive in ["@ts-ignore", "@ts-expect-error"] {
            let source =
                format!("function f(){{\n  // {directive}\n  if(missingA)work();missingB();\n}}");
            assert_eq!(
                format_with_control_flow_spacing(&source, StatementSpacingMode::Separate),
                source,
                "{directive}"
            );
        }
    }

    #[test]
    fn separates_return_statements_from_every_adjacent_sibling() {
        let source = "function f(){before();return first;return second;after();}";
        let expected =
            "function f(){\n  before();\n\n  return first;\n\n  return second;\n\n  after();\n}";
        let output = format_with_return_spacing(source, StatementSpacingMode::Separate);

        assert_eq!(output, expected);
        assert_eq!(
            format_with_return_spacing(&output, StatementSpacingMode::Separate),
            output
        );

        let extra_blank_lines = "function f() {\n  before()\n\n\n\n  return value\n}";
        assert_eq!(
            format_with_return_spacing(extra_blank_lines, StatementSpacingMode::Separate),
            "function f() {\n  before()\n\n  return value\n}"
        );
    }

    #[test]
    fn applies_all_return_spacing_modes_without_expanding_a_lone_return() {
        let source = "function f(){work();return value;}";

        assert_eq!(
            format_with_return_spacing(source, StatementSpacingMode::Separate),
            "function f(){\n  work();\n\n  return value;\n}"
        );
        assert_eq!(
            format_with_return_spacing(source, StatementSpacingMode::Compact),
            "function f(){\n  work();\n  return value;\n}"
        );
        assert_eq!(
            format_with_return_spacing(source, StatementSpacingMode::Off),
            source
        );

        let lone_return = "function f(){return;}";
        assert_eq!(
            format_with_return_spacing(lone_return, StatementSpacingMode::Separate),
            lone_return
        );
    }

    #[test]
    fn nested_return_spacing_keeps_a_lone_return_parent_inline() {
        let source = "function f(){return (()=>{work();return value})()}";
        let expected = "function f(){return (()=>{\n    work();\n\n    return value\n  })()}";
        let output = format_with_return_spacing(source, StatementSpacingMode::Separate);

        assert_eq!(output, expected);
        assert_eq!(
            format_with_return_spacing(&output, StatementSpacingMode::Separate),
            output
        );
    }

    #[test]
    fn non_return_layouts_still_cascade_through_a_lone_return_parent() {
        let source = "function f(){return (()=>{const value=1;work();})()}";
        let output = format_with_semicolons_off(
            source,
            FormatConfig {
                rules: RulesConfig {
                    import_layout: false,
                    interface_layout: InterfaceLayoutRule::Mode(InterfaceLayoutMode::Off),
                    object_property_spacing: false,
                    statement_spacing: StatementSpacingConfig {
                        control_flow_statements: StatementSpacingMode::Off,
                        imports: StatementSpacingMode::Off,
                        multiline_call_statements: StatementSpacingMode::Off,
                        return_statements: StatementSpacingMode::Separate,
                        type_aliases: StatementSpacingMode::Off,
                        variable_declarations: StatementSpacingMode::Separate,
                    },
                    trailing_commas: TrailingCommaMode::Off,
                    ..RulesConfig::default()
                },
                ..FormatConfig::default()
            },
        );

        assert_eq!(
            output,
            "function f(){\n  return (()=>{\n    const value=1;\n\n    work();\n  })()\n}"
        );
    }

    #[test]
    fn formats_returns_recursively_at_their_direct_statement_list_level() {
        let source = "function outer() {\n  if (ok) {\n    prepare()\n    return value\n  }\n  switch (kind) {\n    case 1:\n      work()\n      return one\n    default:\n      return fallback\n  }\n  finish()\n}";
        let expected = "function outer() {\n  if (ok) {\n    prepare()\n\n    return value\n  }\n  switch (kind) {\n    case 1:\n      work()\n\n      return one\n    default:\n      return fallback\n  }\n  finish()\n}";
        let output = format_with_return_spacing(source, StatementSpacingMode::Separate);

        assert_eq!(output, expected);
        assert_eq!(
            format_with_return_spacing(&output, StatementSpacingMode::Separate),
            output
        );

        let unbraced = "function f(){if(ok)return value;finish();}";
        assert_eq!(
            format_with_return_spacing(unbraced, StatementSpacingMode::Separate),
            unbraced
        );
    }

    #[test]
    fn combines_return_and_other_spacing_requirements_by_priority() {
        let format = |source, return_statements, type_aliases, variable_declarations| {
            format_with_semicolons_off(
                source,
                FormatConfig {
                    rules: RulesConfig {
                        import_layout: false,
                        interface_layout: InterfaceLayoutRule::Mode(InterfaceLayoutMode::Off),
                        object_property_spacing: false,
                        statement_spacing: StatementSpacingConfig {
                            control_flow_statements: StatementSpacingMode::Off,
                            imports: StatementSpacingMode::Off,
                            multiline_call_statements: StatementSpacingMode::Off,
                            return_statements,
                            type_aliases,
                            variable_declarations,
                        },
                        trailing_commas: TrailingCommaMode::Off,
                        ..RulesConfig::default()
                    },
                    ..FormatConfig::default()
                },
            )
        };
        let variable_source = "function f(){const value=1;return value;}";
        let separate_variables = "function f(){\n  const value=1;\n\n  return value;\n}";
        let compact_variables = "function f(){\n  const value=1;\n  return value;\n}";

        assert_eq!(
            format(
                variable_source,
                StatementSpacingMode::Separate,
                StatementSpacingMode::Off,
                StatementSpacingMode::Compact
            ),
            separate_variables
        );
        assert_eq!(
            format(
                variable_source,
                StatementSpacingMode::Compact,
                StatementSpacingMode::Off,
                StatementSpacingMode::Separate
            ),
            separate_variables
        );
        assert_eq!(
            format(
                variable_source,
                StatementSpacingMode::Compact,
                StatementSpacingMode::Off,
                StatementSpacingMode::Compact
            ),
            compact_variables
        );
        assert_eq!(
            format(
                variable_source,
                StatementSpacingMode::Off,
                StatementSpacingMode::Off,
                StatementSpacingMode::Off
            ),
            variable_source
        );

        let type_alias_source = "function f(){type Value=number;return value;}";
        assert_eq!(
            format(
                type_alias_source,
                StatementSpacingMode::Compact,
                StatementSpacingMode::Separate,
                StatementSpacingMode::Off
            ),
            "function f(){\n  type Value=number;\n\n  return value;\n}"
        );
    }

    #[test]
    fn preserves_return_comments_line_endings_and_asi_shape() {
        let source = "\u{feff}function f(){\r\n  before(); // trailing\r\n  // leading\r\n  return value;\r\n}";
        let output = format_with_return_spacing(source, StatementSpacingMode::Separate);
        assert_eq!(
            output,
            "\u{feff}function f(){\r\n  before(); // trailing\r\n\r\n  // leading\r\n  return value;\r\n}"
        );
        assert_eq!(output.matches("// trailing").count(), 1);
        assert_eq!(output.matches("// leading").count(), 1);
        assert!(!output.replace("\r\n", "").contains('\n'));
        assert!(!output.ends_with('\n'));

        let asi_sensitive = "function f() {\n  return\n  value()\n}";
        assert_eq!(
            format_with_return_spacing(asi_sensitive, StatementSpacingMode::Separate),
            "function f() {\n  return\n\n  value()\n}"
        );

        let detached_semicolon = "function f() {\n  return value\n  ;\n  work()\n}";
        let normalized =
            format_with_return_spacing(detached_semicolon, StatementSpacingMode::Separate);
        assert_eq!(normalized, "function f() {\n  return value;\n\n  work()\n}");
        assert_eq!(
            format_with_return_spacing(&normalized, StatementSpacingMode::Separate),
            normalized
        );
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
        let output = format_with_semicolons_off(
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
    fn preserves_a_typed_class_field_before_its_constructor() {
        let source = "import{type BaseIssue}from'valibot'\nclass RegistryUserValidationError extends Error {\n  readonly fieldErrors: RegistryUserFieldErrors\n\n  constructor(fieldErrors: RegistryUserFieldErrors) {\n    const details = String(fieldErrors)\n    super(details)\n    this.fieldErrors = fieldErrors\n  }\n}";
        let expected = "import { type BaseIssue } from 'valibot'\n\nclass RegistryUserValidationError extends Error {\n  readonly fieldErrors: RegistryUserFieldErrors\n\n  constructor(fieldErrors: RegistryUserFieldErrors) {\n    const details = String(fieldErrors)\n\n    super(details)\n    this.fieldErrors = fieldErrors\n  }\n}";

        let output = format(source);
        assert_eq!(output, expected);
        assert_eq!(format(&output), output);
    }

    #[test]
    fn keeps_import_layout_and_statement_spacing_independent() {
        let source = "import{a}from'x';const b=1;let c=2;run();";
        let variables_only = format_with_semicolons_off(
            source,
            FormatConfig {
                rules: RulesConfig {
                    import_layout: false,
                    statement_spacing: StatementSpacingConfig {
                        control_flow_statements: StatementSpacingMode::Off,
                        imports: StatementSpacingMode::Off,
                        multiline_call_statements: StatementSpacingMode::Off,
                        return_statements: StatementSpacingMode::Off,
                        type_aliases: StatementSpacingMode::Off,
                        variable_declarations: StatementSpacingMode::Separate,
                    },
                    ..RulesConfig::default()
                },
                ..FormatConfig::default()
            },
        );
        assert_eq!(
            variables_only,
            "import{a}from'x';\n\nconst b=1;\nlet c=2;\n\nrun();"
        );

        let imports_only = format_with_semicolons_off(
            source,
            FormatConfig {
                rules: RulesConfig {
                    import_layout: true,
                    statement_spacing: StatementSpacingConfig {
                        control_flow_statements: StatementSpacingMode::Off,
                        imports: StatementSpacingMode::Separate,
                        multiline_call_statements: StatementSpacingMode::Off,
                        return_statements: StatementSpacingMode::Off,
                        type_aliases: StatementSpacingMode::Off,
                        variable_declarations: StatementSpacingMode::Off,
                    },
                    ..RulesConfig::default()
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
            "function outer(){\n  if(ok){\n    const a=1;\n\n    work();\n  }\n\n  finish();\n}";
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
        let nested_expected = "function f(){\n  switch(x){\n    case 1:\n      const a=1;\n\n      run();\n  }\n\n  finish();\n}";
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
        let expected = "function f() {\n  const a=1;\n\n  work();\n}\n\nswitch(x) {\n  case 1:\n    let b=2;\n\n    done();\n}";
        assert_eq!(format(source), expected);
        assert_eq!(format(expected), expected);
    }

    #[test]
    fn preserves_list_indent_across_multiple_inline_boundaries() {
        let source = "function f() {\n  before(); const a=1; other();\n}\nswitch(x) {\n  case 1:\n    before(); let b=2; done();\n}";
        let expected = "function f() {\n  before();\n  const a=1;\n  other();\n}\nswitch(x) {\n  case 1:\n    before();\n    let b=2;\n    done();\n}";
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
    fn excludes_non_runtime_and_ambient_variable_declarations() {
        let source = "export const exported=1;export let mutable=2;export var legacy=3;declare const ambient:number;using resource=get();await using asyncResource=getAsync();for(let i=0;i<1;i++)work();for(const item of items)use(item);";
        assert_eq!(
            format_with_rules(
                source,
                false,
                StatementSpacingMode::Off,
                StatementSpacingMode::Separate,
            ),
            source
        );

        for ambient in [
            "declare namespace Ambient { const value:number; function work():void; }",
            "declare module 'pkg' { const value:number; function work():void; }",
            "declare global { const value:number; function work():void; }",
        ] {
            assert_eq!(
                format_with_rules(
                    ambient,
                    false,
                    StatementSpacingMode::Off,
                    StatementSpacingMode::Separate,
                ),
                ambient
            );
        }

        let definition = "const first:number;const second:string;";
        for file_name in ["types.d.ts", "types.d.mts", "types.d.cts"] {
            assert_eq!(
                format_file_with(
                    file_name,
                    definition,
                    FormatConfig {
                        rules: RulesConfig {
                            semicolons: semicolons_off(),
                            ..RulesConfig::default()
                        },
                        ..FormatConfig::default()
                    },
                ),
                definition
            );
        }
    }

    #[test]
    fn preserves_a_definition_union_with_a_final_line_comment() {
        let declaration = "type VectorizeIndexConfig = {\n  dimensions: number;\n} | {\n  preset: string; // keep this generic\n};";
        assert_eq!(
            format_file_with(
                "worker-configuration.d.ts",
                declaration,
                FormatConfig {
                    rules: RulesConfig {
                        semicolons: semicolons_off(),
                        ..RulesConfig::default()
                    },
                    ..FormatConfig::default()
                }
            ),
            declaration
        );

        let source = format!("import{{type Value}}from'pkg'\n{declaration}");
        let expected = format!("import {{ type Value }} from 'pkg'\n\n{declaration}");
        let output = format_file_with(
            "worker-configuration.d.ts",
            &source,
            FormatConfig {
                rules: RulesConfig {
                    semicolons: semicolons_off(),
                    ..RulesConfig::default()
                },
                ..FormatConfig::default()
            },
        );
        assert_eq!(output, expected);
        assert_eq!(
            format_file_with(
                "worker-configuration.d.ts",
                &output,
                FormatConfig {
                    rules: RulesConfig {
                        semicolons: semicolons_off(),
                        ..RulesConfig::default()
                    },
                    ..FormatConfig::default()
                }
            ),
            output
        );
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

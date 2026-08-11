use std::cell::{Cell, RefCell};
use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, ArrayExpression, ArrayExpressionElement, ArrowFunctionBody, AssignmentTarget,
    BindingPattern, CallExpression, ChainElement, Comment, ComputedMemberExpression, Expression,
    ForStatementInit, ForStatementLeft, Function, FunctionBody, ImportDeclaration,
    ImportDeclarationSpecifier, ImportOrExportKind, LogicalExpression, ModuleExportName,
    NewExpression, ObjectExpression, ObjectProperty, ObjectPropertyKind, PrivateFieldExpression,
    Program, PropertyKey, SimpleAssignmentTarget, Statement, StaticMemberExpression, StringLiteral,
    TSType, TemplateLiteral, VariableDeclaration,
};
use oxc_parser::{Kind, ParseOptions, Parser, Token, config::TokensParserConfig};
use oxc_span::{ContentEq, GetSpan, SourceType, Span};

use crate::comments::CommentTracker;
use crate::config::{
    ArrayObjectLayout, CollectionItemLayout, CollectionLayout, LineEnding, ObjectArrayLayout,
    Semicolons, StatementKind, StatementScope, StatementSelector, TrailingCommas,
};
use crate::doc::{
    Doc, StatementSpacingCondition, concat, empty, force_flat, forces_line_break, group, hard_line,
    indent, line_or_space, line_suffix, measured, render, soft_line, space, statement_separator,
    surround, text, token,
};
use crate::index::NodeIndex;
use crate::precedence::{
    Associativity, ParentContext, ParentPosition, Precedence, needs_parentheses,
};
use crate::{FormatError, QuoteStyle, ResolvedConfig};

const BOM: char = '\u{feff}';

/// Formats JavaScript, TypeScript, JSX, or TSX source text.
///
/// # Errors
///
/// Returns a [`FormatError`] when the source type is unsupported, parsing or
/// semantic verification fails, or the formatter cannot preserve its internal
/// invariants.
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
    let parsed = parse(&allocator, source, source_type)?;

    if parsed.is_flow_language {
        return Err(FormatError::UnsupportedSource {
            message: "Flow is not supported in v0.1".to_owned(),
        });
    }
    let newline = resolve_newline(source, config.line_ending());
    let printer = Printer::new(
        source,
        config,
        &parsed.program,
        &parsed.tokens,
        &parsed.program.comments,
    );
    let document = printer.program_doc(&parsed.program)?;
    printer.finish_comments()?;
    let mut output = render(&document, config, newline);
    normalize_final_newline(&mut output, newline, config.final_newline());
    output.insert_str(0, bom);

    if config.verify_ast() {
        verify(
            file_name,
            source_type,
            &parsed.program,
            output.strip_prefix(BOM).unwrap_or(&output),
        )?;
    }

    if output == source_text {
        Ok(None)
    } else {
        Ok(Some(output))
    }
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
    let options = ParseOptions {
        preserve_parens: false,
        enable_ident_hashes: false,
        allow_return_outside_function: true,
        allow_v8_intrinsics: true,
    };
    let parsed = Parser::new(allocator, source, source_type)
        .with_options(options)
        .with_config(TokensParserConfig)
        .parse();

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

fn resolve_newline(source: &str, line_ending: LineEnding) -> &'static str {
    match line_ending {
        LineEnding::Lf => "\n",
        LineEnding::Crlf => "\r\n",
        LineEnding::Preserve => {
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
    }
}

fn normalize_final_newline(output: &mut String, newline: &str, final_newline: bool) {
    while output.ends_with('\n') || output.ends_with('\r') {
        output.pop();
    }
    if final_newline {
        output.push_str(newline);
    }
}

struct Printer<'a> {
    source: &'a str,
    tokens: &'a [Token],
    config: &'a ResolvedConfig,
    node_index: NodeIndex,
    comments: RefCell<CommentTracker<'a>>,
    next_measurement_id: Cell<usize>,
}

impl<'a> Printer<'a> {
    fn new(
        source: &'a str,
        config: &'a ResolvedConfig,
        program: &Program<'_>,
        tokens: &'a [Token],
        comments: &[Comment],
    ) -> Self {
        let node_index = NodeIndex::build(program, tokens);
        let comments = CommentTracker::new(source, comments, &node_index);
        Self {
            source,
            tokens,
            config,
            node_index,
            comments: RefCell::new(comments),
            next_measurement_id: Cell::new(0),
        }
    }

    fn program_doc(&self, program: &Program<'_>) -> Result<Doc, FormatError> {
        let mut docs = Vec::new();

        if let Some(hashbang) = &program.hashbang {
            docs.push(line_suffix(text(self.slice(hashbang.span)?)));
            docs.push(hard_line());
        }

        for directive in &program.directives {
            docs.push(concat([
                token(self.string_literal(&directive.expression)?),
                self.semicolon(),
                hard_line(),
            ]));
        }

        let root_comments = self.dangling(program.span)?;
        if !root_comments.is_empty() {
            docs.push(concat(comments_with_lines(root_comments)));
            if !program.body.is_empty() {
                docs.push(hard_line());
            }
        }

        docs.push(self.statement_sequence(&program.body, StatementScope::TopLevel)?);

        Ok(concat(docs))
    }

    fn statement_sequence(
        &self,
        statements: &[Statement<'_>],
        scope: StatementScope,
    ) -> Result<Doc, FormatError> {
        let statements = statements
            .iter()
            .map(|statement| {
                let measurement_id = self.next_measurement_id.get();
                self.next_measurement_id.set(measurement_id + 1);
                let doc = self.measured_statement(statement, measurement_id)?;
                Ok((
                    measurement_id,
                    doc,
                    classify_statement(statement),
                    crate::asi::needs_leading_semicolon(statement),
                ))
            })
            .collect::<Result<Vec<_>, FormatError>>()?;
        let mut docs = Vec::new();
        for (index, (_, doc, _, needs_asi_guard)) in statements.iter().enumerate() {
            if index > 0
                && matches!(self.config.semicolons(), Semicolons::AsNeeded)
                && *needs_asi_guard
            {
                docs.push(token(";"));
            }
            docs.push(doc.clone());
            if let Some(next) = statements.get(index + 1) {
                let conditions =
                    self.statement_spacing_conditions(statements[index].2, next.2, scope);
                docs.push(statement_separator(statements[index].0, next.0, conditions));
            }
        }
        Ok(concat(docs))
    }

    fn statement(&self, statement: &Statement<'_>) -> Result<Doc, FormatError> {
        self.statement_with_measurement(statement, None)
    }

    fn measured_statement(
        &self,
        statement: &Statement<'_>,
        measurement_id: usize,
    ) -> Result<Doc, FormatError> {
        self.statement_with_measurement(statement, Some(measurement_id))
    }

    #[allow(
        clippy::too_many_lines,
        reason = "exhaustive statement dispatch stays together so new Oxc variants fail compilation"
    )]
    fn statement_with_measurement(
        &self,
        statement: &Statement<'_>,
        measurement_id: Option<usize>,
    ) -> Result<Doc, FormatError> {
        let span = statement.span();
        if let Some((directive, raw)) = self.suppressed(span)? {
            let raw = measurement_id.map_or(raw.clone(), |id| measured(id, raw));
            return self.commented(span, concat([directive, raw]));
        }
        let doc = match statement {
            Statement::VariableDeclaration(declaration) => {
                self.variable_declaration(declaration, true)
            }
            Statement::ExpressionStatement(statement) => Ok(concat([
                self.expression_statement(&statement.expression)?,
                self.semicolon(),
            ])),
            Statement::ImportDeclaration(declaration) => self.import_declaration(declaration),
            Statement::BlockStatement(block) => self.block_statement(block),
            Statement::IfStatement(if_statement) => self.if_statement(if_statement),
            Statement::WhileStatement(while_statement) => Ok(concat([
                token("while"),
                space(),
                self.parenthesized_expression(&while_statement.test)?,
                self.control_body(&while_statement.body)?,
            ])),
            Statement::DoWhileStatement(do_while) => Ok(concat([
                token("do"),
                self.control_body(&do_while.body)?,
                if matches!(do_while.body, Statement::BlockStatement(_)) {
                    space()
                } else {
                    hard_line()
                },
                token("while"),
                space(),
                self.parenthesized_expression(&do_while.test)?,
                self.semicolon(),
            ])),
            Statement::ForStatement(for_statement) => self.for_statement(for_statement),
            Statement::ForInStatement(for_in) => Ok(concat([
                token("for"),
                space(),
                token("("),
                self.for_left(&for_in.left)?,
                space(),
                token("in"),
                space(),
                self.expression(&for_in.right, false)?,
                token(")"),
                self.control_body(&for_in.body)?,
            ])),
            Statement::ForOfStatement(for_of) => {
                let mut parts = vec![token("for")];
                if for_of.r#await {
                    parts.extend([space(), token("await")]);
                }
                parts.extend([
                    space(),
                    token("("),
                    self.for_left(&for_of.left)?,
                    space(),
                    token("of"),
                    space(),
                    self.assignment_expression(&for_of.right, false)?,
                    token(")"),
                    self.control_body(&for_of.body)?,
                ]);
                Ok(concat(parts))
            }
            Statement::BreakStatement(statement) => {
                Ok(self.keyword_with_label("break", statement.label.as_ref()))
            }
            Statement::ContinueStatement(statement) => {
                Ok(self.keyword_with_label("continue", statement.label.as_ref()))
            }
            Statement::ReturnStatement(statement) => {
                self.keyword_with_argument("return", statement.argument.as_ref())
            }
            Statement::ThrowStatement(statement) => {
                self.keyword_with_argument("throw", Some(&statement.argument))
            }
            Statement::DebuggerStatement(_) => Ok(concat([token("debugger"), self.semicolon()])),
            Statement::LabeledStatement(labeled) => Ok(concat([
                token(labeled.label.name.as_str()),
                token(":"),
                self.control_body(&labeled.body)?,
            ])),
            Statement::SwitchStatement(switch) => self.switch_statement(switch),
            Statement::TryStatement(try_statement) => self.try_statement(try_statement),
            Statement::WithStatement(with_statement) => Ok(concat([
                token("with"),
                space(),
                self.parenthesized_expression(&with_statement.object)?,
                self.control_body(&with_statement.body)?,
            ])),
            Statement::FunctionDeclaration(function) => self.function(function),
            Statement::ClassDeclaration(class) => self.class(class),
            Statement::ExportDeclaration(export) => Ok(concat([
                token("export"),
                space(),
                self.statement(export.declaration.as_statement())?,
            ])),
            Statement::ExportNamedDeclaration(export) => self.export_named(export),
            Statement::ExportFromDeclaration(export) => self.export_from(export),
            Statement::ExportAllDeclaration(export) => self.export_all(export),
            Statement::ExportDefaultDeclaration(export) => self.export_default(export),
            Statement::EmptyStatement(_) => Ok(token(";")),
            Statement::TSTypeAliasDeclaration(declaration) => self.syntax_doc(declaration.span),
            Statement::TSInterfaceDeclaration(declaration) => self.syntax_doc(declaration.span),
            Statement::TSEnumDeclaration(declaration) => self.syntax_doc(declaration.span),
            Statement::TSExternalModuleDeclaration(declaration) => {
                self.syntax_doc(declaration.span)
            }
            Statement::TSNamespaceDeclaration(declaration) => self.syntax_doc(declaration.span),
            Statement::TSGlobalDeclaration(declaration) => self.syntax_doc(declaration.span),
            Statement::TSImportEqualsDeclaration(declaration) => self.syntax_doc(declaration.span),
            Statement::TSExportAssignment(declaration) => self.syntax_doc(declaration.span),
            Statement::TSNamespaceExportDeclaration(declaration) => {
                self.syntax_doc(declaration.span)
            }
        }?;
        let doc = measurement_id.map_or(doc.clone(), |id| measured(id, doc));
        self.commented(span, doc)
    }

    fn variable_declaration(
        &self,
        declaration: &VariableDeclaration<'_>,
        include_semicolon: bool,
    ) -> Result<Doc, FormatError> {
        let mut declarators = Vec::with_capacity(declaration.declarations.len());
        for declarator in &declaration.declarations {
            declarators.push(self.node_doc(declarator.span, || {
                let mut parts = vec![self.binding_pattern(&declarator.id)?];
                if declarator.definite {
                    parts.push(token("!"));
                }
                if let Some(type_annotation) = &declarator.type_annotation {
                    parts.push(self.syntax_doc(type_annotation.span)?);
                }
                if let Some(initializer) = &declarator.init {
                    parts.extend([
                        space(),
                        token("="),
                        space(),
                        self.assignment_expression(initializer, false)?,
                    ]);
                }
                Ok(concat(parts))
            })?);
        }

        let mut declarators = declarators.into_iter();
        let mut body = declarators.next().into_iter().collect::<Vec<_>>();
        for declarator in declarators {
            body.extend([token(","), indent(concat([line_or_space(), declarator]))]);
        }
        let mut parts = Vec::new();
        if declaration.declare {
            parts.extend([token("declare"), space()]);
        }
        parts.extend([
            token(declaration.kind.as_str()),
            space(),
            group(concat(body)),
        ]);
        if include_semicolon {
            parts.push(self.semicolon());
        }
        Ok(group(concat(parts)))
    }

    fn block_statement(
        &self,
        block: &oxc_ast::ast::BlockStatement<'_>,
    ) -> Result<Doc, FormatError> {
        self.block_contents(&[], &block.body, block.span)
    }

    fn block_contents(
        &self,
        directives: &[oxc_ast::ast::Directive<'_>],
        statements: &[Statement<'_>],
        span: Span,
    ) -> Result<Doc, FormatError> {
        let dangling = self.dangling(span)?;
        if directives.is_empty() && statements.is_empty() && dangling.is_empty() {
            return Ok(token("{}"));
        }
        let mut body = comments_with_lines(dangling);
        if !body.is_empty() && (!directives.is_empty() || !statements.is_empty()) {
            body.push(hard_line());
        }
        for directive in directives {
            body.extend([
                token(self.string_literal(&directive.expression)?),
                self.semicolon(),
                hard_line(),
            ]);
        }
        if !statements.is_empty() {
            body.push(self.statement_sequence(statements, StatementScope::Block)?);
        }
        Ok(concat([
            token("{"),
            indent(concat([hard_line(), concat(body)])),
            hard_line(),
            token("}"),
        ]))
    }

    fn control_body(&self, statement: &Statement<'_>) -> Result<Doc, FormatError> {
        let body = self.statement(statement)?;
        Ok(if matches!(statement, Statement::BlockStatement(_)) {
            concat([space(), body])
        } else {
            indent(concat([hard_line(), body]))
        })
    }

    fn if_statement(&self, statement: &oxc_ast::ast::IfStatement<'_>) -> Result<Doc, FormatError> {
        let consequent_is_block = matches!(statement.consequent, Statement::BlockStatement(_));
        let mut parts = vec![
            token("if"),
            space(),
            self.parenthesized_expression(&statement.test)?,
            self.control_body(&statement.consequent)?,
        ];
        if let Some(alternate) = &statement.alternate {
            parts.push(if consequent_is_block {
                space()
            } else {
                hard_line()
            });
            parts.push(token("else"));
            if matches!(
                alternate,
                Statement::IfStatement(_) | Statement::BlockStatement(_)
            ) {
                parts.extend([space(), self.statement(alternate)?]);
            } else {
                parts.push(indent(concat([hard_line(), self.statement(alternate)?])));
            }
        }
        Ok(concat(parts))
    }

    fn for_statement(
        &self,
        statement: &oxc_ast::ast::ForStatement<'_>,
    ) -> Result<Doc, FormatError> {
        let init = match &statement.init {
            None => empty(),
            Some(ForStatementInit::VariableDeclaration(declaration)) => {
                self.variable_declaration(declaration, false)?
            }
            Some(init) => self.expression(init.to_expression(), false)?,
        };
        let test = statement
            .test
            .as_ref()
            .map(|expression| self.expression(expression, false))
            .transpose()?
            .unwrap_or_else(empty);
        let update = statement
            .update
            .as_ref()
            .map(|expression| self.expression(expression, false))
            .transpose()?
            .unwrap_or_else(empty);
        Ok(concat([
            token("for"),
            space(),
            token("("),
            init,
            token(";"),
            if statement.test.is_some() {
                space()
            } else {
                empty()
            },
            test,
            token(";"),
            if statement.update.is_some() {
                space()
            } else {
                empty()
            },
            update,
            token(")"),
            self.control_body(&statement.body)?,
        ]))
    }

    fn for_left(&self, left: &ForStatementLeft<'_>) -> Result<Doc, FormatError> {
        match left {
            ForStatementLeft::VariableDeclaration(declaration) => {
                self.variable_declaration(declaration, false)
            }
            _ => self.assignment_target(left.to_assignment_target()),
        }
    }

    fn parenthesized_expression(&self, expression: &Expression<'_>) -> Result<Doc, FormatError> {
        Ok(concat([
            token("("),
            self.expression(expression, false)?,
            token(")"),
        ]))
    }

    fn keyword_with_label(
        &self,
        keyword: &'static str,
        label: Option<&oxc_ast::ast::LabelIdentifier<'_>>,
    ) -> Doc {
        let mut parts = vec![token(keyword)];
        if let Some(label) = label {
            parts.extend([space(), token(label.name.as_str())]);
        }
        parts.push(self.semicolon());
        concat(parts)
    }

    fn keyword_with_argument(
        &self,
        keyword: &'static str,
        argument: Option<&Expression<'_>>,
    ) -> Result<Doc, FormatError> {
        let mut parts = vec![token(keyword)];
        if let Some(argument) = argument {
            parts.extend([space(), self.expression(argument, false)?]);
        }
        parts.push(self.semicolon());
        Ok(concat(parts))
    }

    fn switch_statement(
        &self,
        statement: &oxc_ast::ast::SwitchStatement<'_>,
    ) -> Result<Doc, FormatError> {
        if statement.cases.is_empty() {
            return Ok(concat([
                token("switch"),
                space(),
                self.parenthesized_expression(&statement.discriminant)?,
                space(),
                token("{}"),
            ]));
        }
        let mut cases = Vec::new();
        for (case_index, case) in statement.cases.iter().enumerate() {
            cases.push(self.node_doc(case.span, || {
                let mut parts = Vec::new();
                if let Some(test) = &case.test {
                    parts.extend([
                        token("case"),
                        space(),
                        self.expression(test, false)?,
                        token(":"),
                    ]);
                } else {
                    parts.extend([token("default"), token(":")]);
                }
                if !case.consequent.is_empty() {
                    parts.push(indent(concat([
                        hard_line(),
                        self.statement_sequence(&case.consequent, StatementScope::Block)?,
                    ])));
                }
                Ok(concat(parts))
            })?);
            if case_index + 1 != statement.cases.len() {
                cases.push(hard_line());
            }
        }
        Ok(concat([
            token("switch"),
            space(),
            self.parenthesized_expression(&statement.discriminant)?,
            space(),
            token("{"),
            indent(concat([hard_line(), concat(cases)])),
            hard_line(),
            token("}"),
        ]))
    }

    fn try_statement(
        &self,
        statement: &oxc_ast::ast::TryStatement<'_>,
    ) -> Result<Doc, FormatError> {
        let mut parts = vec![
            token("try"),
            space(),
            self.block_statement(&statement.block)?,
        ];
        if let Some(handler) = &statement.handler {
            parts.extend([space(), token("catch")]);
            if let Some(param) = &handler.param {
                if param.type_annotation.is_some() {
                    parts.extend([
                        space(),
                        token("("),
                        self.syntax_doc(param.span)?,
                        token(")"),
                    ]);
                } else {
                    parts.extend([
                        space(),
                        token("("),
                        self.binding_pattern(&param.pattern)?,
                        token(")"),
                    ]);
                }
            }
            parts.extend([space(), self.block_statement(&handler.body)?]);
        }
        if let Some(finalizer) = &statement.finalizer {
            parts.extend([
                space(),
                token("finally"),
                space(),
                self.block_statement(finalizer)?,
            ]);
        }
        Ok(concat(parts))
    }

    fn expression(&self, expression: &Expression<'_>, in_array: bool) -> Result<Doc, FormatError> {
        self.expression_with_parent(expression, in_array, None)
    }

    fn assignment_expression(
        &self,
        expression: &Expression<'_>,
        in_array: bool,
    ) -> Result<Doc, FormatError> {
        self.expression_with_parent(
            expression,
            in_array,
            Some(ParentContext::new(
                Precedence::Assignment,
                Associativity::Right,
                ParentPosition::Right,
            )),
        )
    }

    fn expression_statement(&self, expression: &Expression<'_>) -> Result<Doc, FormatError> {
        let doc = self.expression(expression, false)?;
        Ok(if expression_statement_needs_parentheses(expression) {
            concat([token("("), doc, token(")")])
        } else {
            doc
        })
    }

    #[allow(
        clippy::too_many_lines,
        reason = "exhaustive expression dispatch stays together so new Oxc variants fail compilation"
    )]
    fn expression_with_parent(
        &self,
        expression: &Expression<'_>,
        in_array: bool,
        parent: Option<ParentContext>,
    ) -> Result<Doc, FormatError> {
        let span = expression.span();
        if let Some((directive, raw)) = self.suppressed(span)? {
            return self.commented(span, concat([directive, raw]));
        }
        let doc = match expression {
            Expression::BooleanLiteral(literal) => {
                Ok(token(if literal.value { "true" } else { "false" }))
            }
            Expression::NullLiteral(_) => Ok(token("null")),
            Expression::NumericLiteral(literal) => Ok(token(self.slice(literal.span)?)),
            Expression::BigIntLiteral(literal) => Ok(token(self.slice(literal.span)?)),
            Expression::RegExpLiteral(literal) => Ok(token(self.slice(literal.span)?)),
            Expression::StringLiteral(literal) => Ok(token(self.string_literal(literal)?)),
            Expression::TemplateLiteral(literal) => self.template_literal(literal),
            Expression::Identifier(identifier) => Ok(token(identifier.name.as_str())),
            Expression::Super(_) => Ok(token("super")),
            Expression::ArrayExpression(array) => self.array_expression(array),
            Expression::ObjectExpression(object) => self.object_expression(object, in_array),
            Expression::ThisExpression(_) => Ok(token("this")),
            Expression::UnaryExpression(unary) => {
                let separator = if unary.operator.is_keyword() {
                    space()
                } else {
                    empty()
                };
                Ok(concat([
                    token(unary.operator.as_str()),
                    separator,
                    self.expression_with_parent(
                        &unary.argument,
                        false,
                        Some(ParentContext::new(
                            Precedence::Unary,
                            Associativity::Right,
                            ParentPosition::Operand,
                        )),
                    )?,
                ]))
            }
            Expression::UpdateExpression(update) => {
                let argument = self.simple_assignment_target(&update.argument)?;
                Ok(if update.prefix {
                    concat([token(update.operator.as_str()), argument])
                } else {
                    concat([argument, token(update.operator.as_str())])
                })
            }
            Expression::BinaryExpression(binary) => {
                let left = self.expression_with_parent(
                    &binary.left,
                    false,
                    Some(ParentContext::binary(binary.operator, ParentPosition::Left)),
                )?;
                let right = self.expression_with_parent(
                    &binary.right,
                    false,
                    Some(ParentContext::binary(
                        binary.operator,
                        ParentPosition::Right,
                    )),
                )?;
                Ok(group(concat([
                    left,
                    indent(concat([
                        line_or_space(),
                        token(binary.operator.as_str()),
                        space(),
                        right,
                    ])),
                ])))
            }
            Expression::PrivateInExpression(private) => Ok(group(concat([
                token(format!("#{}", private.left.name)),
                indent(concat([
                    line_or_space(),
                    token("in"),
                    space(),
                    self.expression_with_parent(
                        &private.right,
                        false,
                        Some(ParentContext::new(
                            Precedence::Relational,
                            Associativity::Left,
                            ParentPosition::Right,
                        )),
                    )?,
                ])),
            ]))),
            Expression::LogicalExpression(logical) => self.logical_expression(logical),
            Expression::ConditionalExpression(conditional) => Ok(group(concat([
                self.expression_with_parent(
                    &conditional.test,
                    false,
                    Some(ParentContext::new(
                        Precedence::Conditional,
                        Associativity::Right,
                        ParentPosition::Test,
                    )),
                )?,
                indent(concat([
                    line_or_space(),
                    token("?"),
                    space(),
                    self.expression_with_parent(
                        &conditional.consequent,
                        false,
                        Some(ParentContext::new(
                            Precedence::Conditional,
                            Associativity::Right,
                            ParentPosition::Consequent,
                        )),
                    )?,
                    line_or_space(),
                    token(":"),
                    space(),
                    self.expression_with_parent(
                        &conditional.alternate,
                        false,
                        Some(ParentContext::new(
                            Precedence::Conditional,
                            Associativity::Right,
                            ParentPosition::Alternate,
                        )),
                    )?,
                ])),
            ]))),
            Expression::AssignmentExpression(assignment) => Ok(group(concat([
                self.assignment_target(&assignment.left)?,
                indent(concat([
                    line_or_space(),
                    token(assignment.operator.as_str()),
                    space(),
                    self.expression_with_parent(
                        &assignment.right,
                        false,
                        Some(ParentContext::new(
                            Precedence::Assignment,
                            Associativity::Right,
                            ParentPosition::Right,
                        )),
                    )?,
                ])),
            ]))),
            Expression::SequenceExpression(sequence) => {
                let expressions = sequence
                    .expressions
                    .iter()
                    .map(|expression| {
                        self.expression_with_parent(
                            expression,
                            false,
                            Some(ParentContext::new(
                                Precedence::Assignment,
                                Associativity::None,
                                ParentPosition::Right,
                            )),
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(group(join(
                    expressions,
                    concat([token(","), line_or_space()]),
                )))
            }
            Expression::AwaitExpression(await_expression) => Ok(concat([
                token("await"),
                space(),
                self.expression_with_parent(
                    &await_expression.argument,
                    false,
                    Some(ParentContext::new(
                        Precedence::Unary,
                        Associativity::Right,
                        ParentPosition::Operand,
                    )),
                )?,
            ])),
            Expression::YieldExpression(yield_expression) => {
                let mut parts = vec![token("yield")];
                if yield_expression.delegate {
                    parts.push(token("*"));
                }
                if let Some(argument) = &yield_expression.argument {
                    parts.extend([
                        space(),
                        self.expression_with_parent(
                            argument,
                            false,
                            Some(ParentContext::new(
                                Precedence::Assignment,
                                Associativity::Right,
                                ParentPosition::Right,
                            )),
                        )?,
                    ]);
                }
                Ok(concat(parts))
            }
            Expression::ComputedMemberExpression(member) => self.computed_member(member),
            Expression::StaticMemberExpression(member) => self.static_member(member),
            Expression::PrivateFieldExpression(member) => self.private_member(member),
            Expression::CallExpression(call) => self.call_expression(call),
            Expression::NewExpression(new_expression) => self.new_expression(new_expression),
            Expression::TaggedTemplateExpression(tagged) => {
                let mut parts = vec![self.expression_with_parent(
                    &tagged.tag,
                    false,
                    Some(ParentContext::new(
                        Precedence::Member,
                        Associativity::Left,
                        ParentPosition::Tag,
                    )),
                )?];
                if let Some(type_arguments) = &tagged.type_arguments {
                    parts.push(self.syntax_doc(type_arguments.span)?);
                }
                parts.push(self.template_literal(&tagged.quasi)?);
                Ok(concat(parts))
            }
            Expression::ImportExpression(import_expression) => {
                if import_expression.phase.is_some() {
                    return self.syntax_doc(import_expression.span);
                }
                let mut arguments =
                    vec![self.assignment_expression(&import_expression.source, false)?];
                if let Some(options) = &import_expression.options {
                    arguments.push(self.assignment_expression(options, false)?);
                }
                Ok(concat([
                    token("import"),
                    self.arguments_doc(arguments, false),
                ]))
            }
            Expression::ImportMeta(_) => Ok(token("import.meta")),
            Expression::NewTarget(_) => Ok(token("new.target")),
            Expression::FunctionExpression(function) => self.function(function),
            Expression::ArrowFunctionExpression(arrow) => self.arrow_function(arrow),
            Expression::ClassExpression(class) => self.class(class),
            Expression::ParenthesizedExpression(parenthesized) => {
                self.expression(&parenthesized.expression, false)
            }
            Expression::ChainExpression(chain) => self.chain_element(&chain.expression),
            Expression::V8IntrinsicExpression(intrinsic) => {
                let arguments = intrinsic
                    .arguments
                    .iter()
                    .map(|argument| self.argument(argument))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(concat([
                    token(format!("%{}", intrinsic.name.name)),
                    self.arguments_doc(arguments, false),
                ]))
            }
            Expression::JSXElement(expression) => self.jsx_element(expression),
            Expression::JSXFragment(expression) => self.jsx_fragment(expression),
            Expression::TSAsExpression(as_expression) => Ok(concat([
                self.expression_with_parent(
                    &as_expression.expression,
                    false,
                    Some(ParentContext::new(
                        Precedence::Relational,
                        Associativity::Left,
                        ParentPosition::Left,
                    )),
                )?,
                space(),
                token("as"),
                space(),
                self.type_doc(&as_expression.type_annotation)?,
            ])),
            Expression::TSSatisfiesExpression(satisfies) => Ok(concat([
                self.expression_with_parent(
                    &satisfies.expression,
                    false,
                    Some(ParentContext::new(
                        Precedence::Relational,
                        Associativity::Left,
                        ParentPosition::Left,
                    )),
                )?,
                space(),
                token("satisfies"),
                space(),
                self.type_doc(&satisfies.type_annotation)?,
            ])),
            Expression::TSTypeAssertion(assertion) => Ok(concat([
                token("<"),
                self.type_doc(&assertion.type_annotation)?,
                token(">"),
                self.expression_with_parent(
                    &assertion.expression,
                    false,
                    Some(ParentContext::new(
                        Precedence::Unary,
                        Associativity::Right,
                        ParentPosition::Operand,
                    )),
                )?,
            ])),
            Expression::TSNonNullExpression(non_null) => Ok(concat([
                self.expression_with_parent(
                    &non_null.expression,
                    false,
                    Some(ParentContext::new(
                        Precedence::Member,
                        Associativity::Left,
                        ParentPosition::Object,
                    )),
                )?,
                token("!"),
            ])),
            Expression::TSInstantiationExpression(instantiation) => Ok(concat([
                self.expression_with_parent(
                    &instantiation.expression,
                    false,
                    Some(ParentContext::new(
                        Precedence::Member,
                        Associativity::Left,
                        ParentPosition::Callee,
                    )),
                )?,
                self.syntax_doc(instantiation.type_arguments.span)?,
            ])),
        }?;
        let doc = if parent.is_some_and(|parent| needs_parentheses(expression, parent)) {
            concat([token("("), doc, token(")")])
        } else {
            doc
        };
        self.commented(span, doc)
    }

    fn function(&self, function: &Function<'_>) -> Result<Doc, FormatError> {
        let mut parts = Vec::new();
        if function.declare {
            parts.extend([token("declare"), space()]);
        }
        if function.r#async {
            parts.extend([token("async"), space()]);
        }
        parts.push(token("function"));
        if function.generator {
            parts.push(token("*"));
        }
        if let Some(id) = &function.id {
            parts.extend([space(), token(id.name.as_str())]);
        }
        if let Some(type_parameters) = &function.type_parameters {
            parts.push(self.syntax_doc(type_parameters.span)?);
        }
        parts.push(
            self.formal_parameters_with_this(function.this_param.as_deref(), &function.params)?,
        );
        if let Some(return_type) = &function.return_type {
            parts.push(self.syntax_doc(return_type.span)?);
        }
        if let Some(body) = &function.body {
            parts.extend([space(), self.function_body(body)?]);
        } else {
            parts.push(self.semicolon());
        }
        Ok(concat(parts))
    }

    fn function_body(&self, body: &FunctionBody<'_>) -> Result<Doc, FormatError> {
        self.block_contents(&body.directives, &body.statements, body.span)
    }

    fn formal_parameters(
        &self,
        parameters: &oxc_ast::ast::FormalParameters<'_>,
    ) -> Result<Doc, FormatError> {
        self.formal_parameters_with_this(None, parameters)
    }

    fn formal_parameters_with_this(
        &self,
        this_parameter: Option<&oxc_ast::ast::TSThisParameter<'_>>,
        parameters: &oxc_ast::ast::FormalParameters<'_>,
    ) -> Result<Doc, FormatError> {
        let mut docs = Vec::new();
        if let Some(this_parameter) = this_parameter {
            docs.push(self.node_doc(this_parameter.span, || self.syntax_doc(this_parameter.span))?);
        }
        for parameter in &parameters.items {
            docs.push(self.formal_parameter(parameter)?);
        }
        if let Some(rest) = &parameters.rest {
            docs.push(self.node_doc(rest.span, || {
                if !rest.decorators.is_empty() || rest.type_annotation.is_some() {
                    self.syntax_doc(rest.span)
                } else {
                    Ok(concat([
                        token("..."),
                        self.binding_pattern(&rest.rest.argument)?,
                    ]))
                }
            })?);
        }
        Ok(self.arguments_doc(docs, true))
    }

    fn formal_parameter(
        &self,
        parameter: &oxc_ast::ast::FormalParameter<'_>,
    ) -> Result<Doc, FormatError> {
        self.node_doc(parameter.span, || {
            if !parameter.decorators.is_empty()
                || parameter.type_annotation.is_some()
                || parameter.optional
                || parameter.accessibility.is_some()
                || parameter.readonly
                || parameter.r#override
            {
                return self.syntax_doc(parameter.span);
            }
            let mut doc = self.binding_pattern(&parameter.pattern)?;
            if let Some(initializer) = &parameter.initializer {
                doc = concat([
                    doc,
                    space(),
                    token("="),
                    space(),
                    self.assignment_expression(initializer, false)?,
                ]);
            }
            Ok(doc)
        })
    }

    fn arrow_function(
        &self,
        arrow: &oxc_ast::ast::ArrowFunctionExpression<'_>,
    ) -> Result<Doc, FormatError> {
        let mut parts = Vec::new();
        if arrow.r#async {
            parts.extend([token("async"), space()]);
        }
        if let Some(type_parameters) = &arrow.type_parameters {
            parts.push(self.syntax_doc(type_parameters.span)?);
        }
        let can_omit_parentheses = arrow.type_parameters.is_none()
            && arrow.return_type.is_none()
            && !formal_parameters_have_types(&arrow.params)
            && matches!(
                self.config.arrow_parentheses(),
                crate::ArrowParentheses::AsNeeded
            )
            && arrow.params.items.len() == 1
            && arrow.params.rest.is_none()
            && arrow.params.items[0].initializer.is_none()
            && matches!(
                arrow.params.items[0].pattern,
                BindingPattern::BindingIdentifier(_)
            );
        if can_omit_parentheses {
            parts.push(self.binding_pattern(&arrow.params.items[0].pattern)?);
        } else {
            parts.push(self.formal_parameters(&arrow.params)?);
        }
        if let Some(return_type) = &arrow.return_type {
            parts.push(self.syntax_doc(return_type.span)?);
        }
        parts.extend([space(), token("=>"), space()]);
        if let ArrowFunctionBody::FunctionBody(body) = &arrow.body {
            parts.push(self.function_body(body)?);
        } else {
            let expression = arrow.body.to_expression();
            let doc = self.assignment_expression(expression, false)?;
            parts.push(if matches!(expression, Expression::ObjectExpression(_)) {
                concat([token("("), doc, token(")")])
            } else {
                doc
            });
        }
        Ok(group(concat(parts)))
    }

    fn logical_expression(&self, logical: &LogicalExpression<'_>) -> Result<Doc, FormatError> {
        Ok(group(concat([
            self.expression_with_parent(
                &logical.left,
                false,
                Some(ParentContext::logical(
                    logical.operator,
                    ParentPosition::Left,
                )),
            )?,
            indent(concat([
                line_or_space(),
                token(logical.operator.as_str()),
                space(),
                self.expression_with_parent(
                    &logical.right,
                    false,
                    Some(ParentContext::logical(
                        logical.operator,
                        ParentPosition::Right,
                    )),
                )?,
            ])),
        ])))
    }

    fn template_literal(&self, literal: &TemplateLiteral<'_>) -> Result<Doc, FormatError> {
        let mut parts = vec![token("`")];
        for (index, quasi) in literal.quasis.iter().enumerate() {
            parts.push(text(quasi.value.raw.as_str()));
            if let Some(expression) = literal.expressions.get(index) {
                parts.extend([token("${"), self.expression(expression, false)?, token("}")]);
            }
        }
        parts.push(token("`"));
        Ok(concat(parts))
    }

    fn computed_member(&self, member: &ComputedMemberExpression<'_>) -> Result<Doc, FormatError> {
        Ok(group(concat([
            self.expression_with_parent(
                &member.object,
                false,
                Some(ParentContext::new(
                    Precedence::Member,
                    Associativity::Left,
                    ParentPosition::Object,
                )),
            )?,
            indent(concat([
                soft_line(),
                token(if member.optional { "?.[" } else { "[" }),
                self.expression(&member.expression, false)?,
                token("]"),
            ])),
        ])))
    }

    fn static_member(&self, member: &StaticMemberExpression<'_>) -> Result<Doc, FormatError> {
        Ok(group(concat([
            self.expression_with_parent(
                &member.object,
                false,
                Some(ParentContext::new(
                    Precedence::Member,
                    Associativity::Left,
                    ParentPosition::Object,
                )),
            )?,
            indent(concat([
                soft_line(),
                token(if member.optional { "?." } else { "." }),
                token(member.property.name.as_str()),
            ])),
        ])))
    }

    fn private_member(&self, member: &PrivateFieldExpression<'_>) -> Result<Doc, FormatError> {
        Ok(group(concat([
            self.expression_with_parent(
                &member.object,
                false,
                Some(ParentContext::new(
                    Precedence::Member,
                    Associativity::Left,
                    ParentPosition::Object,
                )),
            )?,
            indent(concat([
                soft_line(),
                token(if member.optional { "?.#" } else { ".#" }),
                token(member.field.name.as_str()),
            ])),
        ])))
    }

    fn call_expression(&self, call: &CallExpression<'_>) -> Result<Doc, FormatError> {
        let arguments = call
            .arguments
            .iter()
            .map(|argument| self.argument(argument))
            .collect::<Result<Vec<_>, _>>()?;
        let mut parts = vec![
            self.expression_with_parent(
                &call.callee,
                false,
                Some(ParentContext::new(
                    Precedence::Call,
                    Associativity::Left,
                    ParentPosition::Callee,
                )),
            )?,
            token(if call.optional { "?." } else { "" }),
        ];
        if let Some(type_arguments) = &call.type_arguments {
            parts.push(self.syntax_doc(type_arguments.span)?);
        }
        parts.push(self.arguments_doc(arguments, true));
        Ok(concat(parts))
    }

    fn new_expression(&self, new_expression: &NewExpression<'_>) -> Result<Doc, FormatError> {
        let arguments = new_expression
            .arguments
            .iter()
            .map(|argument| self.argument(argument))
            .collect::<Result<Vec<_>, _>>()?;
        let mut parts = vec![
            token("new"),
            space(),
            self.expression_with_parent(
                &new_expression.callee,
                false,
                Some(ParentContext::new(
                    Precedence::Member,
                    Associativity::Right,
                    ParentPosition::Callee,
                )),
            )?,
        ];
        if let Some(type_arguments) = &new_expression.type_arguments {
            parts.push(self.syntax_doc(type_arguments.span)?);
        }
        parts.push(self.arguments_doc(arguments, true));
        Ok(concat(parts))
    }

    fn argument(&self, argument: &Argument<'_>) -> Result<Doc, FormatError> {
        match argument {
            Argument::SpreadElement(spread) => self.node_doc(spread.span, || {
                Ok(concat([
                    token("..."),
                    self.assignment_expression(&spread.argument, false)?,
                ]))
            }),
            _ => self.assignment_expression(argument.to_expression(), false),
        }
    }

    fn arguments_doc(&self, arguments: Vec<Doc>, allow_trailing: bool) -> Doc {
        if arguments.is_empty() {
            return token("()");
        }
        let mut contents = join(arguments, concat([token(","), line_or_space()]));
        if allow_trailing && matches!(self.config.trailing_commas(), TrailingCommas::All) {
            contents = concat([contents, token(",")]);
        }
        group(concat([token("("), surround(contents, false), token(")")]))
    }

    fn chain_element(&self, chain: &ChainElement<'_>) -> Result<Doc, FormatError> {
        match chain {
            ChainElement::CallExpression(call) => self.call_expression(call),
            ChainElement::ComputedMemberExpression(member) => self.computed_member(member),
            ChainElement::StaticMemberExpression(member) => self.static_member(member),
            ChainElement::PrivateFieldExpression(member) => self.private_member(member),
            ChainElement::TSNonNullExpression(non_null) => Ok(concat([
                self.expression(&non_null.expression, false)?,
                token("!"),
            ])),
        }
    }

    fn assignment_target(&self, target: &AssignmentTarget<'_>) -> Result<Doc, FormatError> {
        self.node_doc(target.span(), || self.assignment_target_inner(target))
    }

    fn assignment_target_inner(&self, target: &AssignmentTarget<'_>) -> Result<Doc, FormatError> {
        match target {
            AssignmentTarget::AssignmentTargetIdentifier(identifier) => {
                Ok(token(identifier.name.as_str()))
            }
            AssignmentTarget::ComputedMemberExpression(member) => self.computed_member(member),
            AssignmentTarget::StaticMemberExpression(member) => self.static_member(member),
            AssignmentTarget::PrivateFieldExpression(member) => self.private_member(member),
            AssignmentTarget::ArrayAssignmentTarget(array) => {
                let mut elements = Vec::new();
                for element in &array.elements {
                    elements.push(match element {
                        Some(element) => self.assignment_target_maybe_default(element)?,
                        None => empty(),
                    });
                }
                if let Some(rest) = &array.rest {
                    elements.push(self.node_doc(rest.span, || {
                        Ok(concat([
                            token("..."),
                            self.assignment_target(&rest.target)?,
                        ]))
                    })?);
                }
                Ok(concat([
                    token("["),
                    join(elements, concat([token(","), space()])),
                    token("]"),
                ]))
            }
            AssignmentTarget::ObjectAssignmentTarget(object) => {
                let mut properties = Vec::new();
                for property in &object.properties {
                    properties.push(self.node_doc(property.span(), || {
                        Ok(match property {
                            oxc_ast::ast::AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(
                                property,
                            ) => {
                                let mut parts = vec![token(property.binding.name.as_str())];
                                if let Some(init) = &property.init {
                                    parts.extend([
                                        space(),
                                        token("="),
                                        space(),
                                        self.assignment_expression(init, false)?,
                                    ]);
                                }
                                concat(parts)
                            }
                            oxc_ast::ast::AssignmentTargetProperty::AssignmentTargetPropertyProperty(
                                property,
                            ) => concat([
                                self.property_key(&property.name, property.computed)?,
                                token(":"),
                                space(),
                                self.assignment_target_maybe_default(&property.binding)?,
                            ]),
                        })
                    })?);
                }
                if let Some(rest) = &object.rest {
                    properties.push(self.node_doc(rest.span, || {
                        Ok(concat([
                            token("..."),
                            self.assignment_target(&rest.target)?,
                        ]))
                    })?);
                }
                Ok(concat([
                    token("{"),
                    surround(
                        join(properties, concat([token(","), space()])),
                        self.config.bracket_spacing(),
                    ),
                    token("}"),
                ]))
            }
            AssignmentTarget::TSAsExpression(expression) => self.syntax_doc(expression.span),
            AssignmentTarget::TSSatisfiesExpression(expression) => self.syntax_doc(expression.span),
            AssignmentTarget::TSNonNullExpression(expression) => self.syntax_doc(expression.span),
            AssignmentTarget::TSTypeAssertion(expression) => self.syntax_doc(expression.span),
        }
    }

    fn assignment_target_maybe_default(
        &self,
        target: &oxc_ast::ast::AssignmentTargetMaybeDefault<'_>,
    ) -> Result<Doc, FormatError> {
        self.node_doc(target.span(), || {
            self.assignment_target_maybe_default_inner(target)
        })
    }

    fn assignment_target_maybe_default_inner(
        &self,
        target: &oxc_ast::ast::AssignmentTargetMaybeDefault<'_>,
    ) -> Result<Doc, FormatError> {
        match target {
            oxc_ast::ast::AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(default) => {
                Ok(concat([
                    self.assignment_target(&default.binding)?,
                    space(),
                    token("="),
                    space(),
                    self.assignment_expression(&default.init, false)?,
                ]))
            }
            target => self.assignment_target(target.to_assignment_target()),
        }
    }

    fn simple_assignment_target(
        &self,
        target: &SimpleAssignmentTarget<'_>,
    ) -> Result<Doc, FormatError> {
        self.node_doc(target.span(), || {
            self.simple_assignment_target_inner(target)
        })
    }

    fn simple_assignment_target_inner(
        &self,
        target: &SimpleAssignmentTarget<'_>,
    ) -> Result<Doc, FormatError> {
        match target {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) => {
                Ok(token(identifier.name.as_str()))
            }
            SimpleAssignmentTarget::ComputedMemberExpression(member) => {
                self.computed_member(member)
            }
            SimpleAssignmentTarget::StaticMemberExpression(member) => self.static_member(member),
            SimpleAssignmentTarget::PrivateFieldExpression(member) => self.private_member(member),
            SimpleAssignmentTarget::TSAsExpression(expression) => self.syntax_doc(expression.span),
            SimpleAssignmentTarget::TSSatisfiesExpression(expression) => {
                self.syntax_doc(expression.span)
            }
            SimpleAssignmentTarget::TSNonNullExpression(expression) => {
                self.syntax_doc(expression.span)
            }
            SimpleAssignmentTarget::TSTypeAssertion(expression) => self.syntax_doc(expression.span),
        }
    }

    fn array_expression(&self, array: &ArrayExpression<'_>) -> Result<Doc, FormatError> {
        let dangling = self.dangling(array.span)?;
        if array.elements.is_empty() && dangling.is_empty() {
            return Ok(token("[]"));
        }

        let item_docs = array
            .elements
            .iter()
            .map(|element| match element {
                ArrayExpressionElement::SpreadElement(spread) => self.node_doc(spread.span, || {
                    Ok(concat([
                        token("..."),
                        self.assignment_expression(&spread.argument, true)?,
                    ]))
                }),
                ArrayExpressionElement::Elision(_) => Ok(empty()),
                _ => self.assignment_expression(element.to_expression(), true),
            })
            .collect::<Result<Vec<_>, FormatError>>()?;
        let force_multiline = !dangling.is_empty()
            || match self.config.arrays().layout {
                CollectionLayout::MultiLine => true,
                CollectionLayout::Preserve => self.slice(array.span)?.contains(['\n', '\r']),
                CollectionLayout::Auto | CollectionLayout::SingleLine => false,
            }
            || matches!(
                self.config.arrays().element_layout,
                CollectionItemLayout::OnePerLine
            )
            || (matches!(
                self.config.arrays().object_elements,
                ArrayObjectLayout::OnePerLine
            ) && array
                .elements
                .iter()
                .any(|element| matches!(element, ArrayExpressionElement::ObjectExpression(_))))
            || item_docs.iter().any(forces_line_break);
        let separator_line = if force_multiline {
            hard_line()
        } else {
            line_or_space()
        };
        let mut elements = comments_with_lines(dangling);
        if !elements.is_empty() && !array.elements.is_empty() {
            elements.push(hard_line());
        }

        for (index, (element, doc)) in array.elements.iter().zip(item_docs).enumerate() {
            elements.push(doc);

            let is_last = index + 1 == array.elements.len();
            if !is_last || element.is_elision() {
                elements.push(token(","));
            }
            if !is_last {
                elements.push(separator_line.clone());
            }
        }

        let trailing = matches!(self.config.trailing_commas(), TrailingCommas::All)
            || (force_multiline
                && matches!(self.config.trailing_commas(), TrailingCommas::Multiline));
        if trailing
            && !array
                .elements
                .last()
                .is_some_and(ArrayExpressionElement::is_elision)
        {
            elements.push(token(","));
        }

        let contents = concat(elements);
        let doc = group(if force_multiline {
            concat([
                token("["),
                indent(concat([hard_line(), contents])),
                hard_line(),
                token("]"),
            ])
        } else {
            concat([token("["), surround(contents, false), token("]")])
        });
        Ok(
            if matches!(self.config.arrays().layout, CollectionLayout::SingleLine) {
                force_flat(doc)
            } else {
                doc
            },
        )
    }

    fn object_expression(
        &self,
        object: &ObjectExpression<'_>,
        in_array: bool,
    ) -> Result<Doc, FormatError> {
        let dangling = self.dangling(object.span)?;
        if object.properties.is_empty() && dangling.is_empty() {
            return Ok(token("{}"));
        }

        let property_docs = object
            .properties
            .iter()
            .map(|property| self.object_property(property))
            .collect::<Result<Vec<_>, FormatError>>()?;
        let force_multiline = !dangling.is_empty()
            || match self.config.objects().layout {
                CollectionLayout::MultiLine => true,
                CollectionLayout::Preserve => self.slice(object.span)?.contains(['\n', '\r']),
                CollectionLayout::Auto | CollectionLayout::SingleLine => false,
            }
            || matches!(
                self.config.objects().property_layout,
                CollectionItemLayout::OnePerLine
            )
            || (in_array
                && matches!(
                    self.config.objects().when_array_element,
                    ObjectArrayLayout::MultiLine
                ))
            || property_docs.iter().any(forces_line_break);
        let separator_line = if force_multiline {
            hard_line()
        } else {
            line_or_space()
        };
        let mut properties = comments_with_lines(dangling);
        if !properties.is_empty() && !object.properties.is_empty() {
            properties.push(hard_line());
        }

        for (index, property) in property_docs.into_iter().enumerate() {
            properties.push(property);
            let is_last = index + 1 == object.properties.len();
            if !is_last {
                properties.extend([token(","), separator_line.clone()]);
            }
        }

        let trailing = matches!(self.config.trailing_commas(), TrailingCommas::All)
            || (force_multiline
                && matches!(self.config.trailing_commas(), TrailingCommas::Multiline));
        if trailing {
            properties.push(token(","));
        }

        let contents = concat(properties);
        let doc = group(if force_multiline {
            concat([
                token("{"),
                indent(concat([hard_line(), contents])),
                hard_line(),
                token("}"),
            ])
        } else {
            concat([
                token("{"),
                surround(contents, self.config.bracket_spacing()),
                token("}"),
            ])
        });
        Ok(
            if matches!(self.config.objects().layout, CollectionLayout::SingleLine) {
                force_flat(doc)
            } else {
                doc
            },
        )
    }

    fn object_property(&self, property: &ObjectPropertyKind<'_>) -> Result<Doc, FormatError> {
        let span = property.span();
        if let Some((directive, raw)) = self.suppressed(span)? {
            return self.commented(span, concat([directive, raw]));
        }
        let doc = match property {
            ObjectPropertyKind::SpreadProperty(spread) => Ok(concat([
                token("..."),
                self.assignment_expression(&spread.argument, false)?,
            ])),
            ObjectPropertyKind::ObjectProperty(property) => self.regular_object_property(property),
        }?;
        self.commented(span, doc)
    }

    fn regular_object_property(&self, property: &ObjectProperty<'_>) -> Result<Doc, FormatError> {
        if property.method || !matches!(property.kind, oxc_ast::ast::PropertyKind::Init) {
            let Expression::FunctionExpression(function) = &property.value else {
                return Err(FormatError::internal(
                    "object method did not contain a function expression",
                ));
            };
            if function_has_types(function) {
                return self.syntax_doc(property.span);
            }
            return self.method(
                &property.key,
                property.computed,
                false,
                property.kind,
                function,
            );
        }
        let key = self.property_key(&property.key, property.computed)?;
        if property.shorthand {
            return Ok(key);
        }
        Ok(concat([
            key,
            token(":"),
            space(),
            self.assignment_expression(&property.value, false)?,
        ]))
    }

    fn property_key(&self, key: &PropertyKey<'_>, computed: bool) -> Result<Doc, FormatError> {
        let key = match key {
            PropertyKey::StaticIdentifier(identifier) => token(identifier.name.as_str()),
            PropertyKey::PrivateIdentifier(identifier) => token(format!("#{}", identifier.name)),
            _ => self.expression(key.to_expression(), false)?,
        };
        Ok(if computed {
            concat([token("["), key, token("]")])
        } else {
            key
        })
    }

    fn binding_pattern(&self, pattern: &BindingPattern<'_>) -> Result<Doc, FormatError> {
        self.node_doc(pattern.span(), || self.binding_pattern_inner(pattern))
    }

    fn binding_pattern_inner(&self, pattern: &BindingPattern<'_>) -> Result<Doc, FormatError> {
        match pattern {
            BindingPattern::BindingIdentifier(identifier) => Ok(token(identifier.name.as_str())),
            BindingPattern::AssignmentPattern(assignment) => Ok(concat([
                self.binding_pattern(&assignment.left)?,
                space(),
                token("="),
                space(),
                self.assignment_expression(&assignment.right, false)?,
            ])),
            BindingPattern::ObjectPattern(object) => {
                let mut properties = Vec::new();
                for property in &object.properties {
                    properties.push(self.node_doc(property.span, || {
                        let key = self.property_key(&property.key, property.computed)?;
                        Ok(if property.shorthand {
                            self.binding_pattern(&property.value)?
                        } else {
                            concat([
                                key,
                                token(":"),
                                space(),
                                self.binding_pattern(&property.value)?,
                            ])
                        })
                    })?);
                }
                if let Some(rest) = &object.rest {
                    properties.push(self.node_doc(rest.span, || {
                        Ok(concat([
                            token("..."),
                            self.binding_pattern(&rest.argument)?,
                        ]))
                    })?);
                }
                if properties.is_empty() {
                    Ok(token("{}"))
                } else {
                    Ok(group(concat([
                        token("{"),
                        surround(
                            join(properties, concat([token(","), line_or_space()])),
                            self.config.bracket_spacing(),
                        ),
                        token("}"),
                    ])))
                }
            }
            BindingPattern::ArrayPattern(array) => {
                let mut elements = Vec::new();
                for element in &array.elements {
                    if let Some(element) = element {
                        elements.push(self.binding_pattern(element)?);
                    } else {
                        elements.push(empty());
                    }
                }
                if let Some(rest) = &array.rest {
                    elements.push(self.node_doc(rest.span, || {
                        Ok(concat([
                            token("..."),
                            self.binding_pattern(&rest.argument)?,
                        ]))
                    })?);
                }
                if elements.is_empty() {
                    Ok(token("[]"))
                } else {
                    Ok(group(concat([
                        token("["),
                        surround(join(elements, concat([token(","), line_or_space()])), false),
                        token("]"),
                    ])))
                }
            }
        }
    }

    fn class(&self, class: &oxc_ast::ast::Class<'_>) -> Result<Doc, FormatError> {
        if class.type_parameters.is_some()
            || !class.implements.is_empty()
            || class.r#abstract
            || class.declare
            || class
                .heritage
                .as_ref()
                .is_some_and(|heritage| heritage.type_arguments.is_some())
            || class.body.body.iter().any(class_element_has_types)
        {
            return self.syntax_doc(class.span);
        }
        let mut parts = self.decorators(&class.decorators)?;
        parts.push(token("class"));
        if let Some(id) = &class.id {
            parts.extend([space(), token(id.name.as_str())]);
        }
        if let Some(heritage) = &class.heritage {
            parts.extend([
                space(),
                token("extends"),
                space(),
                self.expression_with_parent(
                    &heritage.expression,
                    false,
                    Some(ParentContext::new(
                        Precedence::Member,
                        Associativity::Right,
                        ParentPosition::Right,
                    )),
                )?,
            ]);
        }
        parts.extend([space(), self.class_body(&class.body)?]);
        Ok(concat(parts))
    }

    #[allow(
        clippy::too_many_lines,
        reason = "exhaustive class element dispatch stays together so new Oxc variants fail compilation"
    )]
    fn class_body(&self, body: &oxc_ast::ast::ClassBody<'_>) -> Result<Doc, FormatError> {
        let dangling = self.dangling(body.span)?;
        if body.body.is_empty() && dangling.is_empty() {
            return Ok(token("{}"));
        }
        let mut elements = comments_with_lines(dangling);
        if !elements.is_empty() && !body.body.is_empty() {
            elements.push(hard_line());
        }
        for (index, element) in body.body.iter().enumerate() {
            let span = element.span();
            let doc = if let Some((directive, raw)) = self.suppressed(span)? {
                self.commented(span, concat([directive, raw]))?
            } else {
                let doc = match element {
                    oxc_ast::ast::ClassElement::StaticBlock(block) => {
                        let block_doc = self.block_contents(&[], &block.body, block.span)?;
                        concat([token("static"), space(), block_doc])
                    }
                    oxc_ast::ast::ClassElement::MethodDefinition(method) => {
                        if method.r#type != oxc_ast::ast::MethodDefinitionType::MethodDefinition
                            || method.r#override
                            || method.optional
                            || method.accessibility.is_some()
                        {
                            self.syntax_doc(method.span)?
                        } else {
                            let mut docs = self.decorators(&method.decorators)?;
                            docs.push(self.method(
                                &method.key,
                                method.computed,
                                method.r#static,
                                match method.kind {
                                    oxc_ast::ast::MethodDefinitionKind::Get => {
                                        oxc_ast::ast::PropertyKind::Get
                                    }
                                    oxc_ast::ast::MethodDefinitionKind::Set => {
                                        oxc_ast::ast::PropertyKind::Set
                                    }
                                    oxc_ast::ast::MethodDefinitionKind::Constructor
                                    | oxc_ast::ast::MethodDefinitionKind::Method => {
                                        oxc_ast::ast::PropertyKind::Init
                                    }
                                },
                                &method.value,
                            )?);
                            concat(docs)
                        }
                    }
                    oxc_ast::ast::ClassElement::PropertyDefinition(property) => {
                        if property.r#type
                            != oxc_ast::ast::PropertyDefinitionType::PropertyDefinition
                            || property.type_annotation.is_some()
                            || property.declare
                            || property.r#override
                            || property.optional
                            || property.definite
                            || property.readonly
                            || property.accessibility.is_some()
                        {
                            self.syntax_doc(property.span)?
                        } else {
                            let mut docs = self.decorators(&property.decorators)?;
                            if property.r#static {
                                docs.extend([token("static"), space()]);
                            }
                            docs.push(self.property_key(&property.key, property.computed)?);
                            if let Some(value) = &property.value {
                                docs.extend([
                                    space(),
                                    token("="),
                                    space(),
                                    self.assignment_expression(value, false)?,
                                ]);
                            }
                            docs.push(self.semicolon());
                            concat(docs)
                        }
                    }
                    oxc_ast::ast::ClassElement::AccessorProperty(accessor) => {
                        if accessor.r#type != oxc_ast::ast::AccessorPropertyType::AccessorProperty
                            || accessor.type_annotation.is_some()
                            || accessor.r#override
                            || accessor.definite
                            || accessor.accessibility.is_some()
                        {
                            self.syntax_doc(accessor.span)?
                        } else {
                            let mut docs = self.decorators(&accessor.decorators)?;
                            if accessor.r#static {
                                docs.extend([token("static"), space()]);
                            }
                            docs.extend([
                                token("accessor"),
                                space(),
                                self.property_key(&accessor.key, accessor.computed)?,
                            ]);
                            if let Some(value) = &accessor.value {
                                docs.extend([
                                    space(),
                                    token("="),
                                    space(),
                                    self.assignment_expression(value, false)?,
                                ]);
                            }
                            docs.push(self.semicolon());
                            concat(docs)
                        }
                    }
                    oxc_ast::ast::ClassElement::TSIndexSignature(signature) => {
                        self.syntax_doc(signature.span)?
                    }
                };
                self.commented(span, doc)?
            };
            elements.push(doc);
            if index + 1 != body.body.len() {
                elements.push(hard_line());
            }
        }
        Ok(concat([
            token("{"),
            indent(concat([hard_line(), concat(elements)])),
            hard_line(),
            token("}"),
        ]))
    }

    fn decorators(
        &self,
        decorators: &[oxc_ast::ast::Decorator<'_>],
    ) -> Result<Vec<Doc>, FormatError> {
        let mut docs = Vec::new();
        for decorator in decorators {
            docs.extend([
                token("@"),
                self.assignment_expression(&decorator.expression, false)?,
                hard_line(),
            ]);
        }
        Ok(docs)
    }

    fn method(
        &self,
        key: &PropertyKey<'_>,
        computed: bool,
        is_static: bool,
        kind: oxc_ast::ast::PropertyKind,
        function: &Function<'_>,
    ) -> Result<Doc, FormatError> {
        let mut parts = Vec::new();
        if is_static {
            parts.extend([token("static"), space()]);
        }
        if function.r#async {
            parts.extend([token("async"), space()]);
        }
        match kind {
            oxc_ast::ast::PropertyKind::Get => parts.extend([token("get"), space()]),
            oxc_ast::ast::PropertyKind::Set => parts.extend([token("set"), space()]),
            oxc_ast::ast::PropertyKind::Init => {}
        }
        if function.generator {
            parts.push(token("*"));
        }
        parts.push(self.property_key(key, computed)?);
        parts.push(self.formal_parameters(&function.params)?);
        let Some(body) = &function.body else {
            return Err(FormatError::internal("method did not contain a body"));
        };
        parts.extend([space(), self.function_body(body)?]);
        Ok(concat(parts))
    }

    fn import_declaration(&self, declaration: &ImportDeclaration<'_>) -> Result<Doc, FormatError> {
        let mut parts = vec![token("import")];
        if let Some(phase) = declaration.phase {
            parts.extend([
                space(),
                token(match phase {
                    oxc_ast::ast::ImportPhase::Source => "source",
                    oxc_ast::ast::ImportPhase::Defer => "defer",
                }),
            ]);
        }
        if declaration.import_kind == ImportOrExportKind::Type {
            parts.extend([space(), token("type")]);
        }

        let Some(specifiers) = &declaration.specifiers else {
            parts.extend([
                space(),
                token(self.string_literal(&declaration.source)?),
                self.with_clause(declaration.with_clause.as_deref())?,
                self.semicolon(),
            ]);
            return Ok(concat(parts));
        };

        let mut prefix = Vec::new();
        let mut named = Vec::new();
        for specifier in specifiers {
            match specifier {
                ImportDeclarationSpecifier::ImportDefaultSpecifier(specifier) => {
                    prefix.push(
                        self.node_doc(specifier.span, || Ok(token(specifier.local.name.as_str())))?,
                    );
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier) => {
                    prefix.push(self.node_doc(specifier.span, || {
                        Ok(concat([
                            token("*"),
                            space(),
                            token("as"),
                            space(),
                            token(specifier.local.name.as_str()),
                        ]))
                    })?);
                }
                ImportDeclarationSpecifier::ImportSpecifier(specifier) => {
                    named.push(self.node_doc(specifier.span, || {
                        let mut item = Vec::new();
                        if specifier.import_kind == ImportOrExportKind::Type {
                            item.extend([token("type"), space()]);
                        }
                        item.push(self.module_export_name(&specifier.imported)?);
                        let imported = module_export_name_text(&specifier.imported);
                        if imported.as_deref() != Some(specifier.local.name.as_str()) {
                            item.extend([
                                space(),
                                token("as"),
                                space(),
                                token(specifier.local.name.as_str()),
                            ]);
                        }
                        Ok(concat(item))
                    })?);
                }
            }
        }

        let mut clauses = prefix;
        if !named.is_empty() || specifiers.is_empty() {
            let force_multiline = matches!(
                self.config.imports().specifier_layout,
                CollectionItemLayout::OnePerLine
            ) || (matches!(
                self.config.imports().specifier_layout,
                CollectionItemLayout::Preserve
            ) && self.slice(declaration.span)?.contains(['\n', '\r']));
            let separator = if force_multiline {
                hard_line()
            } else {
                line_or_space()
            };
            let contents = join(named, concat([token(","), separator]));
            clauses.push(group(if force_multiline {
                concat([
                    token("{"),
                    indent(concat([hard_line(), contents])),
                    hard_line(),
                    token("}"),
                ])
            } else {
                concat([token("{"), surround(contents, true), token("}")])
            }));
        }

        parts.extend([
            space(),
            join(clauses, concat([token(","), space()])),
            space(),
            token("from"),
            space(),
            token(self.string_literal(&declaration.source)?),
            self.with_clause(declaration.with_clause.as_deref())?,
            self.semicolon(),
        ]);
        Ok(group(concat(parts)))
    }

    fn export_named(
        &self,
        declaration: &oxc_ast::ast::ExportNamedDeclaration<'_>,
    ) -> Result<Doc, FormatError> {
        let mut parts = vec![token("export")];
        if declaration.export_kind == ImportOrExportKind::Type {
            parts.extend([space(), token("type")]);
        }
        parts.extend([
            space(),
            self.export_specifiers(&declaration.specifiers)?,
            self.semicolon(),
        ]);
        Ok(concat(parts))
    }

    fn export_from(
        &self,
        declaration: &oxc_ast::ast::ExportFromDeclaration<'_>,
    ) -> Result<Doc, FormatError> {
        let mut parts = vec![token("export")];
        if declaration.export_kind == ImportOrExportKind::Type {
            parts.extend([space(), token("type")]);
        }
        parts.extend([
            space(),
            self.export_specifiers(&declaration.specifiers)?,
            space(),
            token("from"),
            space(),
            token(self.string_literal(&declaration.source)?),
            self.with_clause(declaration.with_clause.as_deref())?,
            self.semicolon(),
        ]);
        Ok(concat(parts))
    }

    fn export_all(
        &self,
        declaration: &oxc_ast::ast::ExportAllDeclaration<'_>,
    ) -> Result<Doc, FormatError> {
        let mut parts = vec![token("export")];
        if declaration.export_kind == ImportOrExportKind::Type {
            parts.extend([space(), token("type")]);
        }
        parts.extend([space(), token("*")]);
        if let Some(exported) = &declaration.exported {
            parts.extend([
                space(),
                token("as"),
                space(),
                self.module_export_name(exported)?,
            ]);
        }
        parts.extend([
            space(),
            token("from"),
            space(),
            token(self.string_literal(&declaration.source)?),
            self.with_clause(declaration.with_clause.as_deref())?,
            self.semicolon(),
        ]);
        Ok(concat(parts))
    }

    fn export_default(
        &self,
        declaration: &oxc_ast::ast::ExportDefaultDeclaration<'_>,
    ) -> Result<Doc, FormatError> {
        let doc = match &declaration.declaration {
            oxc_ast::ast::ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                self.function(function)?
            }
            oxc_ast::ast::ExportDefaultDeclarationKind::ClassDeclaration(class) => {
                self.class(class)?
            }
            oxc_ast::ast::ExportDefaultDeclarationKind::TSInterfaceDeclaration(interface) => {
                self.syntax_doc(interface.span)?
            }
            expression => concat([
                self.assignment_expression(expression.to_expression(), false)?,
                self.semicolon(),
            ]),
        };
        Ok(concat([
            token("export"),
            space(),
            token("default"),
            space(),
            doc,
        ]))
    }

    fn export_specifiers(
        &self,
        specifiers: &[oxc_ast::ast::ExportSpecifier<'_>],
    ) -> Result<Doc, FormatError> {
        let mut docs = Vec::new();
        for specifier in specifiers {
            docs.push(self.node_doc(specifier.span, || {
                let mut parts = Vec::new();
                if specifier.export_kind == ImportOrExportKind::Type {
                    parts.extend([token("type"), space()]);
                }
                parts.push(self.module_export_name(&specifier.local)?);
                if module_export_name_text(&specifier.local)
                    != module_export_name_text(&specifier.exported)
                {
                    parts.extend([
                        space(),
                        token("as"),
                        space(),
                        self.module_export_name(&specifier.exported)?,
                    ]);
                }
                Ok(concat(parts))
            })?);
        }
        if docs.is_empty() {
            Ok(token("{}"))
        } else {
            Ok(group(concat([
                token("{"),
                surround(join(docs, concat([token(","), line_or_space()])), true),
                token("}"),
            ])))
        }
    }

    fn with_clause(
        &self,
        clause: Option<&oxc_ast::ast::WithClause<'_>>,
    ) -> Result<Doc, FormatError> {
        let Some(clause) = clause else {
            return Ok(empty());
        };
        let entries = clause
            .with_entries
            .iter()
            .map(|entry| {
                self.node_doc(entry.span, || {
                    let key = match &entry.key {
                        oxc_ast::ast::ImportAttributeKey::Identifier(identifier) => {
                            token(identifier.name.as_str())
                        }
                        oxc_ast::ast::ImportAttributeKey::StringLiteral(literal) => {
                            token(self.string_literal(literal)?)
                        }
                    };
                    Ok(concat([
                        key,
                        token(":"),
                        space(),
                        token(self.string_literal(&entry.value)?),
                    ]))
                })
            })
            .collect::<Result<Vec<_>, FormatError>>()?;
        Ok(concat([
            space(),
            token(match clause.keyword {
                oxc_ast::ast::WithClauseKeyword::With => "with",
                oxc_ast::ast::WithClauseKeyword::Assert => "assert",
            }),
            space(),
            token("{"),
            surround(join(entries, concat([token(","), line_or_space()])), true),
            token("}"),
        ]))
    }

    fn module_export_name(&self, name: &ModuleExportName<'_>) -> Result<Doc, FormatError> {
        match name {
            ModuleExportName::IdentifierName(identifier) => Ok(token(identifier.name.as_str())),
            ModuleExportName::IdentifierReference(identifier) => {
                Ok(token(identifier.name.as_str()))
            }
            ModuleExportName::StringLiteral(literal) => Ok(token(self.string_literal(literal)?)),
        }
    }

    fn string_literal(&self, literal: &StringLiteral<'_>) -> Result<String, FormatError> {
        if literal.lone_surrogates {
            return self.slice(literal.span).map(ToOwned::to_owned);
        }
        let json = serde_json::to_string(literal.value.as_str()).map_err(|error| {
            FormatError::internal(format!("failed to encode string literal: {error}"))
        })?;
        if matches!(self.config.quote_style(), QuoteStyle::Double) {
            return Ok(json);
        }

        let inner = &json[1..json.len() - 1];
        let single = inner.replace("\\\"", "\"").replace('\'', "\\'");
        Ok(format!("'{single}'"))
    }

    fn semicolon(&self) -> Doc {
        if matches!(self.config.semicolons(), Semicolons::Always) {
            token(";")
        } else {
            empty()
        }
    }

    fn statement_spacing_conditions(
        &self,
        previous: StatementKind,
        next: StatementKind,
        scope: StatementScope,
    ) -> Vec<StatementSpacingCondition> {
        self.config
            .statement_spacing()
            .iter()
            .filter(|rule| {
                (matches!(rule.scope, StatementScope::Any)
                    || matches!(
                        (rule.scope, scope),
                        (StatementScope::TopLevel, StatementScope::TopLevel)
                            | (StatementScope::Block, StatementScope::Block)
                    ))
                    && selector_kind_matches(&rule.previous, previous)
                    && selector_kind_matches(&rule.next, next)
            })
            .map(|rule| StatementSpacingCondition {
                previous_shape: rule.previous.line_shape,
                next_shape: rule.next.line_shape,
                blank_lines: rule.blank_lines,
            })
            .collect()
    }

    fn slice(&self, span: Span) -> Result<&'a str, FormatError> {
        self.source
            .get(span.start as usize..span.end as usize)
            .ok_or_else(|| {
                FormatError::internal(format!("invalid source span {}..{}", span.start, span.end))
            })
    }

    fn suppressed(&self, span: Span) -> Result<Option<(Doc, Doc)>, FormatError> {
        self.comments
            .borrow_mut()
            .suppression(span, &self.node_index)
    }

    fn node_doc(
        &self,
        span: Span,
        format: impl FnOnce() -> Result<Doc, FormatError>,
    ) -> Result<Doc, FormatError> {
        if let Some((directive, raw)) = self.suppressed(span)? {
            return self.commented(span, concat([directive, raw]));
        }
        self.commented(span, format()?)
    }

    fn commented(&self, span: Span, doc: Doc) -> Result<Doc, FormatError> {
        self.comments.borrow_mut().wrap(span, doc, &self.node_index)
    }

    fn dangling(&self, span: Span) -> Result<Vec<Doc>, FormatError> {
        self.comments.borrow_mut().dangling(span, &self.node_index)
    }

    fn finish_comments(&self) -> Result<(), FormatError> {
        self.comments.borrow().finish()
    }

    fn syntax_doc(&self, span: Span) -> Result<Doc, FormatError> {
        let mut lexemes = self
            .tokens
            .iter()
            .filter(|token| {
                token.start() >= span.start
                    && token.end() <= span.end
                    && token.end() > token.start()
            })
            .map(|token| {
                Ok(SyntaxLexeme {
                    span: token.span(),
                    text: self.syntax_token_text(token)?,
                    line_comment: false,
                })
            })
            .collect::<Result<Vec<_>, FormatError>>()?;
        for (comment_span, raw, line_comment) in self.comments.borrow_mut().take_within(span)? {
            lexemes.push(SyntaxLexeme {
                span: comment_span,
                text: raw,
                line_comment,
            });
        }
        lexemes.sort_by_key(|lexeme| (lexeme.span.start, lexeme.span.end));
        let mut index = 0;
        Ok(group(syntax_sequence(&lexemes, &mut index, None)))
    }

    fn syntax_token_text(&self, source_token: &Token) -> Result<String, FormatError> {
        let raw = self.slice(source_token.span())?;
        if source_token.kind() != Kind::Str {
            return Ok(raw.to_owned());
        }
        Ok(requote_raw_string(raw, self.config.quote_style()))
    }

    fn type_doc(&self, r#type: &TSType<'_>) -> Result<Doc, FormatError> {
        match r#type {
            TSType::TSAnyKeyword(_)
            | TSType::TSBigIntKeyword(_)
            | TSType::TSBooleanKeyword(_)
            | TSType::TSIntrinsicKeyword(_)
            | TSType::TSNeverKeyword(_)
            | TSType::TSNullKeyword(_)
            | TSType::TSNumberKeyword(_)
            | TSType::TSObjectKeyword(_)
            | TSType::TSStringKeyword(_)
            | TSType::TSSymbolKeyword(_)
            | TSType::TSUndefinedKeyword(_)
            | TSType::TSUnknownKeyword(_)
            | TSType::TSVoidKeyword(_)
            | TSType::TSArrayType(_)
            | TSType::TSConditionalType(_)
            | TSType::TSConstructorType(_)
            | TSType::TSFunctionType(_)
            | TSType::TSImportType(_)
            | TSType::TSIndexedAccessType(_)
            | TSType::TSInferType(_)
            | TSType::TSIntersectionType(_)
            | TSType::TSLiteralType(_)
            | TSType::TSMappedType(_)
            | TSType::TSNamedTupleMember(_)
            | TSType::TSTemplateLiteralType(_)
            | TSType::TSThisType(_)
            | TSType::TSTupleType(_)
            | TSType::TSTypeLiteral(_)
            | TSType::TSTypeOperatorType(_)
            | TSType::TSTypePredicate(_)
            | TSType::TSTypeQuery(_)
            | TSType::TSTypeReference(_)
            | TSType::TSUnionType(_)
            | TSType::TSParenthesizedType(_)
            | TSType::JSDocNullableType(_)
            | TSType::JSDocNonNullableType(_)
            | TSType::JSDocUnknownType(_) => self.syntax_doc(r#type.span()),
        }
    }

    fn jsx_element(&self, element: &oxc_ast::ast::JSXElement<'_>) -> Result<Doc, FormatError> {
        let mut parts =
            vec![self.jsx_opening_element(
                &element.opening_element,
                element.closing_element.is_none(),
            )?];
        for child in &element.children {
            parts.push(self.jsx_child(child)?);
        }
        if let Some(closing) = &element.closing_element {
            parts.extend([
                token("</"),
                Self::jsx_element_name(&closing.name),
                token(">"),
            ]);
        }
        Ok(concat(parts))
    }

    fn jsx_fragment(&self, fragment: &oxc_ast::ast::JSXFragment<'_>) -> Result<Doc, FormatError> {
        let mut parts = vec![token("<>")];
        for child in &fragment.children {
            parts.push(self.jsx_child(child)?);
        }
        parts.push(token("</>"));
        Ok(concat(parts))
    }

    fn jsx_opening_element(
        &self,
        element: &oxc_ast::ast::JSXOpeningElement<'_>,
        self_closing: bool,
    ) -> Result<Doc, FormatError> {
        let mut head = vec![token("<"), Self::jsx_element_name(&element.name)];
        if let Some(type_arguments) = &element.type_arguments {
            head.push(self.syntax_doc(type_arguments.span)?);
        }
        let mut attributes = Vec::new();
        for attribute in &element.attributes {
            attributes.push(self.node_doc(attribute.span(), || {
                Ok(match attribute {
                    oxc_ast::ast::JSXAttributeItem::Attribute(attribute) => {
                        let mut parts = vec![Self::jsx_attribute_name(&attribute.name)];
                        if let Some(value) = &attribute.value {
                            parts.extend([token("="), self.jsx_attribute_value(value)?]);
                        }
                        concat(parts)
                    }
                    oxc_ast::ast::JSXAttributeItem::SpreadAttribute(attribute) => concat([
                        token("{..."),
                        self.expression(&attribute.argument, false)?,
                        token("}"),
                    ]),
                })
            })?);
        }
        if !attributes.is_empty() {
            head.push(indent(concat([
                line_or_space(),
                join(attributes, line_or_space()),
            ])));
        }
        if self_closing {
            head.extend([space(), token("/>")]);
        } else {
            head.push(token(">"));
        }
        Ok(group(concat(head)))
    }

    fn jsx_element_name(name: &oxc_ast::ast::JSXElementName<'_>) -> Doc {
        match name {
            oxc_ast::ast::JSXElementName::Identifier(identifier) => token(identifier.name.as_str()),
            oxc_ast::ast::JSXElementName::IdentifierReference(identifier) => {
                token(identifier.name.as_str())
            }
            oxc_ast::ast::JSXElementName::NamespacedName(name) => concat([
                token(name.namespace.name.as_str()),
                token(":"),
                token(name.name.name.as_str()),
            ]),
            oxc_ast::ast::JSXElementName::MemberExpression(member) => {
                Self::jsx_member_expression(member)
            }
            oxc_ast::ast::JSXElementName::ThisExpression(_) => token("this"),
        }
    }

    fn jsx_member_expression(member: &oxc_ast::ast::JSXMemberExpression<'_>) -> Doc {
        let object = match &member.object {
            oxc_ast::ast::JSXMemberExpressionObject::IdentifierReference(identifier) => {
                token(identifier.name.as_str())
            }
            oxc_ast::ast::JSXMemberExpressionObject::MemberExpression(member) => {
                Self::jsx_member_expression(member)
            }
            oxc_ast::ast::JSXMemberExpressionObject::ThisExpression(_) => token("this"),
        };
        concat([object, token("."), token(member.property.name.as_str())])
    }

    fn jsx_attribute_name(name: &oxc_ast::ast::JSXAttributeName<'_>) -> Doc {
        match name {
            oxc_ast::ast::JSXAttributeName::Identifier(identifier) => {
                token(identifier.name.as_str())
            }
            oxc_ast::ast::JSXAttributeName::NamespacedName(name) => concat([
                token(name.namespace.name.as_str()),
                token(":"),
                token(name.name.name.as_str()),
            ]),
        }
    }

    fn jsx_attribute_value(
        &self,
        value: &oxc_ast::ast::JSXAttributeValue<'_>,
    ) -> Result<Doc, FormatError> {
        match value {
            oxc_ast::ast::JSXAttributeValue::StringLiteral(literal) => {
                Ok(token(self.string_literal(literal)?))
            }
            oxc_ast::ast::JSXAttributeValue::ExpressionContainer(container) => {
                self.jsx_expression_container(container)
            }
            oxc_ast::ast::JSXAttributeValue::Element(element) => self.jsx_element(element),
            oxc_ast::ast::JSXAttributeValue::Fragment(fragment) => self.jsx_fragment(fragment),
        }
    }

    fn jsx_child(&self, child: &oxc_ast::ast::JSXChild<'_>) -> Result<Doc, FormatError> {
        match child {
            oxc_ast::ast::JSXChild::Text(text_node) => Ok(text(self.slice(text_node.span)?)),
            oxc_ast::ast::JSXChild::Element(element) => self.jsx_element(element),
            oxc_ast::ast::JSXChild::Fragment(fragment) => self.jsx_fragment(fragment),
            oxc_ast::ast::JSXChild::ExpressionContainer(container) => {
                self.jsx_expression_container(container)
            }
            oxc_ast::ast::JSXChild::Spread(spread) => Ok(concat([
                token("{..."),
                self.expression(&spread.expression, false)?,
                token("}"),
            ])),
        }
    }

    fn jsx_expression_container(
        &self,
        container: &oxc_ast::ast::JSXExpressionContainer<'_>,
    ) -> Result<Doc, FormatError> {
        match &container.expression {
            oxc_ast::ast::JSXExpression::EmptyExpression(_) => self.syntax_doc(container.span),
            expression => Ok(concat([
                token("{"),
                self.expression(expression.to_expression(), false)?,
                token("}"),
            ])),
        }
    }
}

#[cfg(feature = "benchmarking")]
pub struct PreparedDocument {
    document: Doc,
    newline: &'static str,
}

#[cfg(feature = "benchmarking")]
impl PreparedDocument {
    #[must_use]
    pub fn render(&self, config: &ResolvedConfig) -> String {
        render(&self.document, config, self.newline)
    }
}

#[cfg(feature = "benchmarking")]
/// Parses a benchmark input without formatting it.
///
/// # Errors
///
/// Returns a [`FormatError`] when source-type detection or parsing fails.
pub fn benchmark_parse(file_name: &Path, source: &str) -> Result<(), FormatError> {
    let allocator = Allocator::default();
    parse(&allocator, source, source_type(file_name)?)?;
    Ok(())
}

#[cfg(feature = "benchmarking")]
/// Builds the node and comment indices for a benchmark input.
///
/// # Errors
///
/// Returns a [`FormatError`] when source-type detection or parsing fails.
pub fn benchmark_index(file_name: &Path, source: &str) -> Result<usize, FormatError> {
    let allocator = Allocator::default();
    let parsed = parse(&allocator, source, source_type(file_name)?)?;
    let index = NodeIndex::build(&parsed.program, &parsed.tokens);
    let _comments = CommentTracker::new(source, &parsed.program.comments, &index);
    Ok(index.records().len())
}

#[cfg(feature = "benchmarking")]
/// Parses and lowers a benchmark input into the internal document IR.
///
/// # Errors
///
/// Returns a [`FormatError`] when parsing, indexing, comment attachment, or IR
/// generation fails.
pub fn prepare_document(
    file_name: &Path,
    source: &str,
    config: &ResolvedConfig,
) -> Result<PreparedDocument, FormatError> {
    let source = source.strip_prefix(BOM).unwrap_or(source);
    let allocator = Allocator::default();
    let parsed = parse(&allocator, source, source_type(file_name)?)?;
    let newline = resolve_newline(source, config.line_ending());
    let printer = Printer::new(
        source,
        config,
        &parsed.program,
        &parsed.tokens,
        &parsed.program.comments,
    );
    let document = printer.program_doc(&parsed.program)?;
    printer.finish_comments()?;
    Ok(PreparedDocument { document, newline })
}

#[derive(Debug)]
struct SyntaxLexeme {
    span: Span,
    text: String,
    line_comment: bool,
}

fn syntax_sequence(lexemes: &[SyntaxLexeme], index: &mut usize, closing: Option<&str>) -> Doc {
    let mut docs = Vec::new();
    let mut previous: Option<&SyntaxLexeme> = None;
    while let Some(current) = lexemes.get(*index) {
        if closing.is_some_and(|closing| current.text == closing) {
            *index += 1;
            break;
        }

        if let Some(previous) = previous {
            docs.push(syntax_separator(previous, current, lexemes.get(*index + 1)));
        }

        let last_index = if let Some(close) = matching_delimiter(&current.text) {
            let open = current.text.clone();
            *index += 1;
            if lexemes
                .get(*index)
                .is_some_and(|lexeme| lexeme.text == close)
            {
                *index += 1;
                docs.push(token(format!("{open}{close}")));
            } else {
                let inner = syntax_sequence(lexemes, index, Some(close));
                let spacing = open == "{";
                docs.push(group(concat([
                    token(open),
                    surround(inner, spacing),
                    token(close),
                ])));
            }
            (*index).saturating_sub(1)
        } else {
            docs.push(text(current.text.clone()));
            *index += 1;
            *index - 1
        };
        previous = lexemes.get(last_index);
    }
    concat(docs)
}

fn matching_delimiter(open: &str) -> Option<&'static str> {
    match open {
        "(" => Some(")"),
        "[" => Some("]"),
        "{" => Some("}"),
        "<" => Some(">"),
        _ => None,
    }
}

fn syntax_separator(
    previous: &SyntaxLexeme,
    current: &SyntaxLexeme,
    next: Option<&SyntaxLexeme>,
) -> Doc {
    if previous.line_comment {
        return hard_line();
    }
    if current.text.starts_with("//") {
        return space();
    }
    if previous.text.starts_with("/*") || current.text.starts_with("/*") {
        return space();
    }
    if previous.text.ends_with("${") {
        return empty();
    }
    if current.text == "?" {
        return if matches!(previous.text.as_str(), "-" | "+")
            || next.is_some_and(|next| next.text == ":")
        {
            empty()
        } else {
            space()
        };
    }
    if previous.text == "?" {
        return if current.text == ":" {
            empty()
        } else {
            space()
        };
    }
    if matches!(current.text.as_str(), "-" | "+")
        && previous.text == "]"
        && next.is_some_and(|next| next.text == "?")
    {
        return empty();
    }
    if matches!(previous.text.as_str(), "," | ";") || matches!(current.text.as_str(), "|" | "&") {
        return line_or_space();
    }
    if matches!(previous.text.as_str(), "|" | "&" | ":")
        || is_spaced_operator(&previous.text)
        || is_spaced_operator(&current.text)
    {
        return space();
    }
    if current.text == "{"
        && (is_word(&previous.text) || matches!(previous.text.as_str(), ")" | ">"))
    {
        return space();
    }
    if previous.text == "readonly" && current.text == "[" {
        return space();
    }
    if matches!(previous.text.as_str(), ")" | "]" | "}" | ">") && is_word(&current.text) {
        return space();
    }
    if is_word(&previous.text) && is_word(&current.text) {
        return space();
    }
    empty()
}

fn requote_raw_string(raw: &str, quote_style: QuoteStyle) -> String {
    let target = match quote_style {
        QuoteStyle::Single => '\'',
        QuoteStyle::Double => '"',
    };
    let Some(source) = raw.chars().next() else {
        return raw.to_owned();
    };
    if source == target || !matches!(source, '\'' | '"') || !raw.ends_with(source) {
        return raw.to_owned();
    }

    let mut output = String::with_capacity(raw.len());
    output.push(target);
    let mut characters = raw[1..raw.len() - 1].chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            let Some(escaped) = characters.next() else {
                output.push('\\');
                break;
            };
            if escaped == source {
                if escaped == target {
                    output.push('\\');
                }
                output.push(escaped);
            } else {
                output.push('\\');
                output.push(escaped);
            }
        } else {
            if character == target {
                output.push('\\');
            }
            output.push(character);
        }
    }
    output.push(target);
    output
}

fn is_spaced_operator(text: &str) -> bool {
    matches!(
        text,
        "=" | "=>"
            | "+"
            | "-"
            | "*"
            | "/"
            | "%"
            | "=="
            | "==="
            | "!="
            | "!=="
            | "??"
            | "&&"
            | "||"
            | "as"
            | "satisfies"
            | "extends"
            | "in"
            | "is"
    )
}

fn is_word(text: &str) -> bool {
    text.chars().next().is_some_and(|character| {
        character.is_alphanumeric() || matches!(character, '_' | '$' | '\'' | '"' | '`')
    })
}

fn module_export_name_text(name: &ModuleExportName<'_>) -> Option<String> {
    match name {
        ModuleExportName::IdentifierName(identifier) => Some(identifier.name.to_string()),
        ModuleExportName::IdentifierReference(identifier) => Some(identifier.name.to_string()),
        ModuleExportName::StringLiteral(_) => None,
    }
}

fn expression_statement_needs_parentheses(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::ObjectExpression(_)
        | Expression::FunctionExpression(_)
        | Expression::ClassExpression(_) => true,
        Expression::AssignmentExpression(assignment) => {
            matches!(assignment.left, AssignmentTarget::ObjectAssignmentTarget(_))
        }
        Expression::SequenceExpression(sequence) => sequence
            .expressions
            .first()
            .is_some_and(expression_statement_needs_parentheses),
        _ => false,
    }
}

fn function_has_types(function: &Function<'_>) -> bool {
    function.declare
        || function.type_parameters.is_some()
        || function.this_param.is_some()
        || function.return_type.is_some()
        || formal_parameters_have_types(&function.params)
}

fn formal_parameters_have_types(parameters: &oxc_ast::ast::FormalParameters<'_>) -> bool {
    parameters.items.iter().any(|parameter| {
        !parameter.decorators.is_empty()
            || parameter.type_annotation.is_some()
            || parameter.optional
            || parameter.accessibility.is_some()
            || parameter.readonly
            || parameter.r#override
    }) || parameters
        .rest
        .as_ref()
        .is_some_and(|rest| !rest.decorators.is_empty() || rest.type_annotation.is_some())
}

fn class_element_has_types(element: &oxc_ast::ast::ClassElement<'_>) -> bool {
    match element {
        oxc_ast::ast::ClassElement::StaticBlock(_) => false,
        oxc_ast::ast::ClassElement::MethodDefinition(method) => {
            method.r#type != oxc_ast::ast::MethodDefinitionType::MethodDefinition
                || method.r#override
                || method.optional
                || method.accessibility.is_some()
                || function_has_types(&method.value)
        }
        oxc_ast::ast::ClassElement::PropertyDefinition(property) => {
            property.r#type != oxc_ast::ast::PropertyDefinitionType::PropertyDefinition
                || property.type_annotation.is_some()
                || property.declare
                || property.r#override
                || property.optional
                || property.definite
                || property.readonly
                || property.accessibility.is_some()
        }
        oxc_ast::ast::ClassElement::AccessorProperty(accessor) => {
            accessor.r#type != oxc_ast::ast::AccessorPropertyType::AccessorProperty
                || accessor.type_annotation.is_some()
                || accessor.r#override
                || accessor.definite
                || accessor.accessibility.is_some()
        }
        oxc_ast::ast::ClassElement::TSIndexSignature(_) => true,
    }
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "callers construct separators inline and join clones them only when needed"
)]
fn join(mut docs: Vec<Doc>, separator: Doc) -> Doc {
    if docs.len() < 2 {
        return docs.pop().unwrap_or_else(empty);
    }
    let mut joined = Vec::with_capacity(docs.len() * 2 - 1);
    for (index, doc) in docs.into_iter().enumerate() {
        if index > 0 {
            joined.push(separator.clone());
        }
        joined.push(doc);
    }
    concat(joined)
}

fn comments_with_lines(comments: Vec<Doc>) -> Vec<Doc> {
    let mut docs = Vec::new();
    for (index, comment) in comments.into_iter().enumerate() {
        if index > 0 {
            docs.push(hard_line());
        }
        docs.push(comment);
    }
    docs
}

fn selector_kind_matches(selector: &StatementSelector, kind: StatementKind) -> bool {
    matches!(selector.kind, StatementKind::Any) || selector.kind == kind
}

fn classify_statement(statement: &Statement<'_>) -> StatementKind {
    match statement {
        Statement::ImportDeclaration(_) => StatementKind::Import,
        Statement::ExportAllDeclaration(_)
        | Statement::ExportDeclaration(_)
        | Statement::ExportDefaultDeclaration(_)
        | Statement::ExportFromDeclaration(_)
        | Statement::ExportNamedDeclaration(_)
        | Statement::TSExportAssignment(_)
        | Statement::TSNamespaceExportDeclaration(_) => StatementKind::Export,
        Statement::VariableDeclaration(declaration) => match declaration.kind {
            oxc_ast::ast::VariableDeclarationKind::Const => StatementKind::Const,
            oxc_ast::ast::VariableDeclarationKind::Let => StatementKind::Let,
            oxc_ast::ast::VariableDeclarationKind::Var => StatementKind::Var,
            oxc_ast::ast::VariableDeclarationKind::Using
            | oxc_ast::ast::VariableDeclarationKind::AwaitUsing => StatementKind::Other,
        },
        Statement::FunctionDeclaration(_) => StatementKind::Function,
        Statement::ClassDeclaration(_) => StatementKind::Class,
        Statement::TSTypeAliasDeclaration(_) => StatementKind::Type,
        Statement::TSInterfaceDeclaration(_) => StatementKind::Interface,
        Statement::TSEnumDeclaration(_) => StatementKind::Enum,
        Statement::TSNamespaceDeclaration(_) | Statement::TSExternalModuleDeclaration(_) => {
            StatementKind::Namespace
        }
        _ => StatementKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::{
        ArrowParentheses, CollectionItemLayout, CollectionLayout, FormatConfig, LineShape,
        Semicolons, StatementKind, StatementScope, StatementSelector, StatementSpacingRule,
        TrailingCommas, format_text, resolve_config,
    };

    #[test]
    fn formats_vertical_slice_and_is_idempotent() {
        let config = resolve_config(FormatConfig::default()).unwrap();
        let source = "import {foo,bar as baz} from \"pkg\"; const value={foo:[1,2],bar:true};";
        let formatted = format_text(Path::new("sample.ts"), source, &config)
            .unwrap()
            .unwrap();
        assert_eq!(
            formatted,
            "import { foo, bar as baz } from 'pkg';\nconst value = { foo: [1, 2], bar: true };\n"
        );
        assert!(
            format_text(Path::new("sample.ts"), &formatted, &config)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn preserves_bom_and_crlf() {
        let config = resolve_config(FormatConfig::default()).unwrap();
        let source = "\u{feff}const value={\r\nfoo:1\r\n};\r\n";
        let formatted = format_text(Path::new("sample.js"), source, &config)
            .unwrap()
            .unwrap();
        assert!(formatted.starts_with('\u{feff}'));
        assert!(formatted.contains("\r\n"));
        assert!(!formatted.replace("\r\n", "").contains('\n'));
    }

    #[test]
    fn rejects_invalid_source_instead_of_copying_raw_source() {
        let config = resolve_config(FormatConfig::default()).unwrap();
        let error = format_text(Path::new("sample.ts"), "const value = @;", &config).unwrap_err();
        assert_eq!(error.code(), "PARSE_ERROR");
    }

    #[test]
    fn formats_javascript_functions_and_control_flow() {
        let config = resolve_config(FormatConfig::default()).unwrap();
        let source = "function foo(a,b=1,...rest){if(a){return b;}else return rest[0];}for(let i=0;i<3;i++){foo(i);}try{throw new Error(\"x\");}catch(error){console.log(error);}finally{debugger;}const fn=(value)=>value*2;";
        let formatted = format_text(Path::new("sample.js"), source, &config)
            .unwrap()
            .unwrap();

        assert!(formatted.contains("function foo(a, b = 1, ...rest) {"));
        assert!(formatted.contains("if (a) {"));
        assert!(formatted.contains("for (let i = 0; i < 3; i++) {"));
        assert!(formatted.contains("} catch (error) {"));
        assert!(formatted.contains("const fn = (value) => value * 2;"));
        assert!(
            format_text(Path::new("sample.js"), &formatted, &config)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn formats_javascript_classes_and_exports() {
        let config = resolve_config(FormatConfig::default()).unwrap();
        let source = "class Example extends Base{static value=1;#private=2;constructor(x){this.x=x;}get current(){return this.x;}set current(value){this.x=value;}*items(){yield this.x;}static{this.ready=true;}}export{Example as Renamed};export default Example;export*as utilities from \"./utils.js\";";
        let formatted = format_text(Path::new("sample.js"), source, &config)
            .unwrap()
            .unwrap();

        assert!(formatted.contains("class Example extends Base {"));
        assert!(formatted.contains("static value = 1;"));
        assert!(formatted.contains("#private = 2;"));
        assert!(formatted.contains("get current() {"));
        assert!(formatted.contains("*items() {"));
        assert!(formatted.contains("export { Example as Renamed };"));
        assert!(formatted.contains("export * as utilities from './utils.js';"));
        assert!(
            format_text(Path::new("sample.js"), &formatted, &config)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn formats_remaining_javascript_expression_families() {
        let config = resolve_config(FormatConfig::default()).unwrap();
        let source = "import data from \"./data.json\" with {type:\"json\"};const pattern=/a+/gi;const message=`hello ${data?.user?.name??\"world\"}`;let first,rest;[first,...rest]=data.items;({value:first=0,...rest}=data);const loaded=await import(\"./module.js\",{with:{type:\"json\"}});const object={get value(){return first;},set value(next){first=next;},method(value){return value;},*items(){yield first;}};";
        let formatted = format_text(Path::new("sample.mjs"), source, &config)
            .unwrap()
            .unwrap();

        assert!(formatted.contains("with { type: 'json' }"));
        assert!(formatted.contains("/a+/gi"));
        assert!(formatted.contains("data?.user?.name ?? 'world'"));
        assert!(formatted.contains("[first, ...rest] = data.items;"));
        assert!(formatted.contains("({ value: first = 0, ...rest } = data);"));
        assert!(formatted.contains("get value() {"));
        assert!(formatted.contains("*items() {"));
        assert!(
            format_text(Path::new("sample.mjs"), &formatted, &config)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn formats_typescript_declarations_types_and_expressions() {
        let config = resolve_config(FormatConfig::default()).unwrap();
        let source = "interface User<T extends string=string>{readonly id:number;name:T;}type Maybe<T>=T|null|undefined;enum Kind{One,Two=\"two\"}declare function create<T>(value:T):User<T>;const user:User<string>={id:1,name:\"x\"} satisfies User<string>;const cast=user as unknown as User<string>;const nonNull=maybe!;";
        let formatted = format_text(Path::new("sample.ts"), source, &config)
            .unwrap()
            .unwrap();

        assert!(formatted.contains("interface User"));
        assert!(formatted.contains("type Maybe"));
        assert!(formatted.contains("declare function create"));
        assert!(formatted.contains("satisfies User<string>"));
        assert!(formatted.contains("as unknown as User<string>"));
        assert!(formatted.contains("maybe!"));
        assert!(
            format_text(Path::new("sample.ts"), &formatted, &config)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn formats_tsx_without_changing_meaningful_jsx_text() {
        let config = resolve_config(FormatConfig::default()).unwrap();
        let source = "interface Props{title:string;items:string[]}const View=({title,items}:Props)=><section id=\"root\">hello <h1>{title}</h1>{items.map(item=><span key={item}>{item}</span>)}</section>;";
        let formatted = format_text(Path::new("sample.tsx"), source, &config)
            .unwrap()
            .unwrap();

        assert!(formatted.contains("hello "));
        assert!(formatted.contains("<section"));
        assert!(formatted.contains("<span"));
        assert!(
            format_text(Path::new("sample.tsx"), &formatted, &config)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn formats_declaration_files() {
        let config = resolve_config(FormatConfig::default()).unwrap();
        let source = "export interface Options{value?:string;}export declare class Service<T>{private value;constructor(value:T);get():T;}declare namespace Service{type Result<T>=Promise<T>;}";
        for file_name in ["index.d.ts", "index.d.mts", "index.d.cts"] {
            let formatted = format_text(Path::new(file_name), source, &config)
                .unwrap()
                .unwrap();

            assert!(formatted.contains("export interface Options"));
            assert!(formatted.contains("export declare class Service"));
            assert!(formatted.contains("declare namespace Service"));
            assert!(
                format_text(Path::new(file_name), &formatted, &config)
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[test]
    fn preserves_leading_trailing_and_dangling_comments_once() {
        let config = resolve_config(FormatConfig::default()).unwrap();
        let source = "// leading\nconst value = { /* dangling */ key: 1 }; // trailing";
        let formatted = format_text(Path::new("sample.js"), source, &config)
            .unwrap()
            .unwrap();

        assert_eq!(formatted.matches("// leading").count(), 1);
        assert_eq!(formatted.matches("/* dangling */").count(), 1);
        assert_eq!(formatted.matches("// trailing").count(), 1);
        assert!(
            format_text(Path::new("sample.js"), &formatted, &config)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn preserves_comments_in_an_otherwise_empty_program() {
        let config = resolve_config(FormatConfig::default()).unwrap();
        let source = "// first\n/* second */";
        let formatted = format_text(Path::new("sample.js"), source, &config)
            .unwrap()
            .unwrap();

        assert_eq!(formatted, "// first\n/* second */\n");
        assert!(
            format_text(Path::new("sample.js"), &formatted, &config)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn suppression_preserves_the_next_node_as_raw_source() {
        let config = resolve_config(FormatConfig::default()).unwrap();
        let source = "// worsier-ignore\nwhile(true){const value={key:1};} // trailing";
        let formatted = format_text(Path::new("sample.js"), source, &config)
            .unwrap()
            .unwrap();

        assert_eq!(
            formatted,
            "// worsier-ignore\nwhile(true){const value={key:1};} // trailing\n"
        );
        assert!(
            format_text(Path::new("sample.js"), &formatted, &config)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn suppression_preserves_a_nested_ast_node_as_raw_source() {
        let config = resolve_config(FormatConfig::default()).unwrap();
        let source = "const value={\n// worsier-ignore\nuntouched   :[1,2],\nformatted:[1,2]\n};";
        let formatted = format_text(Path::new("sample.js"), source, &config)
            .unwrap()
            .unwrap();

        assert_eq!(
            formatted,
            "const value = {\n  // worsier-ignore\n  untouched   :[1,2],\n  formatted: [1, 2],\n};\n"
        );
        assert!(
            format_text(Path::new("sample.js"), &formatted, &config)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn prints_required_parentheses_from_the_ast() {
        let config = resolve_config(FormatConfig::default()).unwrap();
        let source = "const a=(one+two)*three;const b=one+(two*three);const c=(-one)**two;const d=(one??two)&&three;const e=one-(two-three);";
        let formatted = format_text(Path::new("sample.js"), source, &config)
            .unwrap()
            .unwrap();

        assert_eq!(
            formatted,
            "const a = (one + two) * three;\nconst b = one + two * three;\nconst c = (-one) ** two;\nconst d = (one ?? two) && three;\nconst e = one - (two - three);\n"
        );
        assert!(
            format_text(Path::new("sample.js"), &formatted, &config)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn adds_asi_guard_before_hazardous_statement() {
        let config = resolve_config(FormatConfig {
            semicolons: Semicolons::AsNeeded,
            ..FormatConfig::default()
        })
        .unwrap();
        let source = "const value=1;\n[one,two];";
        let formatted = format_text(Path::new("sample.js"), source, &config)
            .unwrap()
            .unwrap();

        assert_eq!(formatted, "const value = 1\n;[one, two]\n");
        assert!(
            format_text(Path::new("sample.js"), &formatted, &config)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn applies_block_spacing_and_asi_guards_inside_blocks() {
        let any = StatementSelector::default();
        let config = resolve_config(FormatConfig {
            semicolons: Semicolons::AsNeeded,
            statement_spacing: vec![StatementSpacingRule {
                previous: any.clone(),
                next: any,
                scope: StatementScope::Block,
                blank_lines: 1,
            }],
            ..FormatConfig::default()
        })
        .unwrap();
        let source = "function example(){const value=1;[value];}const outside=1;";
        let formatted = format_text(Path::new("sample.js"), source, &config)
            .unwrap()
            .unwrap();

        assert_eq!(
            formatted,
            "function example() {\n  const value = 1\n\n  ;[value]\n}\nconst outside = 1\n"
        );
    }

    #[test]
    fn applies_block_spacing_and_asi_guards_inside_switch_cases() {
        let any = StatementSelector::default();
        let config = resolve_config(FormatConfig {
            semicolons: Semicolons::AsNeeded,
            statement_spacing: vec![StatementSpacingRule {
                previous: any.clone(),
                next: any,
                scope: StatementScope::Block,
                blank_lines: 1,
            }],
            ..FormatConfig::default()
        })
        .unwrap();
        let source = "switch(value){case 1:const item=1;[item];}";
        let formatted = format_text(Path::new("sample.js"), source, &config)
            .unwrap()
            .unwrap();

        assert_eq!(
            formatted,
            "switch (value) {\n  case 1:\n    const item = 1\n\n    ;[item]\n}\n"
        );
    }

    #[test]
    fn statement_spacing_uses_final_shape_and_first_matching_rule() {
        let function = StatementSelector {
            kind: StatementKind::Function,
            line_shape: LineShape::MultiLine,
        };
        let constant = StatementSelector {
            kind: StatementKind::Const,
            line_shape: LineShape::SingleLine,
        };
        let config = resolve_config(FormatConfig {
            statement_spacing: vec![
                StatementSpacingRule {
                    previous: function.clone(),
                    next: constant.clone(),
                    scope: StatementScope::TopLevel,
                    blank_lines: 1,
                },
                StatementSpacingRule {
                    previous: function,
                    next: constant,
                    scope: StatementScope::TopLevel,
                    blank_lines: 2,
                },
            ],
            ..FormatConfig::default()
        })
        .unwrap();
        let source = "function example(){return 1;}const outside=1;";
        let formatted = format_text(Path::new("sample.js"), source, &config)
            .unwrap()
            .unwrap();

        assert!(formatted.contains("return 1;\n}\n\nconst outside"));
        assert!(!formatted.contains("return 1;\n}\n\n\nconst outside"));
    }

    #[test]
    fn statement_spacing_shape_excludes_attached_comments() {
        let constant = StatementSelector {
            kind: StatementKind::Const,
            line_shape: LineShape::SingleLine,
        };
        let config = resolve_config(FormatConfig {
            statement_spacing: vec![StatementSpacingRule {
                previous: constant.clone(),
                next: constant,
                scope: StatementScope::TopLevel,
                blank_lines: 1,
            }],
            ..FormatConfig::default()
        })
        .unwrap();
        let source = "// stays with first\nconst first=1;const second=2;";
        let formatted = format_text(Path::new("sample.js"), source, &config)
            .unwrap()
            .unwrap();

        assert_eq!(
            formatted,
            "// stays with first\nconst first = 1;\n\nconst second = 2;\n"
        );
    }

    #[test]
    fn classifies_every_statement_spacing_kind() {
        use oxc_allocator::Allocator;
        use oxc_parser::Parser;
        use oxc_span::SourceType;

        let allocator = Allocator::default();
        let source = "import 'x'; export {}; const a=1; let b=2; var c=3; function f(){} class C{} type T=string; interface I{} enum E{A} namespace N{} debugger;";
        let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
        assert!(parsed.diagnostics.is_empty());
        let kinds = parsed
            .program
            .body
            .iter()
            .map(super::classify_statement)
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            [
                StatementKind::Import,
                StatementKind::Export,
                StatementKind::Const,
                StatementKind::Let,
                StatementKind::Var,
                StatementKind::Function,
                StatementKind::Class,
                StatementKind::Type,
                StatementKind::Interface,
                StatementKind::Enum,
                StatementKind::Namespace,
                StatementKind::Other,
            ]
        );
    }

    #[test]
    fn applies_collection_and_import_policies() {
        let mut raw = FormatConfig::default();
        raw.objects.property_layout = CollectionItemLayout::OnePerLine;
        raw.arrays.element_layout = CollectionItemLayout::OnePerLine;
        raw.imports.specifier_layout = CollectionItemLayout::OnePerLine;
        let config = resolve_config(raw).unwrap();
        let source = "import{one,two}from'pkg';const object={one:1,two:2};const array=[one,two];";
        let formatted = format_text(Path::new("sample.js"), source, &config)
            .unwrap()
            .unwrap();

        assert!(formatted.contains("import {\n  one,\n  two\n} from 'pkg';"));
        assert!(formatted.contains("const object = {\n  one: 1,\n  two: 2,\n};"));
        assert!(formatted.contains("const array = [\n  one,\n  two,\n];"));
    }

    #[test]
    fn applies_single_line_and_token_style_policies() {
        let mut raw = FormatConfig::default();
        raw.objects.layout = CollectionLayout::SingleLine;
        raw.arrays.layout = CollectionLayout::SingleLine;
        raw.arrow_parentheses = ArrowParentheses::AsNeeded;
        raw.trailing_commas = TrailingCommas::All;
        let config = resolve_config(raw).unwrap();
        let source = "const object={one:1,two:2};const array=[one,two];const map=(value)=>value;";
        let formatted = format_text(Path::new("sample.js"), source, &config)
            .unwrap()
            .unwrap();

        assert_eq!(
            formatted,
            "const object = { one: 1, two: 2, };\nconst array = [one, two,];\nconst map = value => value;\n"
        );
    }
}

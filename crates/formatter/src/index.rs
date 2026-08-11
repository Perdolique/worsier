use std::collections::HashMap;

use oxc_ast::ast::Program;
use oxc_ast::ast_kind::AstKind;
use oxc_ast_visit::Visit;
use oxc_parser::Token;
use oxc_span::{GetSpan, Span};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeCategory {
    Program,
    Statement,
    Container,
    Other,
}

#[derive(Clone, Copy, Debug)]
pub struct NodeRecord {
    pub id: usize,
    #[allow(dead_code, reason = "stored for parent-sensitive formatting policies")]
    pub parent: Option<usize>,
    pub span: Span,
    #[allow(dead_code, reason = "stored for token-sensitive formatting policies")]
    pub first_token: usize,
    #[allow(dead_code, reason = "stored for token-sensitive formatting policies")]
    pub last_token_exclusive: usize,
    pub category: NodeCategory,
}

#[derive(Debug)]
pub struct NodeIndex {
    records: Vec<NodeRecord>,
    preferred_record_by_span: HashMap<(u32, u32), usize>,
}

impl NodeIndex {
    #[must_use]
    pub fn build(program: &Program<'_>, tokens: &[Token]) -> Self {
        let mut builder = IndexBuilder {
            records: Vec::new(),
            stack: Vec::new(),
            tokens,
        };
        builder.visit_program(program);
        debug_assert!(builder.stack.is_empty());
        let mut preferred_record_by_span =
            HashMap::<(u32, u32), usize>::with_capacity(builder.records.len());
        for record in &builder.records {
            preferred_record_by_span
                .entry((record.span.start, record.span.end))
                .and_modify(|id| {
                    if category_priority(record.category)
                        > category_priority(builder.records[*id].category)
                    {
                        *id = record.id;
                    }
                })
                .or_insert(record.id);
        }
        Self {
            records: builder.records,
            preferred_record_by_span,
        }
    }

    #[must_use]
    pub fn records(&self) -> &[NodeRecord] {
        &self.records
    }

    #[must_use]
    pub fn record(&self, id: usize) -> Option<&NodeRecord> {
        self.records.get(id)
    }

    pub fn preferred_record_with_span(&self, span: Span) -> Option<&NodeRecord> {
        self.preferred_record_by_span
            .get(&(span.start, span.end))
            .map(|id| &self.records[*id])
    }
}

pub(crate) const fn category_priority(category: NodeCategory) -> u8 {
    match category {
        NodeCategory::Program => 0,
        NodeCategory::Other => 1,
        NodeCategory::Container => 2,
        NodeCategory::Statement => 3,
    }
}

struct IndexBuilder<'a> {
    records: Vec<NodeRecord>,
    stack: Vec<usize>,
    tokens: &'a [Token],
}

impl<'a> Visit<'a> for IndexBuilder<'_> {
    fn enter_node(&mut self, kind: AstKind<'a>) {
        let span = kind.span();
        let id = self.records.len();
        let first_token = self
            .tokens
            .partition_point(|token| token.end() <= span.start);
        let last_token_exclusive = self
            .tokens
            .partition_point(|token| token.start() < span.end);
        self.records.push(NodeRecord {
            id,
            parent: self.stack.last().copied(),
            span,
            first_token,
            last_token_exclusive,
            category: category(kind),
        });
        self.stack.push(id);
    }

    fn leave_node(&mut self, kind: AstKind<'a>) {
        let id = self
            .stack
            .pop()
            .expect("every visited node must have an index entry");
        debug_assert_eq!(self.records[id].span, kind.span());
    }
}

fn category(kind: AstKind<'_>) -> NodeCategory {
    match kind {
        AstKind::Program(_) => NodeCategory::Program,
        AstKind::BlockStatement(_)
        | AstKind::BreakStatement(_)
        | AstKind::ContinueStatement(_)
        | AstKind::DebuggerStatement(_)
        | AstKind::DoWhileStatement(_)
        | AstKind::EmptyStatement(_)
        | AstKind::ExpressionStatement(_)
        | AstKind::ForInStatement(_)
        | AstKind::ForOfStatement(_)
        | AstKind::ForStatement(_)
        | AstKind::IfStatement(_)
        | AstKind::LabeledStatement(_)
        | AstKind::ReturnStatement(_)
        | AstKind::SwitchStatement(_)
        | AstKind::ThrowStatement(_)
        | AstKind::TryStatement(_)
        | AstKind::WhileStatement(_)
        | AstKind::WithStatement(_)
        | AstKind::VariableDeclaration(_)
        | AstKind::Function(_)
        | AstKind::Class(_)
        | AstKind::ImportDeclaration(_)
        | AstKind::ExportAllDeclaration(_)
        | AstKind::ExportDefaultDeclaration(_)
        | AstKind::ExportNamedDeclaration(_)
        | AstKind::TSTypeAliasDeclaration(_)
        | AstKind::TSInterfaceDeclaration(_)
        | AstKind::TSEnumDeclaration(_)
        | AstKind::TSExternalModuleDeclaration(_)
        | AstKind::TSNamespaceDeclaration(_)
        | AstKind::TSGlobalDeclaration(_)
        | AstKind::TSImportEqualsDeclaration(_)
        | AstKind::TSExportAssignment(_)
        | AstKind::TSNamespaceExportDeclaration(_) => NodeCategory::Statement,
        AstKind::ArrayExpression(_)
        | AstKind::ObjectExpression(_)
        | AstKind::ArrayPattern(_)
        | AstKind::ObjectPattern(_)
        | AstKind::ClassBody(_)
        | AstKind::FunctionBody(_)
        | AstKind::JSXElement(_)
        | AstKind::JSXFragment(_)
        | AstKind::TSTypeLiteral(_)
        | AstKind::TSInterfaceBody(_)
        | AstKind::TSEnumBody(_)
        | AstKind::TSModuleBlock(_) => NodeCategory::Container,
        _ => NodeCategory::Other,
    }
}

#[cfg(test)]
mod tests {
    use oxc_allocator::Allocator;
    use oxc_parser::{Parser, config::TokensParserConfig};
    use oxc_span::SourceType;

    use super::{NodeCategory, NodeIndex};

    #[test]
    fn records_parent_and_token_boundaries() {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, "const value = { key: 1 };", SourceType::ts())
            .with_config(TokensParserConfig)
            .parse();
        let index = NodeIndex::build(&parsed.program, &parsed.tokens);
        let object = index
            .records()
            .iter()
            .find(|record| record.category == NodeCategory::Container)
            .unwrap();

        assert!(object.parent.is_some());
        assert!(object.first_token < object.last_token_exclusive);
        assert!(index.record(object.id).is_some());
    }
}

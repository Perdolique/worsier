use oxc_ast::ast::{Expression, Statement};
use oxc_syntax::operator::UnaryOperator;

#[must_use]
pub fn needs_leading_semicolon(statement: &Statement<'_>) -> bool {
    let Statement::ExpressionStatement(statement) = statement else {
        return false;
    };
    expression_starts_hazardously(&statement.expression)
}

fn expression_starts_hazardously(expression: &Expression<'_>) -> bool {
    match expression {
        Expression::ArrayExpression(_)
        | Expression::TemplateLiteral(_)
        | Expression::RegExpLiteral(_)
        | Expression::JSXElement(_)
        | Expression::JSXFragment(_)
        | Expression::ChainExpression(_)
        | Expression::ParenthesizedExpression(_)
        | Expression::TSTypeAssertion(_) => true,
        Expression::UnaryExpression(expression) => matches!(
            expression.operator,
            UnaryOperator::UnaryPlus | UnaryOperator::UnaryNegation
        ),
        Expression::CallExpression(expression) => expression_starts_hazardously(&expression.callee),
        Expression::TaggedTemplateExpression(expression) => {
            expression_starts_hazardously(&expression.tag)
        }
        Expression::TSAsExpression(expression) => {
            expression_starts_hazardously(&expression.expression)
        }
        Expression::TSSatisfiesExpression(expression) => {
            expression_starts_hazardously(&expression.expression)
        }
        Expression::TSNonNullExpression(expression) => {
            expression_starts_hazardously(&expression.expression)
        }
        Expression::TSInstantiationExpression(expression) => {
            expression_starts_hazardously(&expression.expression)
        }
        Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Identifier(_)
        | Expression::Super(_)
        | Expression::ArrowFunctionExpression(_)
        | Expression::AssignmentExpression(_)
        | Expression::AwaitExpression(_)
        | Expression::BinaryExpression(_)
        | Expression::ClassExpression(_)
        | Expression::ConditionalExpression(_)
        | Expression::FunctionExpression(_)
        | Expression::ImportExpression(_)
        | Expression::LogicalExpression(_)
        | Expression::NewExpression(_)
        | Expression::ObjectExpression(_)
        | Expression::SequenceExpression(_)
        | Expression::ThisExpression(_)
        | Expression::UpdateExpression(_)
        | Expression::YieldExpression(_)
        | Expression::PrivateInExpression(_)
        | Expression::ImportMeta(_)
        | Expression::NewTarget(_)
        | Expression::V8IntrinsicExpression(_)
        | Expression::ComputedMemberExpression(_)
        | Expression::StaticMemberExpression(_)
        | Expression::PrivateFieldExpression(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    use super::needs_leading_semicolon;

    fn hazardous(source: &str) -> bool {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        needs_leading_semicolon(parsed.program.body.first().unwrap())
    }

    #[test]
    fn recognizes_asi_hazards() {
        assert!(hazardous("[1, 2]"));
        assert!(hazardous("+value"));
        assert!(hazardous("`value`"));
        assert!(!hazardous("value"));
        assert!(!hazardous("value + 1"));
    }
}

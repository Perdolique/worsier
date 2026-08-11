use oxc_ast::ast::Expression;
use oxc_syntax::operator::{BinaryOperator, LogicalOperator};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Precedence {
    Sequence,
    Yield,
    Assignment,
    Conditional,
    Coalesce,
    LogicalOr,
    LogicalAnd,
    BitwiseOr,
    BitwiseXor,
    BitwiseAnd,
    Equality,
    Relational,
    Shift,
    Additive,
    Multiplicative,
    Exponential,
    Unary,
    Update,
    Call,
    Member,
    Primary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Associativity {
    Left,
    Right,
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParentPosition {
    Left,
    Right,
    Test,
    Consequent,
    Alternate,
    Operand,
    Callee,
    Object,
    Tag,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operator {
    Binary(BinaryOperator),
    Logical(LogicalOperator),
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParentContext {
    pub precedence: Precedence,
    pub associativity: Associativity,
    pub position: ParentPosition,
    pub operator: Operator,
}

impl ParentContext {
    #[must_use]
    pub const fn new(
        precedence: Precedence,
        associativity: Associativity,
        position: ParentPosition,
    ) -> Self {
        Self {
            precedence,
            associativity,
            position,
            operator: Operator::Other,
        }
    }

    #[must_use]
    pub const fn binary(operator: BinaryOperator, position: ParentPosition) -> Self {
        Self {
            precedence: binary_precedence(operator),
            associativity: if matches!(operator, BinaryOperator::Exponential) {
                Associativity::Right
            } else {
                Associativity::Left
            },
            position,
            operator: Operator::Binary(operator),
        }
    }

    #[must_use]
    pub const fn logical(operator: LogicalOperator, position: ParentPosition) -> Self {
        Self {
            precedence: logical_precedence(operator),
            associativity: Associativity::Left,
            position,
            operator: Operator::Logical(operator),
        }
    }
}

#[must_use]
pub const fn binary_precedence(operator: BinaryOperator) -> Precedence {
    match operator {
        BinaryOperator::BitwiseOR => Precedence::BitwiseOr,
        BinaryOperator::BitwiseXOR => Precedence::BitwiseXor,
        BinaryOperator::BitwiseAnd => Precedence::BitwiseAnd,
        BinaryOperator::Equality
        | BinaryOperator::Inequality
        | BinaryOperator::StrictEquality
        | BinaryOperator::StrictInequality => Precedence::Equality,
        BinaryOperator::LessThan
        | BinaryOperator::LessEqualThan
        | BinaryOperator::GreaterThan
        | BinaryOperator::GreaterEqualThan
        | BinaryOperator::In
        | BinaryOperator::Instanceof => Precedence::Relational,
        BinaryOperator::ShiftLeft
        | BinaryOperator::ShiftRight
        | BinaryOperator::ShiftRightZeroFill => Precedence::Shift,
        BinaryOperator::Addition | BinaryOperator::Subtraction => Precedence::Additive,
        BinaryOperator::Multiplication | BinaryOperator::Division | BinaryOperator::Remainder => {
            Precedence::Multiplicative
        }
        BinaryOperator::Exponential => Precedence::Exponential,
    }
}

#[must_use]
pub const fn logical_precedence(operator: LogicalOperator) -> Precedence {
    match operator {
        LogicalOperator::Or => Precedence::LogicalOr,
        LogicalOperator::And => Precedence::LogicalAnd,
        LogicalOperator::Coalesce => Precedence::Coalesce,
    }
}

#[must_use]
pub fn expression_precedence(expression: &Expression<'_>) -> Precedence {
    match expression {
        Expression::SequenceExpression(_) => Precedence::Sequence,
        Expression::YieldExpression(_) => Precedence::Yield,
        Expression::ArrowFunctionExpression(_) | Expression::AssignmentExpression(_) => {
            Precedence::Assignment
        }
        Expression::ConditionalExpression(_) => Precedence::Conditional,
        Expression::LogicalExpression(expression) => logical_precedence(expression.operator),
        Expression::BinaryExpression(expression) => binary_precedence(expression.operator),
        Expression::PrivateInExpression(_)
        | Expression::TSAsExpression(_)
        | Expression::TSSatisfiesExpression(_)
        | Expression::TSTypeAssertion(_) => Precedence::Relational,
        Expression::UnaryExpression(_) | Expression::AwaitExpression(_) => Precedence::Unary,
        Expression::UpdateExpression(_) => Precedence::Update,
        Expression::CallExpression(_)
        | Expression::ImportExpression(_)
        | Expression::ChainExpression(_) => Precedence::Call,
        Expression::NewExpression(_)
        | Expression::TaggedTemplateExpression(_)
        | Expression::ComputedMemberExpression(_)
        | Expression::StaticMemberExpression(_)
        | Expression::PrivateFieldExpression(_)
        | Expression::TSNonNullExpression(_)
        | Expression::TSInstantiationExpression(_) => Precedence::Member,
        Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::RegExpLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::TemplateLiteral(_)
        | Expression::Identifier(_)
        | Expression::Super(_)
        | Expression::ArrayExpression(_)
        | Expression::ClassExpression(_)
        | Expression::FunctionExpression(_)
        | Expression::ObjectExpression(_)
        | Expression::ParenthesizedExpression(_)
        | Expression::ThisExpression(_)
        | Expression::ImportMeta(_)
        | Expression::NewTarget(_)
        | Expression::JSXElement(_)
        | Expression::JSXFragment(_)
        | Expression::V8IntrinsicExpression(_) => Precedence::Primary,
    }
}

#[must_use]
pub fn needs_parentheses(expression: &Expression<'_>, parent: ParentContext) -> bool {
    if nullish_mix(expression, parent.operator) {
        return true;
    }

    if matches!(
        parent.operator,
        Operator::Binary(BinaryOperator::Exponential)
    ) && matches!(parent.position, ParentPosition::Left)
        && matches!(expression, Expression::UnaryExpression(_))
    {
        return true;
    }

    let child = expression_precedence(expression);
    if child < parent.precedence {
        return !position_accepts_lower_precedence(expression, parent);
    }
    if child > parent.precedence {
        return false;
    }

    match parent.associativity {
        Associativity::Left => matches!(parent.position, ParentPosition::Right),
        Associativity::Right => {
            matches!(parent.position, ParentPosition::Left | ParentPosition::Test)
        }
        Associativity::None => true,
    }
}

fn position_accepts_lower_precedence(expression: &Expression<'_>, parent: ParentContext) -> bool {
    match parent.position {
        ParentPosition::Object | ParentPosition::Tag
            if parent.precedence == Precedence::Member
                && expression_precedence(expression) == Precedence::Call =>
        {
            true
        }
        ParentPosition::Consequent | ParentPosition::Alternate
            if parent.precedence == Precedence::Conditional =>
        {
            expression_precedence(expression) >= Precedence::Assignment
        }
        ParentPosition::Right if parent.precedence == Precedence::Assignment => {
            expression_precedence(expression) >= Precedence::Assignment
        }
        _ => false,
    }
}

fn nullish_mix(expression: &Expression<'_>, parent: Operator) -> bool {
    let Expression::LogicalExpression(child) = expression else {
        return false;
    };
    let Operator::Logical(parent) = parent else {
        return false;
    };
    matches!(child.operator, LogicalOperator::Coalesce)
        != matches!(parent, LogicalOperator::Coalesce)
}

#[cfg(test)]
mod tests {
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;
    use oxc_syntax::operator::{BinaryOperator, LogicalOperator};

    use super::{ParentContext, ParentPosition, needs_parentheses};

    fn check(source: &str, parent: ParentContext) -> bool {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let Some(oxc_ast::ast::Statement::ExpressionStatement(statement)) =
            parsed.program.body.first()
        else {
            panic!("expected expression statement");
        };
        needs_parentheses(&statement.expression, parent)
    }

    #[test]
    fn observes_binary_associativity() {
        assert!(check(
            "a + b",
            ParentContext::binary(BinaryOperator::Multiplication, ParentPosition::Left)
        ));
        assert!(!check(
            "a + b",
            ParentContext::binary(BinaryOperator::Addition, ParentPosition::Left)
        ));

        assert!(!check(
            "a * b",
            ParentContext::binary(BinaryOperator::Addition, ParentPosition::Right)
        ));
    }

    #[test]
    fn protects_exponentiation_and_nullish_grammar() {
        assert!(check(
            "-a",
            ParentContext::binary(BinaryOperator::Exponential, ParentPosition::Left)
        ));

        assert!(check(
            "a ?? b",
            ParentContext::logical(LogicalOperator::And, ParentPosition::Left)
        ));
    }

    #[test]
    fn allows_member_access_and_tags_after_calls() {
        assert!(!check(
            "factory()",
            ParentContext::new(
                super::Precedence::Member,
                super::Associativity::Left,
                ParentPosition::Object,
            )
        ));
        assert!(!check(
            "factory()",
            ParentContext::new(
                super::Precedence::Member,
                super::Associativity::Left,
                ParentPosition::Tag,
            )
        ));
    }
}

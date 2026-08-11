# Worsier formatting contract

Worsier defines its own canonical style. Compatibility with Prettier, Oxfmt,
or another formatter is not a goal.

## General layout

- Empty delimited constructs remain compact.
- A non-empty sequence remains on one line while it fits within `lineWidth`.
- When a sequence breaks, the opening delimiter ends its line, items receive
  one indentation level, and the closing delimiter aligns with the construct.
- Blocks preserve the source AST structure. Formatting never inserts or removes
  braces.
- Binary and logical chains break before the operator.
- Conditional expressions break before `?` and `:` on indented lines.
- Call and `new` arguments, and function parameters, use one item per line when
  their sequence breaks.
- Member and call chains break before `.`, `?.`, or a computed segment.
- Named import and export specifiers use one item per line when they break.
- Variable declarations use one declarator per line when they break.
- A non-block control-flow body is printed on the following indented line.
- `else`, `catch`, and `finally` remain beside the preceding closing brace.
- TypeScript unions and intersections use leading `|` and `&` when broken.
- JSX attributes use one attribute per line when broken. Meaningful JSX
  whitespace is preserved.
- Numeric, regular-expression, and template raw text is preserved. String
  literals follow `quoteStyle`.
- Required parentheses come from Worsier's precedence, associativity, and
  parent-position model. Redundant source parentheses are not preserved.
- With `semicolons: "asNeeded"`, a dedicated ASI guard inserts a leading
  semicolon before a hazardous statement start.
- Declarations, imports, object properties, and union members are never sorted.

## Statement spacing

- `statementSpacing` rules are evaluated from top to bottom, and the first
  matching rule wins.
- `blankLines: 0` means one line break with no empty line.
- `lineShape` is measured from the final printed statement code with dprint
  line markers. Leading and trailing comments are not part of that shape.
- Spacing keeps each attached comment with its statement and never moves a
  comment across user code.

## Comments and suppression

- Comment text is emitted byte-for-byte.
- Every source comment is emitted exactly once. A missing or repeated comment
  is an internal error.
- Line comments use line-suffix semantics and never cross a line boundary.
- Dangling comments inside empty delimiters belong to the container.
- Comments never move across user tokens.
- `// worsier-ignore` and `/* worsier-ignore */` preserve the next AST node as
  an exact source slice.
- Suppressed nodes still participate in output parsing and semantic AST
  verification.

## Semantic guarantees

For every successful format operation, Worsier reparses the output when
`verifyAst` is enabled and requires Oxc `Program::content_eq` with the input.
Formatting an already formatted output produces no change.

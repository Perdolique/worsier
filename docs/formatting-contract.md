# Worsier formatting contract

Worsier rewrites static `import` declarations, TypeScript interface layout, statement and member
semicolons, trailing commas in supported lists, and whitespace boundaries around direct runtime
variable declarations. It does not otherwise rewrite the contents of those declarations or lists.

## Import layout

- `rules.importLayout` defaults to `true` and controls only the contents of static imports.
- `lineWidth` defaults to `120`. A flat import whose length equals the limit stays on one line.
- Named imports use `{ a, b }` spacing and retain specifier order, aliases, `type` modifiers,
  module-specifier quotes, semicolons, import attributes, and comments. The separate
  `rules.semicolons.statements` rule may then normalize the semicolon.
- A named import that exceeds `lineWidth` places every specifier on its own line with two spaces of
  indentation. Import layout preserves whether the named list had a trailing comma; the separate
  `rules.trailingCommas` rule may then normalize it.
- A single named specifier also breaks when the complete flat import exceeds `lineWidth`.
- Default-only, namespace, and side-effect imports have no named list to break and stay on one line.
- Dynamic `import()` expressions are not changed.
- When `rules.importLayout` is `false`, every byte inside each static import span is preserved.

```ts
import { type A, b, c } from 'package'

import {
  one,
  two
} from 'package-with-a-long-name'
```

## Interface layout

- `rules.interfaceLayout` accepts `"off"` or an integer from `0` through `4294967295` and defaults
  to `0`. JSON number spellings such as `1.0` and `1e0` are accepted when their value is an integer
  in that range.
- An interface is expanded when its number of direct members is strictly greater than the numeric
  threshold. The default therefore expands every nonempty interface, while an empty interface is
  unchanged.
- Properties, methods, index signatures, call signatures, and construct signatures all count as
  members. Object type aliases and other brace-delimited TypeScript types are outside the rule.
- An expanded interface places the first member after a line break, every member on its own line
  with two spaces of relative indentation, and the closing brace after a final line break.
- Members that originally share the physical line governed by a preceding `// @ts-ignore` or
  `// @ts-expect-error` stay together on that line so formatting preserves the directive's
  TypeScript diagnostic scope.
- Member order, text, separators, comments, and any multiline layout inside a member are preserved.
  The separate `rules.semicolons.typeMembers` rule may then normalize member semicolons.
- Interfaces at or below the threshold retain their original layout. `"off"` preserves every
  interface layout regardless of its size.
- When expansion starts inside a single-line program, block, namespace, or module statement list,
  its enclosing inline containers expand outward with two-space indentation. In an existing
  multiline outer container, only inline boundaries adjacent to the affected declaration unfold;
  the container's existing indentation is retained.

```ts
interface Example {
  value: string
  run(): void
}
```

## Trailing commas

`rules.trailingCommas` accepts `"always"`, `"never"`, or `"off"` and defaults to `"never"`.

- `"always"` adds a comma to every eligible multiline list and removes optional trailing commas
  from single-line lists.
- `"never"` removes every optional trailing comma from eligible lists.
- `"off"` preserves the presence of trailing commas, including in imports rewritten by
  `rules.importLayout`.

The rule covers object and array literals, binding and assignment destructuring, named static
imports and exports, import/export attributes, function and method parameters, call and `new`
arguments, TypeScript tuple types, enum bodies, and type parameter declarations. A list is
multiline when its delimiters are on different physical lines. A call or `new` expression whose
single multiline argument stays attached to both parentheses is not treated as a multiline
argument list.

Empty lists, dynamic `import()` arguments, and TypeScript type argument instantiations are outside
the rule. Terminal sparse-array or array-pattern elisions are preserved because their comma is
semantic. A comma is not added after a rest parameter, destructuring rest, or tuple rest element.
The required comma on an otherwise ambiguous single generic-arrow parameter in TSX, `.mts`, and
`.cts` is preserved in every mode. An explicit constraint removes that ambiguity; a default also
removes it in TSX, while `.mts` and `.cts` still require either a constraint or comma. Trailing
comments and source parentheses stay in place: an added comma is inserted after the final syntax
token, including a closing parenthesis around the final item, and before the comment.

```ts
const value = {
  items: [
    first,
    second,
  ],
}
```

## Semicolons

`rules.semicolons` configures three independent syntax groups. Every group accepts `"always"`,
`"asNeeded"`, or `"off"` and defaults to `"asNeeded"`.

- `statements` covers directives, runtime statements, static imports and exports, and terminable
  TypeScript declarations.
- `classMembers` covers fields, accessor properties, index signatures, and declarations or
  overloads without a body. Concrete methods with a body are unchanged.
- `typeMembers` covers members of interfaces and object types, including mapped types.

`"always"` adds a semicolon to every eligible ending. `"asNeeded"` removes an optional trailing
semicolon when automatic semicolon insertion can separate the syntax. When the next statement or
computed class member could merge with the previous one, the separator is moved to the beginning
of that item instead:

```ts
const values = load()
;[first, second].forEach(use)

class Example {
  value = load()
  ;[key] = other
}
```

`"off"` preserves semicolons in that group. Commas between TypeScript type members are always
preserved and count as existing separators in `"always"` mode. Semicolons required on the same
line, semicolons inside `for` headers, and standalone empty statements are not removed. The rule
does not add a leading guard before the first item in a statement or class-member list.

## Statement boundaries

`rules.statementSpacing.imports` and `rules.statementSpacing.variableDeclarations` each accept
`"separate"`, `"compact"`, or `"off"`. Both default to `"separate"`.

Each statement contributes a requirement to its shared boundary. A blank-line requirement wins
over a one-line-break requirement. If neither statement contributes a requirement, the original
boundary is preserved.

`"compact"` always requires exactly one line break. `"off"` contributes no requirement.
`"separate"` uses the statement shape and category:

| Adjacent statements | Separator |
| --- | --- |
| single-line import to single-line import | one line break |
| import to import when either import is multiline | one blank line |
| any import next to non-import code | one blank line |
| single-line variable to single-line variable | one line break |
| variable to variable when either declaration is multiline | one blank line |
| any variable next to other code | one blank line |
| other code to other code | original source, or one line break in an unfolded inline list |

One blank line means exactly two LF or CRLF sequences with no whitespace on the empty line. Worsier
does not add whitespace at the start or end of a file.

Import spacing uses the formatted import shape when `rules.importLayout` is `true` and the original
shape when it is `false`. Disabling import spacing does not disable import layout. On an import to
variable-declaration boundary, for example, `"separate"` plus `"compact"` produces a blank line,
`"compact"` plus `"compact"` produces one line break, and `"off"` plus `"compact"` produces one line
break.

## Runtime variable declarations

- `rules.statementSpacing.variableDeclarations` applies only to direct statement-list `const`,
  `let`, and `var` declarations.
- Statement spacing does not rewrite declaration contents. The trailing-comma rule may independently
  change a supported list inside a declaration; all other initializers, destructuring, types,
  comments, semicolons, and multi-declarator structure are preserved. The separate semicolon rule
  may then normalize the statement ending.
- Exported declarations, explicit `declare` declarations, declaration files, declarations inside
  declared namespaces/modules/globals, `using`, `await using`, and declarations in `for` headers
  are excluded. Ambient declarations may be supported in the future, but are not part of this rule;
  no separate future configuration key is reserved.
- Statement lists include files, function bodies, ordinary blocks, `try`/`catch`/`finally` blocks,
  class static blocks, TypeScript module/namespace blocks, and each switch-case consequent.
- In `"separate"` or `"compact"` mode, a single-line list with at least two items and a direct
  runtime variable is unfolded. Every sibling gets its own line, variable boundaries follow the
  selected mode, and nested inline containers cascade outward with two-space indentation. Switch
  labels use one indentation level and their consequent statements use two. `"off"` does not
  unfold a list by itself.
- A lone runtime variable in an inline block does not unfold that block. Existing multiline
  containers keep their current indentation.

## Source and semantic guarantees

- Source outside static imports, semicolon tokens or guards, trailing-comma tokens, or rewritten
  whitespace boundaries is preserved byte-for-byte.
- Existing LF or CRLF line endings are used for rewritten imports and boundaries. A UTF-8 BOM and
  the presence or absence of a final newline are preserved.
- When `verifyAst` is enabled, Worsier reparses the output and requires Oxc `Program::content_eq`
  with the input.
- Formatting an already formatted source produces no further change.

## Configuration

The CLI searches from each input toward the Git or filesystem root for the nearest
`worsier.jsonc`. If none exists, it uses the defaults below. `--config` selects one file explicitly,
and `--init` creates an optional complete configuration. A discovered or explicit invalid file is
an error rather than a signal to fall back to defaults.

The programmatic `format(fileName, sourceText, config?)` API uses the same defaults when its third
argument is omitted. It does not discover configuration files.

```jsonc
{
  "$schema": "./node_modules/worsier/configuration_schema.json",
  "lineWidth": 120,
  "verifyAst": true,
  "rules": {
    "importLayout": true,
    "interfaceLayout": 0,
    "statementSpacing": {
      "imports": "separate",
      "variableDeclarations": "separate"
    },
    "semicolons": {
      "statements": "asNeeded",
      "classMembers": "asNeeded",
      "typeMembers": "asNeeded"
    },
    "trailingCommas": "never"
  },
  "ignorePatterns": []
}
```

Unknown keys, invalid interface-layout values, spacing modes, semicolon modes, and trailing-comma
modes are configuration errors. Interface layout thresholds must be integers from `0` through
`4294967295`.
The removed top-level `semicolons` and `trailingCommas`, `rules.imports`, and `rules.variables` keys
are not aliases. Migrate the old rule keys with the following exact replacements:

| Removed setting | Replacement |
| --- | --- |
| `rules.imports: true` | `rules.importLayout: true` and `rules.statementSpacing.imports: "separate"` |
| `rules.imports: false` | `rules.importLayout: false` and `rules.statementSpacing.imports: "off"` |
| `rules.variables: true` | `rules.statementSpacing.variableDeclarations: "separate"` |
| `rules.variables: false` | `rules.statementSpacing.variableDeclarations: "off"` |

When import layout is `false`, interface layout is `"off"`, both spacing modes are `"off"`, all
semicolon groups are `"off"`, and trailing commas are `"off"`, formatting returns the source
unchanged. `lineWidth` only controls import layout. The CLI flag `--no-verify` disables AST
verification for one invocation.

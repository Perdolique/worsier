# Worsier formatting contract

Worsier rewrites static `import` declarations and whitespace boundaries around direct runtime
variable declarations. It does not rewrite the contents of those variable declarations.

## Import layout

- `rules.importLayout` defaults to `true` and controls only the contents of static imports.
- `lineWidth` defaults to `120`. A flat import whose length equals the limit stays on one line.
- Named imports use `{ a, b }` spacing and retain specifier order, aliases, `type` modifiers,
  module-specifier quotes, semicolons, import attributes, and comments.
- A named import that exceeds `lineWidth` places every specifier on its own line with two spaces of
  indentation. Its trailing comma is removed.
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
- The declaration span is preserved: initializers, destructuring, types, comments, commas,
  semicolons, and multi-declarator statements are not reformatted or split.
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

- Variable declaration contents and source outside static imports or rewritten whitespace
  boundaries are preserved byte-for-byte.
- Existing LF or CRLF line endings are used for rewritten imports and boundaries. A UTF-8 BOM is
  preserved, as are semicolons and the presence or absence of a final newline.
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
    "statementSpacing": {
      "imports": "separate",
      "variableDeclarations": "separate"
    }
  },
  "ignorePatterns": []
}
```

Unknown keys and unknown spacing modes are configuration errors. The removed `rules.imports` and
`rules.variables` keys are not aliases. Migrate them with the following exact replacements:

| Removed setting | Replacement |
| --- | --- |
| `rules.imports: true` | `rules.importLayout: true` and `rules.statementSpacing.imports: "separate"` |
| `rules.imports: false` | `rules.importLayout: false` and `rules.statementSpacing.imports: "off"` |
| `rules.variables: true` | `rules.statementSpacing.variableDeclarations: "separate"` |
| `rules.variables: false` | `rules.statementSpacing.variableDeclarations: "off"` |

When import layout is `false` and both spacing modes are `"off"`, formatting returns the source
unchanged. `lineWidth` only controls import layout. The CLI flag `--no-verify` disables AST
verification for one invocation.

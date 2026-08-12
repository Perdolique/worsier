# Worsier formatting contract

Worsier rewrites static `import` declarations. Every source range outside an import declaration
or an import statement boundary is preserved byte-for-byte.

## Import layout

- `lineWidth` defaults to `120`. A flat import whose length equals the limit stays on one line.
- Named imports use `{ a, b }` spacing and retain specifier order, aliases, `type` modifiers,
  module-specifier quotes, semicolons, import attributes, and comments.
- A named import that exceeds `lineWidth` places every specifier on its own line with two spaces of
  indentation. Its trailing comma is removed.
- A single named specifier also breaks when the complete flat import exceeds `lineWidth`.
- Default-only, namespace, and side-effect imports have no named list to break and stay on one line.
- Dynamic `import()` expressions are not changed.

```ts
import { type A, b, c } from 'package'

import {
  one,
  two
} from 'package-with-a-long-name'
```

## Import boundaries

Final statement shapes determine the separator:

| Adjacent statements | Separator |
| --- | --- |
| single-line import to single-line import | one line break |
| single-line import next to multiline import | one blank line |
| multiline import to multiline import | one blank line |
| any import next to non-import code | one blank line |
| non-import to non-import | original source |

One blank line means exactly two LF or CRLF sequences with no whitespace on the empty line. Worsier
does not add whitespace at the start or end of a file.

## Source and semantic guarantees

- Object literals, destructuring, exports, type literals, arrays, quote choices, semicolons,
  indentation, and the final newline outside static imports are preserved byte-for-byte.
- Existing LF or CRLF line endings are used for rewritten imports and separators. A UTF-8 BOM is
  preserved.
- When `verifyAst` is enabled, Worsier reparses the output and requires Oxc `Program::content_eq`
  with the input.
- Formatting an already formatted source produces no further change.

## Configuration

```jsonc
{
  "$schema": "./node_modules/worsier/configuration_schema.json",
  "lineWidth": 120,
  "verifyAst": true,
  "rules": {
    "imports": true
  },
  "ignorePatterns": []
}
```

Unknown keys are configuration errors. `rules.imports: false` disables both import layout and import
boundary spacing. The CLI flag `--no-verify` disables AST verification for one invocation.

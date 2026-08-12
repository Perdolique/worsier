# Worsier

Worsier is a focused JavaScript, TypeScript, JSX, and TSX formatter backed by a Rust engine.
It formats static imports and the boundaries around runtime variable declarations while preserving
the declarations themselves.
Node.js 24.0.0 or newer is required for the CLI and programmatic API.

```sh
pnpm add -D worsier
pnpm exec worsier --init
pnpm exec worsier --check .
pnpm exec worsier --write .
```

The CLI requires a nearest `worsier.jsonc` unless `--config` is provided.
`--init` creates the complete typed configuration.

```js
import { format } from 'worsier'

const output = await format('example.ts', "import{answer,type Value}from'pkg'", {
  lineWidth: 120,
  rules: {
    importLayout: true,
    statementSpacing: {
      imports: 'separate',
      variableDeclarations: 'separate'
    }
  }
})
```

The programmatic API does not discover configuration files. See
[`docs/formatting-contract.md`](docs/formatting-contract.md) for the canonical
layout and semantic guarantees, and [`docs/benchmarks.md`](docs/benchmarks.md)
for the report-only performance baseline.

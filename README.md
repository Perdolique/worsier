# Worsier

Worsier is a focused JavaScript, TypeScript, JSX, and TSX import formatter backed by a Rust engine.
It rewrites static imports and their blank-line boundaries while preserving all other source text.

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
  rules: { imports: true }
})
```

The programmatic API does not discover configuration files. See
[`docs/formatting-contract.md`](docs/formatting-contract.md) for the canonical
layout and semantic guarantees, and [`docs/benchmarks.md`](docs/benchmarks.md)
for the report-only performance baseline.

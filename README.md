# Worsier

Worsier is a focused JavaScript, TypeScript, JSX, and TSX formatter backed by a Rust engine.
It formats static imports and the boundaries around runtime variable declarations while preserving
the declarations themselves.
Node.js 24.0.0 or newer is required for the CLI and programmatic API.

```sh
pnpm add -D worsier
pnpm exec worsier --check .
pnpm exec worsier --write .
```

The CLI uses its opinionated defaults when no configuration file is present. When present, the
nearest `worsier.jsonc` overrides those defaults; `--config` selects one file explicitly, and
`--init` creates an optional complete typed configuration.

```js
import { format } from 'worsier'

const output = await format('example.ts', "import{answer,type Value}from'pkg'")
```

The optional third argument overrides the programmatic defaults. The programmatic API does not
discover configuration files. See
[`docs/formatting-contract.md`](docs/formatting-contract.md) for the canonical
layout and semantic guarantees, and [`docs/benchmarks.md`](docs/benchmarks.md)
for the report-only performance baseline.

# Worsier

A focused JavaScript, TypeScript, JSX, and TSX formatter powered by Rust. It formats static imports,
statement and member semicolons, trailing commas, and the boundaries around runtime variable
declarations while preserving source text outside the enabled rules.
Node.js 24.0.0 or newer is required.
Prebuilt Windows x64 binaries are temporarily unavailable while
[npm package publication is restored](https://github.com/Perdolique/worsier/issues/11).

```sh
pnpm add -D worsier
pnpm exec worsier --check .
pnpm exec worsier --write .
```

The CLI uses its opinionated defaults without a configuration file. An optional `worsier.jsonc`
can override them; run `pnpm exec worsier --init` to create the complete typed configuration.

```js
import { format } from 'worsier'

const output = await format('example.ts', "import{answer,type Value}from'pkg'")
```

Pass an optional third `FormatConfig` argument to override the programmatic defaults. The
programmatic API does not discover configuration files.

## Broader alternatives

[Prettier](https://prettier.io/docs/) is a mature opinionated formatter that prints the complete
source, while [Oxfmt](https://oxc.rs/docs/guide/usage/formatter.html) is a fast,
Prettier-compatible formatter with broader language and formatting coverage. Both provide selected
configuration options, including semicolon and trailing-comma modes. Worsier instead focuses on
independently configurable source-rewrite rules and preserves text outside the selected rules.

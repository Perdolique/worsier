# Worsier

A focused JavaScript, TypeScript, JSX, and TSX formatter powered by Rust. It formats static imports,
TypeScript interface layout, statement and member semicolons, trailing commas, and the boundaries
around TypeScript type aliases and runtime variable declarations while preserving source text
outside the enabled rules.
Node.js 24.0.0 or newer is required.

```sh
pnpm add -D worsier
pnpm exec worsier --check .
pnpm exec worsier --write .
```

The CLI uses its opinionated defaults without a configuration file. An optional `worsier.jsonc`
can override them; run `pnpm exec worsier --init` to create the complete typed configuration.
Run `pnpm exec worsier --update-config` to migrate a Worsier v1 configuration and expand it with
every current default. It updates `./worsier.jsonc`; pass `--config PATH` to select one other file.
Existing values and JSONC comments are preserved, and formatting never updates the file implicitly.
Only registered v1 keys are migrated; v0.1 configurations and unknown keys remain errors.
Directory discovery skips `.git`, `node_modules`, and Wrangler's generated
`worker-configuration.d.ts`. Pass an ignored file explicitly when you do want Worsier to process it.

By default, every nonempty TypeScript interface is expanded with one member per line. Set
`rules.interfaceLayout` to a member threshold from `0` through `4294967295`, or to `"off"` to
preserve interface layout. Members on the physical line governed by a preceding `// @ts-ignore`
or `// @ts-expect-error` stay together so formatting does not change TypeScript diagnostics.

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

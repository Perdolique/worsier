# Worsier

Worsier is a focused JavaScript, TypeScript, JSX, TSX, and Vue SFC script formatter powered by Rust. Each rule is independently configurable, and source outside enabled rules is preserved instead of being reprinted.

Node.js 24.0.0 or newer is required.

[Read the complete documentation](https://github.com/Perdolique/worsier#readme).

## Installation

```sh
pnpm add -D worsier
pnpm exec worsier --check .
pnpm exec worsier --write .
```

The CLI uses its defaults when no configuration file is present. Run `pnpm exec worsier --init` to create a complete typed `worsier.jsonc`.

For `.vue` files, Worsier formats inline `<script>` and `<script setup>` blocks using JavaScript, JSX, TypeScript, or TSX while preserving their existing top-level indentation. Templates, styles, custom blocks, external `src` scripts, and unknown languages remain byte-for-byte unchanged.

## Programmatic API

```js
import { format } from 'worsier'

const output = await format('example.ts', "import{answer,type Value}from'pkg'")
```

Pass an optional third `FormatConfig` argument to override the defaults. The programmatic API does not discover configuration files.

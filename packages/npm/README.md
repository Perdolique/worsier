# Worsier

A focused JavaScript, TypeScript, JSX, and TSX formatter powered by Rust. It formats static imports
and the boundaries around runtime variable declarations while preserving the declarations
themselves.
Node.js 24.0.0 or newer is required.
Prebuilt Windows x64 binaries are temporarily unavailable while
[npm package publication is restored](https://github.com/Perdolique/worsier/issues/11).

```sh
pnpm add -D worsier
pnpm exec worsier --init
pnpm exec worsier --check .
pnpm exec worsier --write .
```

```js
import { format } from 'worsier'

const output = await format('example.ts', "import{answer,type Value}from'pkg'", {})
```

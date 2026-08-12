# Worsier

A focused JavaScript, TypeScript, JSX, and TSX static-import formatter powered by Rust. Source text
outside imports and their blank-line boundaries is preserved byte-for-byte.

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

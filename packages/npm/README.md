# Worsier

A configurable JavaScript, TypeScript, JSX, and TSX formatter powered by Rust.

```sh
pnpm add -D worsier
pnpm exec worsier --init
pnpm exec worsier --check .
pnpm exec worsier --write .
```

```js
import { format } from 'worsier'

const output = await format('example.ts', 'const value={answer:42};', {})
```

# Performance benchmarks

Worsier keeps performance benchmarks report-only until a stable baseline is
established. Tagged releases run the complete Criterion suite and the Node.js
cold-start benchmark, then include the captured report in the GitHub release.

Run the benchmarks locally after building the native addon:

```sh
cargo bench -p worsier-formatter --features benchmarking -- --noplot
pnpm --filter worsier build
pnpm --filter worsier build:native
pnpm benchmark:node
```

The Criterion suite measures parsing, node/comment indexing, IR generation,
printing, formatting with `verifyAst: false`, formatting with `verifyAst: true`,
and a mixed TypeScript/TSX project. It covers small, 50 KB, and 1 MB generated
inputs. The Node.js benchmark measures API and CLI cold starts in fresh
processes, separately from Rust throughput.

No regression threshold is enforced for v0.1. A committed threshold should be
introduced only after release measurements establish a stable baseline across
the release runner.

# Performance benchmarks

Worsier keeps performance benchmarks report-only until a stable baseline is
established. Tagged releases run the complete Criterion suite and the Node.js
cold-start benchmark, then include the captured report in the GitHub release.

Run the benchmarks locally after building the native addon:

```sh
cargo bench -p worsier-formatter --features benchmarking --bench formatter -- --noplot
pnpm --filter worsier build
pnpm --filter worsier build:native
pnpm benchmark:node
```

The Criterion suite reports a single parse, parse-plus-verification, and end-to-end formatting
without verification for the complete defaults, an otherwise identical configuration with every
semicolon group set to `"off"`, and one with `trailingCommas: "off"`. It covers import-heavy small,
50 KB, and 1 MB generated inputs so each default token pass can be compared directly with its
disabled baseline. The Node.js benchmark measures API and CLI cold starts in fresh processes,
separately from Rust throughput.

No regression threshold is enforced yet. A committed threshold should be
introduced only after release measurements establish a stable baseline across
the release runner.

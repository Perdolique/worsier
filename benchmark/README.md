# Worsier benchmarks

This directory owns all benchmark-only dependencies, fixtures, runner code, detailed results, and Worsier's internal Criterion harness. Nothing here is part of the published Worsier package or its public API.

## Contents

- [What is measured](#what-is-measured)
- [Fixtures](#fixtures)
- [Configuration](#configuration)
- [Running benchmarks](#running-benchmarks)
- [Validation and reporting](#validation-and-reporting)
- [Result interpretation](#result-interpretation)

## What is measured

The comparative benchmark pins Worsier from the current workspace, Prettier `3.9.6`, and Oxfmt `0.63.0`. It runs four end-to-end CLI scenarios:

1. Format the tracked small TypeScript fixture through stdin in a fresh process.
2. Format TypeScript `parser.ts` through stdin in a fresh process.
3. Run `--write` on separate pristine copies of the Outline corpus.
4. Run `--check` on each formatter's own idempotent Outline output. This scenario is reported separately from rewrite performance.

Hyperfine performs three warmups and ten measured runs. Peak RSS is measured in five separate runs with macOS `/usr/bin/time -l` or GNU `/usr/bin/time -v`, and the report publishes the median.

Comparative tables show relative time beside each median. The fastest median in each scenario is the `1.00×` baseline, and higher values mean proportionally slower execution.

The Rust crate in `benchmark/rust` retains Worsier's Criterion microbenchmarks for exact 512 B, 50 KiB, and 1 MiB synthetic import-heavy inputs. These internal parser, rewrite, and verification measurements exclude CLI startup and are diagnostic only; they are never mixed into the formatter comparison.

## Fixtures

- `fixtures/small.ts` is a deterministic tracked micro fixture.
- TypeScript `parser.ts` comes from tag `v5.9.2`, commit `5be33469d551655d878876faa9e30aa3b49f8ee9`. The runner checks the downloaded upstream SHA-256, then normalizes its CRLF source to LF before validation or timing.
- Outline comes from commit `cdc10b45649d04e6dcfb27fb6ca0aeadd100d2bc`. The runner verifies the downloaded archive and copies only `.js`, `.jsx`, `.ts`, and `.tsx` files. Wrangler's generated `worker-configuration.d.ts` is excluded.

Third-party source is downloaded into ignored `benchmark/.work`; it is not committed to this repository. The report records fixture revisions, file counts, byte sizes, and content-manifest hashes.

## Configuration

All three tools receive explicit configs from `benchmark/config`:

- line width 120;
- semicolons disabled or removed when optional;
- trailing commas disabled;
- LF line endings;
- no cache;
- default CLI parallelism.

Worsier keeps its normal `verifyAst: true`. Prettier and Oxfmt are full-source printers while Worsier intentionally rewrites only enabled rules, so their outputs are not expected to match.

The timed commands call the installed CLI files directly. They do not include `pnpm exec` startup.

Oxfmt treats Git-ignored paths as excluded even when they are passed explicitly. Its project command therefore receives a stable `/tmp` symlink to the real copy in `benchmark/.work`; the files, filesystem, contents, and restore work remain identical, and symlink setup is outside the timed command.

## Running benchmarks

Prerequisites are the repository toolchains, network access for the pinned fixtures, `tar`, Hyperfine `1.20.0`, and `/usr/bin/time`. Install workspace dependencies first, then build and measure:

```sh
vp install
vp run benchmark
```

`vp run benchmark` builds Worsier's release native binding, performs the untimed validation pass, runs the complete comparative and Criterion suites, and writes an ignored draft to `benchmark/.work/latest.json` and `benchmark/.work/latest.md`.

To publish a replacement snapshot, first commit all infrastructure or source changes so the worktree is clean, keep the machine on AC power in its normal power mode, stop other heavy work, and run:

```sh
vp run benchmark:update
vp run benchmark:verify
```

`benchmark:update` refuses to run with a dirty worktree. It replaces `results/latest.json`, regenerates `results/latest.md`, and updates only the generated benchmark block in the root README. Review and commit those generated changes separately. The command never pushes.

For fast checks that do not measure performance:

```sh
pnpm --filter worsier-benchmark test
pnpm --filter worsier-benchmark smoke
vp run benchmark:verify
cargo bench -p worsier-benchmark --bench formatter --no-run
```

## Validation and reporting

Before timing, the runner requires every CLI invocation to exit successfully. It checks the common Outline input manifest, confirms no tool loses files, formats every fixture twice, and requires each formatter to be idempotent on its own output. Cross-tool output hashes are recorded but intentionally not compared.

`results/latest.json` is the source of truth. It contains raw timing and RSS samples, derived median/min/max/mean/stddev values, throughput, normalized reproducible commands, tool and toolchain versions, the Worsier Git SHA, fixture metadata and hashes, validation output hashes, and machine parameters. Local executable and checkout paths are represented as `<node>`, `<repo>`, and `<tmp>`. `benchmark:verify` recomputes timing statistics, throughput, RSS medians, and the complete Criterion matrix from the raw report data, then fails if either generated Markdown file drifts.

Ordinary CI runs unit tests, one real small-fixture smoke invocation per CLI, result-render verification, and a compile-only Criterion check. It never records performance measurements. Release jobs do not run benchmarks or upload benchmark result assets.

## Result interpretation

The comparison answers a narrow question: how long each CLI takes end to end on the same input and stated machine under these configs. It does not establish feature parity, formatting quality, or equivalent output. Worsier is a focused source rewriter; Prettier and Oxfmt cover broader formatting behavior.

The project-write scenario includes each tool's filesystem write semantics. Worsier now mirrors Oxfmt by overwriting changed files directly from parallel formatter jobs without temporary replacement or explicit durability synchronization, so an interrupted write can leave a partial file and successful exit does not guarantee persistence after sudden power loss.

The current snapshot is in [the detailed report](results/latest.md), with machine-readable raw data in [`results/latest.json`](results/latest.json). A later manual run replaces `latest`; Git history preserves older snapshots.

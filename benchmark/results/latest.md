# Worsier benchmark results

Snapshot generated at 2026-08-16T13:57:34.894Z from Worsier commit `3016bb11dd6a685cbccd238008b8ff309ebcd718`.

These numbers compare end-to-end CLI time on identical inputs. They do not claim equivalent formatting features or identical output between Worsier, Prettier, and Oxfmt.

## Environment

- Machine: Mac14,6, Apple M2 Max, 12 cores, 32 GB RAM
- OS: macOS 26.5.2 (25F84), arm64
- Power: AC power, normal power mode
- Toolchain: Node 24.19.0, pnpm 11.21.0, Rust 1.97.1, Cargo 1.97.1, Hyperfine 1.20.0

## Comparative results

Each timing uses 3 warmups and 10 measured Hyperfine runs. Peak RSS is the median of 5 separate runs.

Relative time normalizes each scenario to its fastest median (`1.00×`); higher values are slower.

| Scenario | Formatter | Input | Median | Relative time | Min | Max | Stddev | Throughput | Peak RSS |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Small TS stdin format | Worsier | 171 B | 36.30 ms | 1.00× | 34.52 ms | 39.28 ms | 1.51 ms | 0.00 MiB/s | 47.7 MiB |
| Small TS stdin format | Prettier | 171 B | 79.59 ms | 2.19× | 76.61 ms | 83.05 ms | 1.68 ms | 0.00 MiB/s | 70.4 MiB |
| Small TS stdin format | Oxfmt | 171 B | 103.97 ms | 2.86× | 100.81 ms | 105.79 ms | 1.29 ms | 0.00 MiB/s | 56.3 MiB |
| TypeScript parser.ts stdin format | Worsier | 516.38 KiB | 50.02 ms | 1.00× | 46.64 ms | 51.74 ms | 1.63 ms | 10.08 MiB/s | 59.5 MiB |
| TypeScript parser.ts stdin format | Prettier | 516.38 KiB | 725.12 ms | 14.50× | 712.21 ms | 742.96 ms | 8.08 ms | 0.70 MiB/s | 310.8 MiB |
| TypeScript parser.ts stdin format | Oxfmt | 516.38 KiB | 114.66 ms | 2.29× | 109.81 ms | 146.68 ms | 10.14 ms | 4.40 MiB/s | 72.6 MiB |
| Outline project write | Worsier | 9.12 MiB | 311.44 ms | 1.33× | 278.83 ms | 339.88 ms | 16.57 ms | 29.29 MiB/s | 75.3 MiB |
| Outline project write | Prettier | 9.12 MiB | 9.93 s | 42.34× | 9.60 s | 10.23 s | 194.33 ms | 0.92 MiB/s | 478.1 MiB |
| Outline project write | Oxfmt | 9.12 MiB | 234.46 ms | 1.00× | 221.95 ms | 299.10 ms | 24.38 ms | 38.90 MiB/s | 143.7 MiB |
| Outline project check on canonical output | Worsier | 9.02 MiB | 154.78 ms | 1.05× | 152.16 ms | 160.93 ms | 2.32 ms | 58.28 MiB/s | 70.3 MiB |
| Outline project check on canonical output | Prettier | 8.84 MiB | 8.53 s | 57.74× | 8.34 s | 8.75 s | 136.98 ms | 1.04 MiB/s | 453.2 MiB |
| Outline project check on canonical output | Oxfmt | 8.84 MiB | 147.75 ms | 1.00× | 145.29 ms | 152.50 ms | 2.48 ms | 59.83 MiB/s | 145.6 MiB |

## Fixtures and validation

- small: 1 file(s), 171 bytes, SHA-256 `98ef00a3eca530a6480a3aae61fb25f0a4d8a0aac8d2a0935938fe93704ea49f`
- parser: 1 file(s), 528769 bytes, SHA-256 `c882acdd1153ad25b33fe4ec7586a3a53c5de0e8d50bb80a709945a372b6a039`, revision `5be33469d551655d878876faa9e30aa3b49f8ee9`, LF line endings
- outline: 2456 file(s), 9564224 bytes, SHA-256 `ae18e7e2454676292b868c1bdcbc4e5502b149010a9f94dcdd6e1d21128769b7`, revision `cdc10b45649d04e6dcfb27fb6ca0aeadd100d2bc`

The untimed validation pass confirmed 2456 Outline source files for every tool, no lost files, successful exits, and idempotent output. Output hashes are recorded in [the JSON source](latest.json) but are intentionally not compared across formatters.

## Commands

### Small TS stdin format

- Worsier: `'<node>' '<repo>/packages/npm/bin/worsier.js' --config '<repo>/benchmark/config/worsier.jsonc' --stdin-filepath '<repo>/benchmark/fixtures/small.ts' < '<repo>/benchmark/fixtures/small.ts' > '<repo>/benchmark/.work/timed-output/small/worsier.ts'`
- Prettier: `'<node>' '<repo>/benchmark/node_modules/prettier/bin/prettier.cjs' --config '<repo>/benchmark/config/prettier.json' --ignore-path '<repo>/benchmark/config/empty-ignore' --stdin-filepath '<repo>/benchmark/fixtures/small.ts' < '<repo>/benchmark/fixtures/small.ts' > '<repo>/benchmark/.work/timed-output/small/prettier.ts'`
- Oxfmt: `'<node>' '<repo>/benchmark/node_modules/oxfmt/bin/oxfmt' --config '<repo>/benchmark/config/oxfmt.json' --ignore-path '<repo>/benchmark/config/empty-ignore' --stdin-filepath '<repo>/benchmark/fixtures/small.ts' < '<repo>/benchmark/fixtures/small.ts' > '<repo>/benchmark/.work/timed-output/small/oxfmt.ts'`

### TypeScript parser.ts stdin format

- Worsier: `'<node>' '<repo>/packages/npm/bin/worsier.js' --config '<repo>/benchmark/config/worsier.jsonc' --stdin-filepath '<repo>/benchmark/.work/fixtures/parser.ts' < '<repo>/benchmark/.work/fixtures/parser.ts' > '<repo>/benchmark/.work/timed-output/parser/worsier.ts'`
- Prettier: `'<node>' '<repo>/benchmark/node_modules/prettier/bin/prettier.cjs' --config '<repo>/benchmark/config/prettier.json' --ignore-path '<repo>/benchmark/config/empty-ignore' --stdin-filepath '<repo>/benchmark/.work/fixtures/parser.ts' < '<repo>/benchmark/.work/fixtures/parser.ts' > '<repo>/benchmark/.work/timed-output/parser/prettier.ts'`
- Oxfmt: `'<node>' '<repo>/benchmark/node_modules/oxfmt/bin/oxfmt' --config '<repo>/benchmark/config/oxfmt.json' --ignore-path '<repo>/benchmark/config/empty-ignore' --stdin-filepath '<repo>/benchmark/.work/fixtures/parser.ts' < '<repo>/benchmark/.work/fixtures/parser.ts' > '<repo>/benchmark/.work/timed-output/parser/oxfmt.ts'`

### Outline project write

- Worsier: `'<node>' '<repo>/packages/npm/bin/worsier.js' --config '<repo>/benchmark/config/worsier.jsonc' --write '<repo>/benchmark/.work/project-write/worsier'`
- Prettier: `'<node>' '<repo>/benchmark/node_modules/prettier/bin/prettier.cjs' --config '<repo>/benchmark/config/prettier.json' --ignore-path '<repo>/benchmark/config/empty-ignore' --write '<repo>/benchmark/.work/project-write/prettier'`
- Oxfmt: `'<node>' '<repo>/benchmark/node_modules/oxfmt/bin/oxfmt' --config '<repo>/benchmark/config/oxfmt.json' --ignore-path '<repo>/benchmark/config/empty-ignore' --disable-nested-config --write '<tmp>/worsier-benchmark-project-write-oxfmt'`

### Outline project check on canonical output

- Worsier: `'<node>' '<repo>/packages/npm/bin/worsier.js' --config '<repo>/benchmark/config/worsier.jsonc' --check '<repo>/benchmark/.work/validation/worsier/outline'`
- Prettier: `'<node>' '<repo>/benchmark/node_modules/prettier/bin/prettier.cjs' --config '<repo>/benchmark/config/prettier.json' --ignore-path '<repo>/benchmark/config/empty-ignore' --check '<repo>/benchmark/.work/validation/prettier/outline'`
- Oxfmt: `'<node>' '<repo>/benchmark/node_modules/oxfmt/bin/oxfmt' --config '<repo>/benchmark/config/oxfmt.json' --ignore-path '<repo>/benchmark/config/empty-ignore' --disable-nested-config --check '<tmp>/worsier-benchmark-validation-oxfmt-outline'`

## Worsier internal microbenchmarks

Criterion measures parser, rewriting, and AST verification entry points without CLI process startup. These diagnostic measurements are not comparable to the end-to-end formatter table.

| Measurement | Input | Median estimate | Throughput |
| --- | --- | ---: | ---: |
| `format_no_verify_default` | 1 MiB | 38.81 ms | 25.76 MiB/s |
| `format_no_verify_default` | 50 KiB | 1.72 ms | 28.42 MiB/s |
| `format_no_verify_default` | 512 B | 0.02 ms | 28.33 MiB/s |
| `format_no_verify_semicolons_off` | 1 MiB | 30.05 ms | 33.28 MiB/s |
| `format_no_verify_semicolons_off` | 50 KiB | 1.31 ms | 37.16 MiB/s |
| `format_no_verify_semicolons_off` | 512 B | 0.01 ms | 37.72 MiB/s |
| `format_no_verify_trailing_commas_off` | 1 MiB | 35.21 ms | 28.40 MiB/s |
| `format_no_verify_trailing_commas_off` | 50 KiB | 1.57 ms | 31.15 MiB/s |
| `format_no_verify_trailing_commas_off` | 512 B | 0.02 ms | 29.65 MiB/s |
| `parse_and_verify` | 1 MiB | 9.98 ms | 100.18 MiB/s |
| `parse_and_verify` | 50 KiB | 0.50 ms | 97.38 MiB/s |
| `parse_and_verify` | 512 B | 0.01 ms | 85.67 MiB/s |
| `single_parse` | 1 MiB | 4.63 ms | 215.89 MiB/s |
| `single_parse` | 50 KiB | 0.24 ms | 206.70 MiB/s |
| `single_parse` | 512 B | 0.00 ms | 179.38 MiB/s |

## Reproduce

See [the benchmark guide](../README.md) for prerequisites and the manual update procedure. The complete machine-readable report, including raw samples, is in [`latest.json`](latest.json).

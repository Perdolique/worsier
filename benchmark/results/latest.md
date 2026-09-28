# Worsier benchmark results

Snapshot generated at 2026-09-28T15:00:42.004Z from Worsier commit `7c10adc661b0bc003adef48dfd6b1f728c89b886`.

These numbers compare end-to-end CLI time on identical inputs. They do not claim equivalent formatting features or identical output between Worsier, Prettier, and Oxfmt.

## Environment

- Machine: Mac14,6, Apple M2 Max, 12 cores, 32 GB RAM
- OS: macOS 26.5.2 (25F84), arm64
- Power: AC power, normal power mode
- Toolchain: Node 24.20.0, pnpm 11.24.0, Rust 1.98.0, Cargo 1.98.0, Hyperfine 1.20.0

## Comparative results

Each timing uses 3 warmups and 10 measured Hyperfine runs. Peak RSS is the median of 5 separate runs.

Relative time normalizes each scenario to its fastest median (`1.00×`); higher values are slower.

| Scenario | Formatter | Input | Median | Relative time | Min | Max | Stddev | Throughput | Peak RSS |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Small TS stdin format | Worsier | 171 B | 32.79 ms | 1.00× | 29.55 ms | 47.13 ms | 4.60 ms | 0.00 MiB/s | 47.9 MiB |
| Small TS stdin format | Prettier | 171 B | 76.17 ms | 2.32× | 71.57 ms | 104.25 ms | 8.86 ms | 0.00 MiB/s | 69.8 MiB |
| Small TS stdin format | Oxfmt | 171 B | 94.70 ms | 2.89× | 89.37 ms | 99.16 ms | 2.73 ms | 0.00 MiB/s | 56.1 MiB |
| TypeScript parser.ts stdin format | Worsier | 516.38 KiB | 47.67 ms | 1.00× | 46.66 ms | 50.03 ms | 0.91 ms | 10.58 MiB/s | 64.5 MiB |
| TypeScript parser.ts stdin format | Prettier | 516.38 KiB | 740.21 ms | 15.53× | 713.12 ms | 803.21 ms | 28.05 ms | 0.68 MiB/s | 327.5 MiB |
| TypeScript parser.ts stdin format | Oxfmt | 516.38 KiB | 106.09 ms | 2.23× | 102.54 ms | 114.78 ms | 4.34 ms | 4.75 MiB/s | 72.4 MiB |
| Outline project write | Worsier | 9.12 MiB | 306.69 ms | 1.25× | 291.14 ms | 337.40 ms | 15.93 ms | 29.74 MiB/s | 88.5 MiB |
| Outline project write | Prettier | 9.12 MiB | 10.08 s | 41.23× | 9.31 s | 11.72 s | 706.21 ms | 0.91 MiB/s | 473.0 MiB |
| Outline project write | Oxfmt | 9.12 MiB | 244.42 ms | 1.00× | 220.35 ms | 261.56 ms | 12.78 ms | 37.32 MiB/s | 145.3 MiB |
| Outline project check on canonical output | Worsier | 9.09 MiB | 197.48 ms | 1.35× | 191.42 ms | 203.50 ms | 3.15 ms | 46.02 MiB/s | 73.7 MiB |
| Outline project check on canonical output | Prettier | 8.84 MiB | 9.06 s | 61.87× | 8.74 s | 9.50 s | 197.76 ms | 0.98 MiB/s | 539.0 MiB |
| Outline project check on canonical output | Oxfmt | 8.84 MiB | 146.48 ms | 1.00× | 140.96 ms | 155.43 ms | 3.80 ms | 60.35 MiB/s | 143.7 MiB |

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
| `format_no_verify_default` | 1 MiB | 58.39 ms | 17.13 MiB/s |
| `format_no_verify_default` | 50 KiB | 2.47 ms | 19.76 MiB/s |
| `format_no_verify_default` | 512 B | 0.03 ms | 19.08 MiB/s |
| `format_no_verify_semicolons_off` | 1 MiB | 44.84 ms | 22.30 MiB/s |
| `format_no_verify_semicolons_off` | 50 KiB | 1.99 ms | 24.54 MiB/s |
| `format_no_verify_semicolons_off` | 512 B | 0.02 ms | 24.32 MiB/s |
| `format_no_verify_trailing_commas_off` | 1 MiB | 50.76 ms | 19.70 MiB/s |
| `format_no_verify_trailing_commas_off` | 50 KiB | 2.32 ms | 21.09 MiB/s |
| `format_no_verify_trailing_commas_off` | 512 B | 0.02 ms | 19.90 MiB/s |
| `parse_and_verify` | 1 MiB | 10.21 ms | 97.98 MiB/s |
| `parse_and_verify` | 50 KiB | 0.51 ms | 96.60 MiB/s |
| `parse_and_verify` | 512 B | 0.01 ms | 85.21 MiB/s |
| `single_parse` | 1 MiB | 5.02 ms | 199.15 MiB/s |
| `single_parse` | 50 KiB | 0.24 ms | 202.24 MiB/s |
| `single_parse` | 512 B | 0.00 ms | 167.78 MiB/s |

## Reproduce

See [the benchmark guide](../README.md) for prerequisites and the manual update procedure. The complete machine-readable report, including raw samples, is in [`latest.json`](latest.json).

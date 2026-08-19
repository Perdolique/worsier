# Worsier benchmark results

Snapshot generated at 2026-08-19T11:54:31.433Z from Worsier commit `a467d604430069afa701315a9b6f0cd2fd4b5b8d`.

These numbers compare end-to-end CLI time on identical inputs. They do not claim equivalent formatting features or identical output between Worsier, Prettier, and Oxfmt.

## Environment

- Machine: Mac14,6, Apple M2 Max, 12 cores, 32 GB RAM
- OS: macOS 26.5.2 (25F84), arm64
- Power: AC power, normal power mode
- Toolchain: Node 24.19.0, pnpm 11.22.0, Rust 1.97.1, Cargo 1.97.1, Hyperfine 1.20.0

## Comparative results

Each timing uses 3 warmups and 10 measured Hyperfine runs. Peak RSS is the median of 5 separate runs.

Relative time normalizes each scenario to its fastest median (`1.00×`); higher values are slower.

| Scenario | Formatter | Input | Median | Relative time | Min | Max | Stddev | Throughput | Peak RSS |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Small TS stdin format | Worsier | 171 B | 35.86 ms | 1.00× | 34.42 ms | 40.12 ms | 1.91 ms | 0.00 MiB/s | 47.6 MiB |
| Small TS stdin format | Prettier | 171 B | 77.01 ms | 2.15× | 75.59 ms | 80.85 ms | 1.46 ms | 0.00 MiB/s | 70.3 MiB |
| Small TS stdin format | Oxfmt | 171 B | 99.98 ms | 2.79× | 94.18 ms | 104.47 ms | 3.21 ms | 0.00 MiB/s | 56.1 MiB |
| TypeScript parser.ts stdin format | Worsier | 516.38 KiB | 49.37 ms | 1.00× | 47.58 ms | 82.66 ms | 13.27 ms | 10.22 MiB/s | 60.0 MiB |
| TypeScript parser.ts stdin format | Prettier | 516.38 KiB | 718.00 ms | 14.54× | 704.70 ms | 754.08 ms | 14.54 ms | 0.70 MiB/s | 322.7 MiB |
| TypeScript parser.ts stdin format | Oxfmt | 516.38 KiB | 109.66 ms | 2.22× | 108.46 ms | 117.67 ms | 2.92 ms | 4.60 MiB/s | 72.5 MiB |
| Outline project write | Worsier | 9.12 MiB | 286.39 ms | 1.16× | 271.88 ms | 337.40 ms | 20.21 ms | 31.85 MiB/s | 76.3 MiB |
| Outline project write | Prettier | 9.12 MiB | 9.75 s | 39.51× | 9.11 s | 10.21 s | 344.07 ms | 0.94 MiB/s | 629.2 MiB |
| Outline project write | Oxfmt | 9.12 MiB | 246.74 ms | 1.00× | 233.78 ms | 271.23 ms | 13.06 ms | 36.97 MiB/s | 142.8 MiB |
| Outline project check on canonical output | Worsier | 9.08 MiB | 166.83 ms | 1.15× | 163.93 ms | 172.33 ms | 2.60 ms | 54.45 MiB/s | 70.5 MiB |
| Outline project check on canonical output | Prettier | 8.84 MiB | 8.79 s | 60.37× | 8.40 s | 9.80 s | 448.69 ms | 1.01 MiB/s | 469.9 MiB |
| Outline project check on canonical output | Oxfmt | 8.84 MiB | 145.62 ms | 1.00× | 142.09 ms | 158.95 ms | 4.70 ms | 60.71 MiB/s | 145.1 MiB |

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
| `format_no_verify_default` | 1 MiB | 49.82 ms | 20.07 MiB/s |
| `format_no_verify_default` | 50 KiB | 2.33 ms | 20.93 MiB/s |
| `format_no_verify_default` | 512 B | 0.02 ms | 21.00 MiB/s |
| `format_no_verify_semicolons_off` | 1 MiB | 39.21 ms | 25.50 MiB/s |
| `format_no_verify_semicolons_off` | 50 KiB | 1.79 ms | 27.32 MiB/s |
| `format_no_verify_semicolons_off` | 512 B | 0.02 ms | 27.40 MiB/s |
| `format_no_verify_trailing_commas_off` | 1 MiB | 45.61 ms | 21.93 MiB/s |
| `format_no_verify_trailing_commas_off` | 50 KiB | 2.12 ms | 23.07 MiB/s |
| `format_no_verify_trailing_commas_off` | 512 B | 0.02 ms | 21.78 MiB/s |
| `parse_and_verify` | 1 MiB | 9.80 ms | 102.04 MiB/s |
| `parse_and_verify` | 50 KiB | 0.50 ms | 97.64 MiB/s |
| `parse_and_verify` | 512 B | 0.01 ms | 86.14 MiB/s |
| `single_parse` | 1 MiB | 4.62 ms | 216.46 MiB/s |
| `single_parse` | 50 KiB | 0.24 ms | 202.04 MiB/s |
| `single_parse` | 512 B | 0.00 ms | 176.04 MiB/s |

## Reproduce

See [the benchmark guide](../README.md) for prerequisites and the manual update procedure. The complete machine-readable report, including raw samples, is in [`latest.json`](latest.json).

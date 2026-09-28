# Worsier benchmark results

Snapshot generated at 2026-09-28T16:03:16.148Z from Worsier commit `f61963644aa0a4ac59ecbb8a4c4576ce97e5871a`.

These numbers compare end-to-end CLI time on identical inputs. They do not claim equivalent formatting features or identical output between Worsier, Prettier, and Oxfmt.

## Environment

- Machine: Mac14,6, Apple M2 Max, 12 cores, 32 GB RAM
- OS: macOS 26.5.2 (25F84), arm64
- Power: AC power, normal power mode
- Toolchain: Node 24.21.0, pnpm 12.6.0, Rust 1.98.1, Cargo 1.98.1, Hyperfine 1.20.0

## Comparative results

Each timing uses 3 warmups and 10 measured Hyperfine runs. Peak RSS is the median of 5 separate runs.

Relative time normalizes each scenario to its fastest median (`1.00×`); higher values are slower.

| Scenario | Formatter | Input | Median | Relative time | Min | Max | Stddev | Throughput | Peak RSS |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Small TS stdin format | Worsier | 171 B | 30.10 ms | 1.00× | 27.79 ms | 31.81 ms | 1.16 ms | 0.01 MiB/s | 47.9 MiB |
| Small TS stdin format | Prettier | 171 B | 71.44 ms | 2.37× | 68.27 ms | 74.92 ms | 2.34 ms | 0.00 MiB/s | 69.7 MiB |
| Small TS stdin format | Oxfmt | 171 B | 38.35 ms | 1.27× | 35.63 ms | 39.26 ms | 1.00 ms | 0.00 MiB/s | 56.2 MiB |
| TypeScript parser.ts stdin format | Worsier | 516.38 KiB | 44.91 ms | 1.00× | 43.99 ms | 50.47 ms | 1.80 ms | 11.23 MiB/s | 64.2 MiB |
| TypeScript parser.ts stdin format | Prettier | 516.38 KiB | 722.78 ms | 16.09× | 705.71 ms | 764.10 ms | 17.30 ms | 0.70 MiB/s | 327.0 MiB |
| TypeScript parser.ts stdin format | Oxfmt | 516.38 KiB | 48.65 ms | 1.08× | 46.64 ms | 50.18 ms | 0.94 ms | 10.36 MiB/s | 72.5 MiB |
| Outline project write | Worsier | 9.12 MiB | 319.58 ms | 1.67× | 303.55 ms | 548.32 ms | 69.01 ms | 28.54 MiB/s | 91.0 MiB |
| Outline project write | Prettier | 9.12 MiB | 9.78 s | 51.05× | 9.20 s | 10.70 s | 463.24 ms | 0.93 MiB/s | 516.4 MiB |
| Outline project write | Oxfmt | 9.12 MiB | 191.57 ms | 1.00× | 163.93 ms | 211.89 ms | 13.53 ms | 47.61 MiB/s | 143.1 MiB |
| Outline project check on canonical output | Worsier | 9.09 MiB | 192.39 ms | 2.06× | 185.63 ms | 215.01 ms | 10.38 ms | 47.24 MiB/s | 73.1 MiB |
| Outline project check on canonical output | Prettier | 8.84 MiB | 8.91 s | 95.26× | 8.77 s | 9.15 s | 127.83 ms | 0.99 MiB/s | 485.0 MiB |
| Outline project check on canonical output | Oxfmt | 8.84 MiB | 93.58 ms | 1.00× | 90.02 ms | 128.89 ms | 10.96 ms | 94.46 MiB/s | 145.6 MiB |

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
| `format_no_verify_default` | 1 MiB | 51.52 ms | 19.41 MiB/s |
| `format_no_verify_default` | 50 KiB | 2.34 ms | 20.90 MiB/s |
| `format_no_verify_default` | 512 B | 0.02 ms | 22.26 MiB/s |
| `format_no_verify_semicolons_off` | 1 MiB | 41.49 ms | 24.10 MiB/s |
| `format_no_verify_semicolons_off` | 50 KiB | 1.83 ms | 26.68 MiB/s |
| `format_no_verify_semicolons_off` | 512 B | 0.02 ms | 29.43 MiB/s |
| `format_no_verify_trailing_commas_off` | 1 MiB | 49.38 ms | 20.25 MiB/s |
| `format_no_verify_trailing_commas_off` | 50 KiB | 2.13 ms | 22.88 MiB/s |
| `format_no_verify_trailing_commas_off` | 512 B | 0.02 ms | 23.20 MiB/s |
| `parse_and_verify` | 1 MiB | 10.02 ms | 99.84 MiB/s |
| `parse_and_verify` | 50 KiB | 0.52 ms | 94.20 MiB/s |
| `parse_and_verify` | 512 B | 0.01 ms | 83.83 MiB/s |
| `single_parse` | 1 MiB | 4.73 ms | 211.43 MiB/s |
| `single_parse` | 50 KiB | 0.24 ms | 201.93 MiB/s |
| `single_parse` | 512 B | 0.00 ms | 170.75 MiB/s |

## Reproduce

See [the benchmark guide](../README.md) for prerequisites and the manual update procedure. The complete machine-readable report, including raw samples, is in [`latest.json`](latest.json).

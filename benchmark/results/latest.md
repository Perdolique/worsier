# Worsier benchmark results

Snapshot generated at 2026-11-24T14:38:15.770Z from Worsier commit `c345e3fd8dbb7afb15cb4861a86df00eb7cd50ad`.

These numbers compare end-to-end CLI time on identical inputs. They do not claim equivalent formatting features or identical output between Worsier, Prettier, and Oxfmt.

## Environment

- Machine: Mac14,6, Apple M2 Max, 12 cores, 32 GB RAM
- OS: macOS 27.0.1 (26A434), arm64
- Power: AC power, normal power mode
- Toolchain: Node 26.13.1, pnpm 12.23.0, Rust 1.100.0, Cargo 1.100.0, Hyperfine 2.0.0

## Comparative results

Each timing uses 3 warmups and 10 measured Hyperfine runs. Peak RSS is the median from the same measured runs.

Relative time normalizes each scenario to its fastest median (`1.00×`); higher values are slower.

| Scenario | Formatter | Input | Median | Relative time | Min | Max | Stddev | Throughput | Peak RSS |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Small TS stdin format | Worsier | 171 B | 28.05 ms | 1.00× | 27.47 ms | 29.55 ms | 0.69 ms | 0.01 MiB/s | 53.8 MiB |
| Small TS stdin format | Prettier | 171 B | 63.82 ms | 2.28× | 62.96 ms | 68.47 ms | 1.54 ms | 0.00 MiB/s | 75.0 MiB |
| Small TS stdin format | Oxfmt | 171 B | 36.09 ms | 1.29× | 34.86 ms | 41.72 ms | 2.51 ms | 0.00 MiB/s | 62.2 MiB |
| TypeScript parser.ts stdin format | Worsier | 516.38 KiB | 72.75 ms | 1.67× | 70.58 ms | 80.42 ms | 2.91 ms | 6.93 MiB/s | 83.0 MiB |
| TypeScript parser.ts stdin format | Prettier | 516.38 KiB | 412.05 ms | 9.43× | 400.53 ms | 435.54 ms | 9.66 ms | 1.22 MiB/s | 239.4 MiB |
| TypeScript parser.ts stdin format | Oxfmt | 516.38 KiB | 43.68 ms | 1.00× | 42.69 ms | 45.15 ms | 0.86 ms | 11.55 MiB/s | 69.7 MiB |
| Outline project write | Worsier | 9.12 MiB | 241.03 ms | 1.58× | 230.97 ms | 285.77 ms | 16.06 ms | 37.84 MiB/s | 97.5 MiB |
| Outline project write | Prettier | 9.12 MiB | 8.21 s | 53.73× | 7.79 s | 9.44 s | 447.09 ms | 1.11 MiB/s | 479.0 MiB |
| Outline project write | Oxfmt | 9.12 MiB | 152.84 ms | 1.00× | 149.63 ms | 163.68 ms | 5.22 ms | 59.68 MiB/s | 108.1 MiB |
| Outline project check on canonical output | Worsier | 9.09 MiB | 124.86 ms | 1.39× | 119.94 ms | 183.02 ms | 17.76 ms | 72.79 MiB/s | 79.1 MiB |
| Outline project check on canonical output | Prettier | 8.84 MiB | 7.46 s | 82.83× | 7.29 s | 8.35 s | 356.99 ms | 1.18 MiB/s | 467.0 MiB |
| Outline project check on canonical output | Oxfmt | 8.84 MiB | 90.10 ms | 1.00× | 86.72 ms | 97.16 ms | 3.20 ms | 98.11 MiB/s | 108.6 MiB |

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
| `format_no_verify_default` | 1 MiB | 48.61 ms | 20.57 MiB/s |
| `format_no_verify_default` | 50 KiB | 2.06 ms | 23.74 MiB/s |
| `format_no_verify_default` | 512 B | 0.02 ms | 23.92 MiB/s |
| `format_no_verify_semicolons_off` | 1 MiB | 39.57 ms | 25.27 MiB/s |
| `format_no_verify_semicolons_off` | 50 KiB | 1.71 ms | 28.60 MiB/s |
| `format_no_verify_semicolons_off` | 512 B | 0.02 ms | 31.00 MiB/s |
| `format_no_verify_trailing_commas_off` | 1 MiB | 43.73 ms | 22.87 MiB/s |
| `format_no_verify_trailing_commas_off` | 50 KiB | 1.90 ms | 25.67 MiB/s |
| `format_no_verify_trailing_commas_off` | 512 B | 0.02 ms | 25.32 MiB/s |
| `parse_and_verify` | 1 MiB | 6.03 ms | 165.77 MiB/s |
| `parse_and_verify` | 50 KiB | 0.30 ms | 160.75 MiB/s |
| `parse_and_verify` | 512 B | 0.00 ms | 128.77 MiB/s |
| `single_parse` | 1 MiB | 2.74 ms | 365.33 MiB/s |
| `single_parse` | 50 KiB | 0.14 ms | 361.15 MiB/s |
| `single_parse` | 512 B | 0.00 ms | 275.49 MiB/s |

## Reproduce

See [the benchmark guide](../README.md) for prerequisites and the manual update procedure. The complete machine-readable report, including raw samples, is in [`latest.json`](latest.json).

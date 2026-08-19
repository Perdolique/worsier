import { createHash } from 'node:crypto'
import { readFile, readdir, stat } from 'node:fs/promises'
import { relative, resolve } from 'node:path'

import {
  scenarioNames,
  toolNames,
  type BenchmarkReport,
  type BenchmarkScenario,
  type FileDescription,
  type HashedSourceManifest,
  type SourceManifest,
  type Statistics,
  type ToolName
} from './types.mts'

export const README_START = '<!-- benchmark-results:start -->'
export const README_END = '<!-- benchmark-results:end -->'

export function calculateStatistics(samples: number[]): Statistics {
  if (!Array.isArray(samples) || samples.length === 0 || samples.some((sample) => !Number.isFinite(sample))) {
    throw new Error('Statistics require at least one finite sample')
  }

  const sorted = [...samples].sort((left, right) => left - right)
  const mean = sorted.reduce((total, sample) => total + sample, 0) / sorted.length
  const variance = sorted.reduce((total, sample) => total + (sample - mean) ** 2, 0) / sorted.length

  return {
    samples: [...samples],
    median: percentile(sorted, 0.5),
    min: sorted[0],
    max: sorted[sorted.length - 1],
    mean,
    stddev: Math.sqrt(variance)
  }
}

export function parseHyperfineJson(text: string): Record<string, Statistics> {
  const document = JSON.parse(text) as unknown
  if (!isRecord(document) || !Array.isArray(document.results)) {
    throw new Error('Hyperfine output does not contain a results array')
  }

  return Object.fromEntries(document.results.map((result) => {
    if (
      !isRecord(result)
      || typeof result.command !== 'string'
      || !Array.isArray(result.times)
      || result.times.some((sample) => typeof sample !== 'number')
    ) {
      throw new Error('Hyperfine result is missing its command or samples')
    }

    const name = typeof result.command_name === 'string' ? result.command_name : result.command
    return [name, calculateStatistics(result.times)]
  }))
}

export function parsePeakRss(text: string, platform: NodeJS.Platform = process.platform): number {
  if (platform === 'darwin') {
    const match = text.match(/(\d+)\s+maximum resident set size/)
    if (!match) {
      throw new Error('macOS time output does not contain maximum resident set size')
    }
    return Number.parseInt(match[1], 10)
  }

  const match = text.match(/Maximum resident set size \(kbytes\):\s*(\d+)/)
  if (!match) {
    throw new Error('GNU time output does not contain maximum resident set size')
  }
  return Number.parseInt(match[1], 10) * 1024
}

export function normalizeLineEndings(text: string): string {
  return text.replaceAll('\r\n', '\n').replaceAll('\r', '\n')
}

export function replaceGeneratedBlock(readme: string, generatedMarkdown: string): string {
  const startIndex = readme.indexOf(README_START)
  const endIndex = readme.indexOf(README_END)
  if (startIndex === -1 || endIndex === -1 || endIndex < startIndex) {
    throw new Error('README benchmark result markers are missing or invalid')
  }
  if (readme.indexOf(README_START, startIndex + README_START.length) !== -1 || readme.indexOf(README_END, endIndex + README_END.length) !== -1) {
    throw new Error('README must contain exactly one benchmark result block')
  }

  const before = readme.slice(0, startIndex + README_START.length)
  const after = readme.slice(endIndex)
  return `${before}\n${generatedMarkdown.trim()}\n${after}`
}

export async function hashFile(path: string): Promise<string> {
  const contents = await readFile(path)
  return createHash('sha256').update(contents).digest('hex')
}

export async function describeFixture(path: string): Promise<FileDescription> {
  const pathStat = await stat(path)
  if (pathStat.isFile()) {
    return {
      files: 1,
      bytes: pathStat.size,
      sha256: await hashFile(path)
    }
  }

  const entries = await sourceFiles(path)
  const hash = createHash('sha256')
  let bytes = 0
  for (const entry of entries) {
    const contents = await readFile(entry)
    const name = relative(path, entry).split('\\').join('/')
    hash.update(name)
    hash.update('\0')
    hash.update(contents)
    hash.update('\0')
    bytes += contents.length
  }

  return {
    files: entries.length,
    bytes,
    sha256: hash.digest('hex')
  }
}

export async function describeManifest(directory: string): Promise<HashedSourceManifest> {
  const paths = (await sourceFiles(directory)).map((entry) => relative(directory, entry).split('\\').join('/'))
  const hash = createHash('sha256')
  for (const path of paths) {
    hash.update(path)
    hash.update('\0')
  }
  return { paths, sha256: hash.digest('hex') }
}

export function assertSameManifest(expected: SourceManifest, actual: SourceManifest, label = 'Formatted output'): void {
  const expectedPaths = new Set(expected.paths)
  const actualPaths = new Set(actual.paths)
  const missing = expected.paths.filter((path) => !actualPaths.has(path))
  const added = actual.paths.filter((path) => !expectedPaths.has(path))
  if (missing.length === 0 && added.length === 0) {
    return
  }

  const summarize = (paths: string[]): string => paths.length === 0 ? 'none' : paths.slice(0, 5).join(', ')
  throw new Error(`${label} changed the source manifest; missing: ${summarize(missing)}; added: ${summarize(added)}`)
}

export async function sourceFiles(directory: string): Promise<string[]> {
  const found: string[] = []
  await visit(resolve(directory), found)
  return found.sort()
}

export function shellQuote(value: unknown): string {
  return `'${String(value).replaceAll("'", "'\\''")}'`
}

export function formatMilliseconds(seconds: number): string {
  const milliseconds = seconds * 1000
  if (milliseconds >= 1000) {
    return `${seconds.toFixed(2)} s`
  }
  return `${milliseconds.toFixed(2)} ms`
}

export function formatMebibytes(bytes: number): string {
  return `${(bytes / 1024 / 1024).toFixed(1)} MiB`
}

export function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024) {
    return `${(bytes / 1024 / 1024).toFixed(bytes % (1024 * 1024) === 0 ? 0 : 2)} MiB`
  }
  if (bytes >= 1024) {
    return `${(bytes / 1024).toFixed(bytes % 1024 === 0 ? 0 : 2)} KiB`
  }
  return `${bytes} B`
}

export function formatRelativeTime(seconds: number, baselineSeconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0 || !Number.isFinite(baselineSeconds) || baselineSeconds <= 0) {
    throw new Error('Relative benchmark times must be positive finite numbers')
  }
  return `${(seconds / baselineSeconds).toFixed(2)}×`
}

export function buildRootBenchmarkBlock(report: BenchmarkReport): string {
  const rows = toolNames.map((tool) => {
    const version = report.tools[tool].version
    const small = formatRootTiming(report.scenarios.small, tool)
    const parser = formatRootTiming(report.scenarios.parser, tool)
    const project = formatRootTiming(report.scenarios.projectWrite, tool)
    const rss = formatMebibytes(report.scenarios.projectWrite.results[tool].peakRssBytes)
    return `| ${report.tools[tool].displayName} ${version} | ${small} | ${parser} | ${project} | ${rss} |`
  })
  const environment = report.environment

  return [
    'The latest manual snapshot compares end-to-end CLI time on identical inputs, not feature or output equivalence.',
    '',
    'Relative time normalizes each scenario to its fastest median (`1.00×`); higher values are slower.',
    '',
    '| Formatter | Small TS | TypeScript `parser.ts` | Outline project write | Project peak RSS |',
    '| --- | ---: | ---: | ---: | ---: |',
    ...rows,
    '',
    `Environment: ${environment.machineModel}, ${environment.cpu}, ${environment.cores} cores, ${environment.memoryGb} GB RAM, ${environment.osName} ${environment.osVersion} ${environment.architecture}, Node ${environment.node}.`,
    '',
    '[Methodology, commands, raw samples, and diagnostic microbenchmarks](benchmark/results/latest.md).'
  ].join('\n')
}

export function buildDetailedReport(report: BenchmarkReport): string {
  const lines = [
    '# Worsier benchmark results',
    '',
    `Snapshot generated at ${report.generatedAt} from Worsier commit \`${report.source.worsierSha}\`.`,
    '',
    'These numbers compare end-to-end CLI time on identical inputs. They do not claim equivalent formatting features or identical output between Worsier, Prettier, and Oxfmt.',
    '',
    '## Environment',
    '',
    `- Machine: ${report.environment.machineModel}, ${report.environment.cpu}, ${report.environment.cores} cores, ${report.environment.memoryGb} GB RAM`,
    `- OS: ${report.environment.osName} ${report.environment.osVersion} (${report.environment.osBuild}), ${report.environment.architecture}`,
    `- Power: ${report.environment.powerSource}, ${report.environment.powerMode}`,
    `- Toolchain: Node ${report.environment.node}, pnpm ${report.environment.pnpm}, Rust ${report.environment.rust}, Cargo ${report.environment.cargo}, Hyperfine ${report.environment.hyperfine}`,
    '',
    '## Comparative results',
    '',
    `Each timing uses ${report.settings.warmups} warmups and ${report.settings.runs} measured Hyperfine runs. Peak RSS is the median of ${report.settings.rssRuns} separate runs.`,
    '',
    'Relative time normalizes each scenario to its fastest median (`1.00×`); higher values are slower.',
    '',
    '| Scenario | Formatter | Input | Median | Relative time | Min | Max | Stddev | Throughput | Peak RSS |',
    '| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |'
  ]

  for (const scenarioName of scenarioNames) {
    const scenario = report.scenarios[scenarioName]
    const baseline = fastestMedianSeconds(scenario)
    for (const tool of toolNames) {
      const result = scenario.results[tool]
      const throughput = result.throughputMibPerSecond === null ? '—' : `${result.throughputMibPerSecond.toFixed(2)} MiB/s`
      lines.push(`| ${scenario.displayName} | ${report.tools[tool].displayName} | ${formatBytes(result.inputBytes)} | ${formatMilliseconds(result.medianSeconds)} | ${formatRelativeTime(result.medianSeconds, baseline)} | ${formatMilliseconds(result.minSeconds)} | ${formatMilliseconds(result.maxSeconds)} | ${formatMilliseconds(result.stddevSeconds)} | ${throughput} | ${formatMebibytes(result.peakRssBytes)} |`)
    }
  }

  lines.push('', '## Fixtures and validation', '')
  for (const [name, fixture] of Object.entries(report.fixtures)) {
    const revision = fixture.revision ? `, revision \`${fixture.revision}\`` : ''
    const lineEndings = fixture.lineEndings ? `, ${fixture.lineEndings.toUpperCase()} line endings` : ''
    lines.push(`- ${name}: ${fixture.files} file(s), ${fixture.bytes} bytes, SHA-256 \`${fixture.sha256}\`${revision}${lineEndings}`)
  }
  lines.push('', `The untimed validation pass confirmed ${report.validation.fileCount} Outline source files for every tool, no lost files, successful exits, and idempotent output. Output hashes are recorded in [the JSON source](latest.json) but are intentionally not compared across formatters.`, '', '## Commands', '')
  for (const [scenarioName, scenario] of Object.entries(report.scenarios)) {
    lines.push(`### ${scenario.displayName}`, '')
    for (const tool of toolNames) {
      lines.push(`- ${report.tools[tool].displayName}: \`${scenario.results[tool].command}\``)
    }
    lines.push('')
  }

  lines.push('## Worsier internal microbenchmarks', '', 'Criterion measures parser, rewriting, and AST verification entry points without CLI process startup. These diagnostic measurements are not comparable to the end-to-end formatter table.', '', '| Measurement | Input | Median estimate | Throughput |', '| --- | --- | ---: | ---: |')
  for (const benchmark of report.microbenchmarks) {
    const throughput = benchmark.throughputMibPerSecond === null ? '—' : `${benchmark.throughputMibPerSecond.toFixed(2)} MiB/s`
    lines.push(`| \`${benchmark.measurement}\` | ${formatBytes(benchmark.inputBytes)} | ${formatMilliseconds(benchmark.medianSeconds)} | ${throughput} |`)
  }
  lines.push('', '## Reproduce', '', 'See [the benchmark guide](../README.md) for prerequisites and the manual update procedure. The complete machine-readable report, including raw samples, is in [`latest.json`](latest.json).')
  return lines.join('\n')
}

function formatRootTiming(scenario: BenchmarkScenario, tool: ToolName): string {
  const median = scenario.results[tool].medianSeconds
  return `${formatMilliseconds(median)} (${formatRelativeTime(median, fastestMedianSeconds(scenario))})`
}

function fastestMedianSeconds(scenario: BenchmarkScenario): number {
  return Math.min(...Object.values(scenario.results).map((result) => result.medianSeconds))
}

function percentile(sorted: number[], quantile: number): number {
  const position = (sorted.length - 1) * quantile
  const lower = Math.floor(position)
  const upper = Math.ceil(position)
  const fraction = position - lower
  return sorted[lower] + (sorted[upper] - sorted[lower]) * fraction
}

async function visit(directory: string, found: string[]): Promise<void> {
  const entries = await readdir(directory, { withFileTypes: true })
  for (const entry of entries) {
    const path = resolve(directory, entry.name)
    if (entry.isDirectory()) {
      await visit(path, found)
    } else if (entry.isFile()) {
      found.push(path)
    }
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

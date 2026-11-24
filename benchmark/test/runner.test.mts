import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'

import {
  README_END,
  README_START,
  assertSameManifest,
  buildDetailedReport,
  buildRootBenchmarkBlock,
  calculateStatistics,
  formatBytes,
  formatRelativeTime,
  normalizeLineEndings,
  parseHyperfineJson,
  replaceGeneratedBlock,
  shellQuote
} from '../src/lib.mts'
import {
  assertBenchmarkSemicolonsDisabled,
  buildCommands,
  buildHyperfineArgs,
  collectEnvironment,
  commandFailure,
  displayCommand,
  fixturePins,
  loadBenchmarkSettings,
  measurementSettings,
  pinnedToolVersions,
  validateReport
} from '../src/runner.mts'
import type {
  BenchmarkPackage,
  BenchmarkReport,
  BenchmarkResult,
  BenchmarkScenario,
  CaptureCommand,
  MicrobenchmarkResult,
  ScenarioDefinition,
  WorsierSemicolonCheckConfig
} from '../src/types.mts'

test('calculates statistics from raw samples', () => {
  assert.deepEqual(calculateStatistics([4, 1, 3, 2]), {
    samples: [4, 1, 3, 2],
    median: 2.5,
    min: 1,
    max: 4,
    mean: 2.5,
    stddev: Math.sqrt(1.25)
  })
  assert.throws(() => calculateStatistics([]), /at least one finite sample/)
})

test('parses named Hyperfine 2 results with paired wall time and RSS samples', () => {
  const measurements = [[0.3, 3072], [0.1, 1024], [0.2, 2048]].map(([seconds, bytes]) => ({
    time_wall_clock: { value: seconds, unit: 'second' },
    memory_peak_resident: { value: bytes, unit: 'byte' },
    exit_code: 0
  }))
  const report = {
    schema_version: 2,
    results: [{
      command: 'node tool.js',
      name: 'tool',
      measurements
    }]
  }
  const json = JSON.stringify(report)
  const results = parseHyperfineJson(json)
  const result = results.get('tool')
  assert.deepEqual(result?.timing.samples, [0.3, 0.1, 0.2])
  assert.deepEqual(result?.rss.samples, [3072, 1024, 2048])
  assert.equal(result?.rss.median, 2048)

  report.results[0].measurements[0].memory_peak_resident.unit = 'kibibyte'
  const invalidJson = JSON.stringify(report)
  assert.throws(() => parseHyperfineJson(invalidJson), /invalid Hyperfine peak RSS/)
})

test('normalizes third-party fixture line endings to LF', () => {
  assert.equal(normalizeLineEndings('first\r\nsecond\rthird\n'), 'first\nsecond\nthird\n')
})

test('formats exact byte sizes for benchmark reports', () => {
  assert.equal(formatBytes(512), '512 B')
  assert.equal(formatBytes(50 * 1024), '50 KiB')
  assert.equal(formatBytes(1024 * 1024), '1 MiB')
})

test('formats relative time against a positive baseline', () => {
  assert.equal(formatRelativeTime(0.01, 0.01), '1.00×')
  assert.equal(formatRelativeTime(0.025, 0.01), '2.50×')
  assert.throws(() => formatRelativeTime(0.01, 0), /positive finite numbers/)
})

test('rejects changed source manifests even when file counts match', () => {
  const baseline = { paths: ['a.ts', 'b.ts'] }
  assert.doesNotThrow(() => assertSameManifest(baseline, { paths: ['a.ts', 'b.ts'] }))
  assert.throws(() => assertSameManifest(baseline, { paths: ['a.ts', 'c.ts'] }, 'tool'), /missing: b\.ts; added: c\.ts/)
})

test('replaces only the generated README block', () => {
  const readme = `before\n${README_START}\nold\n${README_END}\nafter\n`
  assert.equal(replaceGeneratedBlock(readme, 'new'), `before\n${README_START}\nnew\n${README_END}\nafter\n`)
  assert.throws(() => replaceGeneratedBlock('no markers', 'new'), /markers are missing/)
})

test('builds detailed and root reports from the JSON model', () => {
  const report = sampleReport()
  const root = buildRootBenchmarkBlock(report)
  const detailed = buildDetailedReport(report)
  assert.match(root, /end-to-end CLI time/)
  assert.match(root, /Worsier 2.7.0/)
  assert.match(root, /20\.00 ms \(2\.00×\)/)
  assert.match(detailed, /raw samples/)
  assert.match(detailed, /internal microbenchmarks/)
  assert.match(detailed, /\| Prettier \| 1 KiB \| 20\.00 ms \| 2\.00× \|/)
  assert.equal(detailed.endsWith('\n'), false)
})

test('fixture sources are pinned by immutable revisions and checksums', () => {
  assert.match(fixturePins.typescript.revision, /^[0-9a-f]{40}$/)
  assert.match(fixturePins.typescript.sha256, /^[0-9a-f]{64}$/)
  assert.match(fixturePins.outline.revision, /^[0-9a-f]{40}$/)
  assert.match(fixturePins.outline.archiveSha256, /^[0-9a-f]{64}$/)
  assert.match(fixturePins.typescript.url, new RegExp(fixturePins.typescript.revision))
  assert.match(fixturePins.outline.url, new RegExp(fixturePins.outline.revision))
})

test('reads competitor version pins from the benchmark package manifest', async () => {
  const packageJson = JSON.parse(await readFile(new URL('../package.json', import.meta.url), 'utf8')) as BenchmarkPackage
  assert.deepEqual(pinnedToolVersions, { prettier: packageJson.devDependencies.prettier, oxfmt: packageJson.devDependencies.oxfmt })
})

test('failed command diagnostics preserve invocation, stdout, and stderr', () => {
  const error = commandFailure('Formatter', 'node', ['tool.js'], { status: 2, stdout: 'partial output', stderr: 'raw failure' })
  assert.match(error.message, /Formatter failed with exit code 2/)
  assert.match(error.message, /Command: node tool.js/)
  assert.match(error.message, /partial output/)
  assert.match(error.message, /raw failure/)

  const cause = Object.assign(new Error('spawn hyperfine ENOENT'), { code: 'ENOENT' })
  const spawnError = commandFailure('Hyperfine', 'hyperfine', [], { status: null, stdout: '', stderr: '', error: cause })
  assert.match(spawnError.message, /spawn hyperfine ENOENT/)
  assert.equal(spawnError.cause, cause)
})

test('constructs direct CLI commands and aliases Oxfmt project targets outside ignored work paths', () => {
  const commands = buildCommands()
  const project = '/workspace/benchmark/.work/project-write/oxfmt'
  const prettier = commands.write('prettier', project)
  const oxfmt = commands.write('oxfmt', project)

  assert.doesNotMatch(prettier, /pnpm|pnpx/)
  assert.match(prettier, /prettier\.cjs/)
  assert.doesNotMatch(oxfmt, /pnpm|pnpx/)
  assert.match(oxfmt, /node_modules\/oxfmt\/bin\/oxfmt/)
  assert.match(oxfmt, /\/tmp\/worsier-benchmark-/)
  assert.doesNotMatch(oxfmt, /--write '\/workspace\/benchmark\/\.work/)
})

test('normalizes shell-escaped paths in published commands', () => {
  const workspace = "/Users/person's work/project"
  const node = "/opt/person's tools/node"
  const scriptPath = `${workspace}/packages/npm/bin/worsier.js`
  const inputPath = `${workspace}/benchmark/fixtures/small.ts`
  const outputPath = `${workspace}/benchmark/.work/output.ts`
  const quotedNode = shellQuote(node)
  const script = shellQuote(scriptPath)
  const input = shellQuote(inputPath)
  const output = shellQuote(outputPath)
  const command = `${quotedNode} ${script} --stdin-filepath ${input} < ${input} > ${output}`
  const displayed = displayCommand(command, workspace, node)
  assert.equal(displayed, "'<node>' '<repo>/packages/npm/bin/worsier.js' --stdin-filepath '<repo>/benchmark/fixtures/small.ts' < '<repo>/benchmark/fixtures/small.ts' > '<repo>/benchmark/.work/output.ts'")
})

test('passes the fixture to Hyperfine with a distinct output file for each formatter', () => {
  const definition: ScenarioDefinition = {
    name: 'small',
    displayName: 'Small TypeScript input',
    commands: { worsier: 'worsier-cli', prettier: 'prettier-cli', oxfmt: 'oxfmt-cli' },
    input: "/tmp/person's work/input.ts",
    outputDirectory: "/tmp/person's work/outputs"
  }
  const args = buildHyperfineArgs(definition, measurementSettings, '/tmp/result.json')
  const stdinOption = args.indexOf('--input')
  const formatterArgs = args.slice(stdinOption)
  assert.deepEqual(formatterArgs, [
    '--input', "/tmp/person's work/input.ts",
    '--output', "/tmp/person's work/outputs/worsier.ts", '--command-name', 'worsier', 'worsier-cli',
    '--output', "/tmp/person's work/outputs/prettier.ts", '--command-name', 'prettier', 'prettier-cli',
    '--output', "/tmp/person's work/outputs/oxfmt.ts", '--command-name', 'oxfmt', 'oxfmt-cli'
  ])
})

test('does not collect or publish a Linux hostname', () => {
  const hostname = 'alice-work-laptop'
  const calls: string[][] = []
  const captureCommand: CaptureCommand = (command, args) => {
    calls.push([command, ...args])
    if (command === 'uname') {
      const unameValues: Record<string, string> = {
        '-m': 'x86_64',
        '-n': hostname,
        '-p': 'AMD Ryzen',
        '-r': '6.8.0',
        '-s': 'Linux'
      }
      const argument = args[0]
      const value = argument ? unameValues[argument] : undefined
      if (value) {
        return value
      }
    }
    if (command === 'getconf') return '8'
    if (command === 'pnpm') return '11.21.0'
    if (command === 'rustc') return 'rustc 1.97.1 (example)'
    if (command === 'cargo') return 'cargo 1.97.1 (example)'
    if (command === 'hyperfine') return 'hyperfine 1.20.0'
    throw new Error(`Unexpected command: ${command} ${args.join(' ')}`)
  }
  const environment = collectEnvironment({ platform: 'linux', captureCommand, systemMemory: 32 * 1024 * 1024 * 1024 })
  const report = sampleReport()
  report.environment = environment

  assert.equal(environment.machineModel, 'not recorded')
  assert.equal(environment.osBuild, 'not recorded')
  assert.equal(calls.some((call) => call[0] === 'uname' && call[1] === '-n'), false)
  assert.doesNotMatch(JSON.stringify(report), new RegExp(hostname))
  assert.doesNotMatch(buildDetailedReport(report), new RegExp(hostname))
  assert.doesNotMatch(buildRootBenchmarkBlock(report), new RegExp(hostname))
})

test('loads report metadata from the executed configs and shared measurement counts', async () => {
  assert.deepEqual(await loadBenchmarkSettings(), {
    ...measurementSettings,
    lineWidth: 120,
    semicolons: false,
    trailingCommas: false,
    endOfLine: 'lf',
    worsierVerifyAst: true,
    cache: false,
    concurrency: 'CLI defaults'
  })
})

test('requires every Worsier semicolon group to be explicitly disabled for benchmarks', () => {
  const prettier = { semi: false }
  const oxfmt = { semi: false }
  const worsier = {
    rules: {
      semicolons: {
        statements: 'asNeeded',
        classMembers: 'asNeeded',
        typeMembers: 'asNeeded'
      }
    }
  }

  assert.doesNotThrow(() => assertBenchmarkSemicolonsDisabled(worsier, prettier, oxfmt))

  for (const group of Object.keys(worsier.rules.semicolons) as Array<keyof typeof worsier.rules.semicolons>) {
    const incomplete = structuredClone(worsier) as WorsierSemicolonCheckConfig
    delete incomplete.rules.semicolons[group]
    assert.throws(
      () => assertBenchmarkSemicolonsDisabled(incomplete, prettier, oxfmt),
      /must disable optional semicolons/
    )
  }

  const unexpected = {
    rules: {
      semicolons: {
        ...worsier.rules.semicolons,
        extra: 'asNeeded'
      }
    }
  }
  assert.throws(
    () => assertBenchmarkSemicolonsDisabled(unexpected, prettier, oxfmt),
    /must disable optional semicolons/
  )
})

test('validates every derived sample statistic and the complete Criterion matrix', () => {
  const report = sampleReport()
  assert.doesNotThrow(() => validateReport(report, pinnedToolVersions))

  const wrongMean = structuredClone(report)
  wrongMean.scenarios.small.results.worsier.meanSeconds = 999
  assert.throws(() => validateReport(wrongMean, pinnedToolVersions), /mean does not match its raw samples/)

  const wrongRss = structuredClone(report)
  wrongRss.scenarios.parser.results.prettier.peakRssBytes = 999
  assert.throws(() => validateReport(wrongRss, pinnedToolVersions), /peak RSS does not match its raw samples/)

  const emptyMicrobenchmarks = structuredClone(report)
  emptyMicrobenchmarks.microbenchmarks = []
  assert.throws(() => validateReport(emptyMicrobenchmarks, pinnedToolVersions), /complete Criterion benchmark matrix/)
})

test('rejects a competitor version outside the snapshot pins', () => {
  const report = sampleReport()
  report.tools.prettier.version = '0.0.0'
  assert.throws(() => validateReport(report, pinnedToolVersions), /prettier version 0\.0\.0 does not match its pin/)
})

test('requires paired timing and RSS samples for Hyperfine 2 reports', () => {
  const report = sampleReport()
  report.settings.rssRuns = 2
  for (const scenario of Object.values(report.scenarios)) {
    for (const result of Object.values(scenario.results)) {
      result.peakRssSamplesBytes = [1024, 1024]
    }
  }
  assert.doesNotThrow(() => validateReport(report, pinnedToolVersions))

  report.environment.hyperfine = '2.0.0'
  assert.throws(() => validateReport(report, pinnedToolVersions), /must pair each wall time sample/)
})

function sampleReport(): BenchmarkReport {
  const result = (seconds: number): BenchmarkResult => ({
    command: "'<node>' '<repo>/formatter.js'",
    inputBytes: 1024,
    samplesSeconds: [seconds],
    medianSeconds: seconds,
    minSeconds: seconds,
    maxSeconds: seconds,
    meanSeconds: seconds,
    stddevSeconds: 0,
    throughputMibPerSecond: 1024 / seconds / 1024 / 1024,
    peakRssSamplesBytes: [1024],
    peakRssBytes: 1024
  })
  const scenario: BenchmarkScenario = {
    displayName: 'Scenario',
    inputBytes: 1024,
    inputBytesByTool: null,
    results: {
      worsier: result(0.01),
      prettier: result(0.02),
      oxfmt: result(0.03)
    }
  }
  return {
    schemaVersion: 2,
    generatedAt: '2026-08-16T00:00:00.000Z',
    source: { worsierSha: '1234567890abcdef' },
    environment: {
      machineModel: 'Mac14,6',
      cpu: 'Apple M2 Max',
      cores: 12,
      memoryGb: 32,
      osName: 'macOS',
      osVersion: '26.5.2',
      osBuild: '25F84',
      architecture: 'arm64',
      powerSource: 'AC power',
      powerMode: 'normal power mode',
      node: '24.19.0',
      pnpm: '11.21.0',
      rust: '1.97.1',
      cargo: '1.97.1',
      hyperfine: '1.20.0'
    },
    tools: {
      worsier: { displayName: 'Worsier', version: '2.7.0' },
      prettier: { displayName: 'Prettier', version: pinnedToolVersions.prettier },
      oxfmt: { displayName: 'Oxfmt', version: pinnedToolVersions.oxfmt }
    },
    settings: {
      warmups: 3,
      runs: 1,
      rssRuns: 1,
      lineWidth: 120,
      semicolons: false,
      trailingCommas: false,
      endOfLine: 'lf',
      worsierVerifyAst: true,
      cache: false,
      concurrency: 'CLI defaults'
    },
    fixtures: {
      small: { files: 1, bytes: 1, sha256: 'a', source: 'small.ts' },
      parser: { files: 1, bytes: 1, sha256: 'b', source: 'parser.ts', lineEndings: 'lf' },
      outline: { files: 1, bytes: 1, sha256: 'c', source: 'outline' }
    },
    validation: {
      baselineManifestHash: 'manifest',
      fileCount: 1,
      idempotent: true,
      outputBytes: { worsier: 1, prettier: 1, oxfmt: 1 },
      outputHashes: { worsier: {}, prettier: {}, oxfmt: {} }
    },
    scenarios: { small: scenario, parser: scenario, projectWrite: scenario, projectCheck: scenario },
    microbenchmarks: ['single_parse', 'format_no_verify_default', 'format_no_verify_semicolons_off', 'format_no_verify_trailing_commas_off', 'parse_and_verify'].flatMap((measurement): MicrobenchmarkResult[] => ([
      ['small', 512],
      ['50kb', 50 * 1024],
      ['1mb', 1024 * 1024]
    ] as Array<[string, number]>).map(([input, inputBytes]) => {
      const medianSeconds = 0.001
      return {
        measurement,
        input,
        inputBytes,
        samplesSeconds: [medianSeconds],
        medianSeconds,
        throughputMibPerSecond: inputBytes / medianSeconds / 1024 / 1024
      }
    }))
  }
}

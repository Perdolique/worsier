import { spawnSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { cp, copyFile, mkdir, readFile, readdir, rm, stat, symlink, writeFile } from 'node:fs/promises'
import { totalmem } from 'node:os'
import { basename, dirname, extname, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import {
  assertSameManifest,
  buildDetailedReport,
  buildRootBenchmarkBlock,
  calculateStatistics,
  describeFixture,
  describeManifest,
  hashFile,
  normalizeLineEndings,
  parseHyperfineJson,
  parsePeakRss,
  replaceGeneratedBlock,
  shellQuote
} from './lib.mjs'

const benchmarkDirectory = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const root = resolve(benchmarkDirectory, '..')
const workDirectory = join(benchmarkDirectory, '.work')
const fixtureWorkDirectory = join(workDirectory, 'fixtures')
const resultDirectory = join(benchmarkDirectory, 'results')
const cliPath = join(root, 'packages/npm/bin/worsier.js')
const prettierPath = join(benchmarkDirectory, 'node_modules/prettier/bin/prettier.cjs')
const oxfmtPath = join(benchmarkDirectory, 'node_modules/oxfmt/bin/oxfmt')
const runnerPath = join(benchmarkDirectory, 'src/cli.mjs')
const configDirectory = join(benchmarkDirectory, 'config')
const ignorePath = join(configDirectory, 'empty-ignore')
const toolNames = ['worsier', 'prettier', 'oxfmt']
const sourceExtensions = new Set(['.js', '.jsx', '.ts', '.tsx'])
const benchmarkPackage = JSON.parse(readFileSync(join(benchmarkDirectory, 'package.json'), 'utf8'))

export const pinnedToolVersions = Object.freeze({
  prettier: exactDependencyVersion('prettier'),
  oxfmt: exactDependencyVersion('oxfmt')
})

const criterionMeasurements = ['single_parse', 'format_no_verify_default', 'format_no_verify_semicolons_off', 'format_no_verify_trailing_commas_off', 'parse_and_verify']
const criterionInputs = new Map([['small', 512], ['50kb', 50 * 1024], ['1mb', 1024 * 1024]])

export const measurementSettings = Object.freeze({
  warmups: 3,
  runs: 10,
  rssRuns: 5
})

export const fixturePins = {
  typescript: {
    tag: 'v5.9.2',
    revision: '5be33469d551655d878876faa9e30aa3b49f8ee9',
    sha256: 'dcddb577aa14f455193ca98c7c069217a0c523466fc777b985a7aefbd8dafc8e',
    url: 'https://raw.githubusercontent.com/microsoft/TypeScript/5be33469d551655d878876faa9e30aa3b49f8ee9/src/compiler/parser.ts'
  },
  outline: {
    revision: 'cdc10b45649d04e6dcfb27fb6ca0aeadd100d2bc',
    archiveSha256: 'd597d3f00349bbccca9059bd3bc8366b415f7fce754e2db4f885b5e6e4c58135',
    url: 'https://codeload.github.com/outline/outline/tar.gz/cdc10b45649d04e6dcfb27fb6ca0aeadd100d2bc'
  }
}

export async function loadBenchmarkSettings() {
  const [worsier, prettier, oxfmt] = await Promise.all([
    readJsonConfig(join(configDirectory, 'worsier.jsonc')),
    readJsonConfig(join(configDirectory, 'prettier.json')),
    readJsonConfig(join(configDirectory, 'oxfmt.json'))
  ])
  if (new Set([worsier.lineWidth, prettier.printWidth, oxfmt.printWidth]).size !== 1) {
    throw new Error('Benchmark formatter configs must use the same line width')
  }
  if (prettier.semi !== false || oxfmt.semi !== false || Object.values(worsier.rules.semicolons).some((mode) => mode !== 'asNeeded')) {
    throw new Error('Benchmark formatter configs must disable optional semicolons')
  }
  if (prettier.trailingComma !== 'none' || oxfmt.trailingComma !== 'none' || worsier.rules.trailingCommas !== 'never') {
    throw new Error('Benchmark formatter configs must disable optional trailing commas')
  }
  if (prettier.endOfLine !== 'lf' || oxfmt.endOfLine !== 'lf') {
    throw new Error('Benchmark full-source formatter configs must use LF line endings')
  }
  if (worsier.verifyAst !== true) {
    throw new Error('Benchmark Worsier config must keep AST verification enabled')
  }

  return {
    ...measurementSettings,
    lineWidth: worsier.lineWidth,
    semicolons: false,
    trailingCommas: false,
    endOfLine: prettier.endOfLine,
    worsierVerifyAst: worsier.verifyAst,
    cache: false,
    concurrency: 'CLI defaults'
  }
}

export async function runBenchmark({ publish = false } = {}) {
  if (publish) {
    requireCleanWorktree()
  }

  await mkdir(workDirectory, { recursive: true })
  await prepareFixtures()
  const fixtures = await collectFixtureMetadata()
  const commands = buildCommands()
  const settings = await loadBenchmarkSettings()
  const validation = await validateTools(commands, fixtures)
  const scenarios = {}

  for (const definition of scenarioDefinitions(commands, fixtures, validation)) {
    console.log(`Measuring ${definition.displayName}`)
    scenarios[definition.name] = await measureScenario(definition, settings)
  }

  console.log('Running Worsier internal Criterion benchmarks')
  const microbenchmarks = await runCriterionBenchmarks()
  const report = {
    schemaVersion: 2,
    generatedAt: new Date().toISOString(),
    source: { worsierSha: capture('git', ['rev-parse', 'HEAD']) },
    environment: collectEnvironment(),
    tools: collectToolVersions(),
    fixtures,
    settings,
    validation,
    scenarios,
    microbenchmarks
  }

  const json = `${JSON.stringify(report, null, 2)}\n`
  const markdown = `${buildDetailedReport(report)}\n`
  if (publish) {
    await mkdir(resultDirectory, { recursive: true })
    await writeFile(join(resultDirectory, 'latest.json'), json)
    await writeFile(join(resultDirectory, 'latest.md'), markdown)
    await updateRootReadme(report)
  } else {
    await writeFile(join(workDirectory, 'latest.json'), json)
    await writeFile(join(workDirectory, 'latest.md'), markdown)
  }

  return report
}

export async function verifyPublishedResults() {
  const jsonPath = join(resultDirectory, 'latest.json')
  const report = JSON.parse(await readFile(jsonPath, 'utf8'))
  validateReport(report)
  const expectedMarkdown = `${buildDetailedReport(report)}\n`
  const actualMarkdown = await readFile(join(resultDirectory, 'latest.md'), 'utf8')
  if (actualMarkdown !== expectedMarkdown) {
    throw new Error('benchmark/results/latest.md is out of sync with latest.json')
  }

  const readmePath = join(root, 'README.md')
  const readme = await readFile(readmePath, 'utf8')
  const expectedReadme = replaceGeneratedBlock(readme, buildRootBenchmarkBlock(report))
  if (readme !== expectedReadme) {
    throw new Error('README.md benchmark table is out of sync with latest.json')
  }
}

export async function smokeTools() {
  const fixture = join(benchmarkDirectory, 'fixtures/small.ts')
  const commands = buildCommands()
  for (const tool of toolNames) {
    const output = join(workDirectory, `smoke-${tool}.ts`)
    await mkdir(workDirectory, { recursive: true })
    runShell(commands.stdin(tool, fixture, output), `Smoke ${tool}`)
    const outputStat = await stat(output)
    if (outputStat.size === 0) {
      throw new Error(`${tool} smoke test produced empty output`)
    }
  }
}

export async function restoreProjectCopies() {
  const baseline = join(fixtureWorkDirectory, 'outline')
  for (const tool of toolNames) {
    const destination = join(workDirectory, 'project-write', tool)
    await rm(destination, { recursive: true, force: true })
    await cp(baseline, destination, { recursive: true })
  }
  await ensureOxfmtAlias(join(workDirectory, 'project-write', 'oxfmt'))
}

async function prepareFixtures() {
  const downloads = join(workDirectory, 'downloads')
  await mkdir(downloads, { recursive: true })
  await mkdir(fixtureWorkDirectory, { recursive: true })

  const parserDownloadPath = join(downloads, `typescript-parser-${fixturePins.typescript.revision}.ts`)
  await downloadVerified(fixturePins.typescript.url, parserDownloadPath, fixturePins.typescript.sha256)
  const parserPath = join(fixtureWorkDirectory, 'parser.ts')
  const parserSource = normalizeLineEndings(await readFile(parserDownloadPath, 'utf8'))
  if (parserSource.includes('\r')) {
    throw new Error('TypeScript parser fixture could not be normalized to LF line endings')
  }
  await writeFile(parserPath, parserSource)

  const archivePath = join(downloads, `outline-${fixturePins.outline.revision}.tar.gz`)
  await downloadVerified(fixturePins.outline.url, archivePath, fixturePins.outline.archiveSha256)
  const extractedDirectory = join(downloads, 'outline-extracted')
  const outlineDirectory = join(fixtureWorkDirectory, 'outline')
  await rm(extractedDirectory, { recursive: true, force: true })
  await rm(outlineDirectory, { recursive: true, force: true })
  await mkdir(extractedDirectory, { recursive: true })
  run('tar', ['-xzf', archivePath, '-C', extractedDirectory], { label: 'Extract Outline fixture' })
  const [archiveRoot] = await readdir(extractedDirectory)
  const sourceRoot = join(extractedDirectory, archiveRoot)
  await copySourceTree(sourceRoot, outlineDirectory)
}

async function collectFixtureMetadata() {
  const small = await describeFixture(join(benchmarkDirectory, 'fixtures/small.ts'))
  const parser = await describeFixture(join(fixtureWorkDirectory, 'parser.ts'))
  const outline = await describeFixture(join(fixtureWorkDirectory, 'outline'))
  return {
    small: { ...small, source: 'benchmark/fixtures/small.ts' },
    parser: { ...parser, source: 'TypeScript src/compiler/parser.ts', revision: fixturePins.typescript.revision, tag: fixturePins.typescript.tag, lineEndings: 'lf' },
    outline: { ...outline, source: 'Outline JS/JSX/TS/TSX corpus', revision: fixturePins.outline.revision }
  }
}

export function buildCommands() {
  const configs = {
    worsier: join(configDirectory, 'worsier.jsonc'),
    prettier: join(configDirectory, 'prettier.json'),
    oxfmt: join(configDirectory, 'oxfmt.json')
  }
  const binaries = {
    worsier: `${shellQuote(process.execPath)} ${shellQuote(cliPath)}`,
    prettier: `${shellQuote(process.execPath)} ${shellQuote(prettierPath)}`,
    oxfmt: `${shellQuote(process.execPath)} ${shellQuote(oxfmtPath)}`
  }

  return {
    stdin(tool, input, output) {
      const base = `${binaries[tool]} --config ${shellQuote(configs[tool])}`
      const ignore = tool === 'worsier' ? '' : ` --ignore-path ${shellQuote(ignorePath)}`
      return `${base}${ignore} --stdin-filepath ${shellQuote(input)} < ${shellQuote(input)} > ${shellQuote(output)}`
    },
    write(tool, directory) {
      const base = `${binaries[tool]} --config ${shellQuote(configs[tool])}`
      const ignore = tool === 'worsier' ? '' : ` --ignore-path ${shellQuote(ignorePath)}`
      const nested = tool === 'oxfmt' ? ' --disable-nested-config' : ''
      return `${base}${ignore}${nested} --write ${shellQuote(projectTarget(tool, directory))}`
    },
    check(tool, directory) {
      const base = `${binaries[tool]} --config ${shellQuote(configs[tool])}`
      const ignore = tool === 'worsier' ? '' : ` --ignore-path ${shellQuote(ignorePath)}`
      const nested = tool === 'oxfmt' ? ' --disable-nested-config' : ''
      return `${base}${ignore}${nested} --check ${shellQuote(projectTarget(tool, directory))}`
    }
  }
}

async function validateTools(commands, fixtures) {
  console.log('Validating tools and idempotency')
  const baselineManifest = await describeManifest(join(fixtureWorkDirectory, 'outline'))
  const outputHashes = {}
  const outputBytes = {}
  for (const tool of toolNames) {
    outputHashes[tool] = {}
    for (const name of ['small', 'parser']) {
      const input = name === 'small' ? join(benchmarkDirectory, 'fixtures/small.ts') : join(fixtureWorkDirectory, 'parser.ts')
      const first = join(workDirectory, 'validation', tool, `${name}-first.ts`)
      const second = join(workDirectory, 'validation', tool, `${name}-second.ts`)
      await mkdir(dirname(first), { recursive: true })
      runShell(commands.stdin(tool, input, first), `Validate ${tool} ${name}`)
      runShell(commands.stdin(tool, first, second), `Validate ${tool} ${name} idempotency`)
      const [firstHash, secondHash] = await Promise.all([hashFile(first), hashFile(second)])
      if (firstHash !== secondHash) {
        throw new Error(`${tool} is not idempotent for ${name}`)
      }
      outputHashes[tool][name] = firstHash
    }

    const project = join(workDirectory, 'validation', tool, 'outline')
    await rm(project, { recursive: true, force: true })
    await cp(join(fixtureWorkDirectory, 'outline'), project, { recursive: true })
    if (tool === 'oxfmt') {
      await ensureOxfmtAlias(project)
    }
    runShell(commands.write(tool, project), `Validate ${tool} project write`)
    const firstProject = await describeFixture(project)
    const outputManifest = await describeManifest(project)
    assertSameManifest(baselineManifest, outputManifest, tool)
    runShell(commands.write(tool, project), `Validate ${tool} project idempotency`)
    const secondProject = await describeFixture(project)
    if (firstProject.sha256 !== secondProject.sha256) {
      throw new Error(`${tool} is not idempotent for the Outline fixture`)
    }
    runShell(commands.check(tool, project), `Validate ${tool} project check`)
    outputHashes[tool].outline = firstProject.sha256
    outputBytes[tool] = firstProject.bytes
  }

  await restoreProjectCopies()
  return {
    baselineManifestHash: baselineManifest.sha256,
    fileCount: fixtures.outline.files,
    idempotent: true,
    outputHashes,
    outputBytes
  }
}

function scenarioDefinitions(commands, fixtures, validation) {
  const smallPath = join(benchmarkDirectory, 'fixtures/small.ts')
  const parserPath = join(fixtureWorkDirectory, 'parser.ts')
  const canonical = (tool) => join(workDirectory, 'validation', tool, 'outline')
  const project = (tool) => join(workDirectory, 'project-write', tool)
  return [
    stdinScenario('small', 'Small TS stdin format', smallPath, fixtures.small.bytes, commands),
    stdinScenario('parser', 'TypeScript parser.ts stdin format', parserPath, fixtures.parser.bytes, commands),
    {
      name: 'projectWrite',
      displayName: 'Outline project write',
      bytes: fixtures.outline.bytes,
      prepareCommand: `${shellQuote(process.execPath)} ${shellQuote(runnerPath)} restore-project`,
      commands: Object.fromEntries(toolNames.map((tool) => [tool, commands.write(tool, project(tool))])),
      prepareRss: async (tool) => {
        const destination = project(tool)
        await rm(destination, { recursive: true, force: true })
        await cp(join(fixtureWorkDirectory, 'outline'), destination, { recursive: true })
      }
    },
    {
      name: 'projectCheck',
      displayName: 'Outline project check on canonical output',
      bytesByTool: validation.outputBytes,
      commands: Object.fromEntries(toolNames.map((tool) => [tool, commands.check(tool, canonical(tool))]))
    }
  ]
}

function stdinScenario(name, displayName, input, bytes, commands) {
  const outputDirectory = join(workDirectory, 'timed-output', name)
  return {
    name,
    displayName,
    bytes,
    commands: Object.fromEntries(toolNames.map((tool) => [tool, commands.stdin(tool, input, join(outputDirectory, `${tool}.ts`))])),
    before: () => mkdir(outputDirectory, { recursive: true })
  }
}

async function measureScenario(definition, settings) {
  await definition.before?.()
  const hyperfinePath = join(workDirectory, 'hyperfine', `${definition.name}.json`)
  await mkdir(dirname(hyperfinePath), { recursive: true })
  const args = ['--warmup', String(settings.warmups), '--runs', String(settings.runs), '--export-json', hyperfinePath]
  if (definition.prepareCommand) {
    args.push('--prepare', definition.prepareCommand)
  }
  for (const tool of toolNames) {
    args.push('--command-name', tool, definition.commands[tool])
  }
  run('hyperfine', args, { label: `Hyperfine ${definition.name}`, inherit: true })
  const hyperfine = parseHyperfineJson(await readFile(hyperfinePath, 'utf8'))
  const results = {}

  for (const tool of toolNames) {
    const rssSamples = []
    for (let index = 0; index < settings.rssRuns; index += 1) {
      await definition.prepareRss?.(tool)
      rssSamples.push(measurePeakRss(definition.commands[tool]))
    }
    const timing = hyperfine[tool]
    const rss = calculateStatistics(rssSamples)
    const inputBytes = definition.bytesByTool?.[tool] ?? definition.bytes
    results[tool] = {
      command: displayCommand(definition.commands[tool]),
      inputBytes,
      samplesSeconds: timing.samples,
      medianSeconds: timing.median,
      minSeconds: timing.min,
      maxSeconds: timing.max,
      meanSeconds: timing.mean,
      stddevSeconds: timing.stddev,
      throughputMibPerSecond: inputBytes === 0 ? null : inputBytes / timing.median / 1024 / 1024,
      peakRssSamplesBytes: rss.samples,
      peakRssBytes: rss.median
    }
  }

  return {
    displayName: definition.displayName,
    inputBytes: definition.bytes ?? null,
    inputBytesByTool: definition.bytesByTool ?? null,
    results
  }
}

export function displayCommand(command) {
  return command
    .replaceAll(process.execPath, '<node>')
    .replaceAll(root, '<repo>')
    .replaceAll('/tmp/worsier-benchmark-', '<tmp>/worsier-benchmark-')
}

function measurePeakRss(command) {
  const timeArgs = process.platform === 'darwin' ? ['-l', '/bin/sh', '-c', command] : ['-v', '/bin/sh', '-c', command]
  const result = spawnSync('/usr/bin/time', timeArgs, { cwd: root, encoding: 'utf8', maxBuffer: 100 * 1024 * 1024 })
  if (result.status !== 0) {
    throw commandFailure('Peak RSS measurement', '/usr/bin/time', timeArgs, result)
  }
  return parsePeakRss(result.stderr)
}

async function runCriterionBenchmarks() {
  const criterionDirectory = join(root, 'target/criterion/formatter')
  await rm(criterionDirectory, { recursive: true, force: true })
  run('cargo', ['bench', '-p', 'worsier-benchmark', '--bench', 'formatter', '--', '--noplot'], { label: 'Criterion benchmark', inherit: true })
  const estimates = await findNamedFiles(criterionDirectory, 'estimates.json')
  const benchmarks = []
  for (const estimatePath of estimates) {
    if (!estimatePath.endsWith(`${join('new', 'estimates.json')}`)) {
      continue
    }
    const pathParts = relative(criterionDirectory, estimatePath).split(/[\\/]/)
    if (pathParts.length < 4) {
      continue
    }
    const [measurement, input] = pathParts
    const estimate = JSON.parse(await readFile(estimatePath, 'utf8'))
    const samplePath = join(dirname(estimatePath), 'sample.json')
    const sample = JSON.parse(await readFile(samplePath, 'utf8'))
    const benchmarkPath = join(dirname(estimatePath), 'benchmark.json')
    const benchmark = JSON.parse(await readFile(benchmarkPath, 'utf8'))
    const samplesSeconds = sample.times.map((time, index) => time / sample.iters[index] / 1e9)
    const inputBytes = benchmark.throughput?.Bytes
    if (!Number.isSafeInteger(inputBytes) || inputBytes <= 0) {
      throw new Error(`Criterion benchmark ${measurement}/${input} does not record byte throughput`)
    }
    const medianSeconds = estimate.median.point_estimate / 1e9
    benchmarks.push({
      measurement,
      input,
      inputBytes,
      samplesSeconds,
      medianSeconds,
      throughputMibPerSecond: inputBytes / medianSeconds / 1024 / 1024
    })
  }
  return benchmarks.sort((left, right) => `${left.measurement}/${left.input}`.localeCompare(`${right.measurement}/${right.input}`))
}

function collectToolVersions() {
  return {
    worsier: { displayName: 'Worsier', version: capture(process.execPath, [cliPath, '--version']).replace(/^worsier\s+/, '') },
    prettier: { displayName: 'Prettier', version: capture(process.execPath, [prettierPath, '--version']) },
    oxfmt: { displayName: 'Oxfmt', version: capture(process.execPath, [oxfmtPath, '--version']).replace(/^Version:\s*/, '') }
  }
}

export function collectEnvironment({ platform = process.platform, captureCommand = capture, systemMemory = totalmem() } = {}) {
  const architecture = captureCommand('uname', ['-m'])
  if (platform !== 'darwin') {
    return {
      machineModel: 'not recorded',
      cpu: captureCommand('uname', ['-p']),
      cores: Number.parseInt(captureCommand('getconf', ['_NPROCESSORS_ONLN']), 10),
      memoryGb: Math.round(systemMemory / 1024 / 1024 / 1024),
      osName: captureCommand('uname', ['-s']),
      osVersion: captureCommand('uname', ['-r']),
      osBuild: 'not recorded',
      architecture,
      powerSource: 'not recorded',
      powerMode: 'not recorded',
      ...collectToolchainVersions(captureCommand)
    }
  }

  const power = captureCommand('pmset', ['-g', 'batt'])
  const settings = captureCommand('pmset', ['-g', 'custom'])
  const powerModeValue = settings.match(/powermode\s+(\d+)/)?.[1]
  const powerModes = { '0': 'normal power mode', '1': 'low power mode', '2': 'high power mode' }
  return {
    machineModel: captureCommand('sysctl', ['-n', 'hw.model']),
    cpu: captureCommand('sysctl', ['-n', 'machdep.cpu.brand_string']),
    cores: Number.parseInt(captureCommand('sysctl', ['-n', 'hw.physicalcpu']), 10),
    memoryGb: Math.round(Number.parseInt(captureCommand('sysctl', ['-n', 'hw.memsize']), 10) / 1024 / 1024 / 1024),
    osName: 'macOS',
    osVersion: captureCommand('sw_vers', ['-productVersion']),
    osBuild: captureCommand('sw_vers', ['-buildVersion']),
    architecture,
    powerSource: power.includes('AC Power') ? 'AC power' : 'battery power',
    powerMode: powerModes[powerModeValue] ?? 'power mode not reported',
    ...collectToolchainVersions(captureCommand)
  }
}

function collectToolchainVersions(captureCommand = capture) {
  return {
    node: process.version.replace(/^v/, ''),
    pnpm: captureCommand('pnpm', ['--version']),
    rust: captureCommand('rustc', ['--version']).replace(/^rustc\s+/, '').split(' ')[0],
    cargo: captureCommand('cargo', ['--version']).replace(/^cargo\s+/, '').split(' ')[0],
    hyperfine: captureCommand('hyperfine', ['--version']).replace(/^hyperfine\s+/, '')
  }
}

async function updateRootReadme(report) {
  const path = join(root, 'README.md')
  const readme = await readFile(path, 'utf8')
  const updated = replaceGeneratedBlock(readme, buildRootBenchmarkBlock(report))
  await writeFile(path, updated)
}

export function validateReport(report) {
  if (report.schemaVersion !== 2) {
    throw new Error(`Unsupported benchmark schema version: ${report.schemaVersion}`)
  }
  if (report.tools.prettier.version !== pinnedToolVersions.prettier || report.tools.oxfmt.version !== pinnedToolVersions.oxfmt) {
    throw new Error('Published benchmark tool versions do not match pinned dependencies')
  }
  if (report.fixtures?.parser?.lineEndings !== 'lf' && report.fixtures?.parser?.lineEndings !== 'crlf') {
    throw new Error('Published parser fixture does not record its line endings')
  }
  for (const scenario of ['small', 'parser', 'projectWrite', 'projectCheck']) {
    for (const tool of toolNames) {
      const result = report.scenarios?.[scenario]?.results?.[tool]
      const samples = result?.samplesSeconds
      if (!Array.isArray(samples) || samples.length !== report.settings.runs || samples.some((sample) => !Number.isFinite(sample) || sample <= 0)) {
        throw new Error(`${scenario}/${tool} does not contain the expected raw samples`)
      }
      if (!Number.isSafeInteger(result.inputBytes) || result.inputBytes <= 0) {
        throw new Error(`${scenario}/${tool} does not record its input byte count`)
      }
      if (!result.command.includes('<node>') || !result.command.includes('<repo>')) {
        throw new Error(`${scenario}/${tool} command contains machine-specific paths`)
      }
      const timing = calculateStatistics(samples)
      assertDerivedValue(result.medianSeconds, timing.median, `${scenario}/${tool} median`)
      assertDerivedValue(result.minSeconds, timing.min, `${scenario}/${tool} minimum`)
      assertDerivedValue(result.maxSeconds, timing.max, `${scenario}/${tool} maximum`)
      assertDerivedValue(result.meanSeconds, timing.mean, `${scenario}/${tool} mean`)
      assertDerivedValue(result.stddevSeconds, timing.stddev, `${scenario}/${tool} standard deviation`)
      assertDerivedValue(result.throughputMibPerSecond, result.inputBytes / timing.median / 1024 / 1024, `${scenario}/${tool} throughput`)
      const rssSamples = result.peakRssSamplesBytes
      if (!Array.isArray(rssSamples) || rssSamples.length !== report.settings.rssRuns || rssSamples.some((sample) => !Number.isSafeInteger(sample) || sample <= 0)) {
        throw new Error(`${scenario}/${tool} does not contain the expected peak RSS samples`)
      }
      assertDerivedValue(result.peakRssBytes, calculateStatistics(rssSamples).median, `${scenario}/${tool} peak RSS`)
    }
  }
  const expectedMicrobenchmarks = new Set(criterionMeasurements.flatMap((measurement) => [...criterionInputs.keys()].map((input) => `${measurement}/${input}`)))
  if (!Array.isArray(report.microbenchmarks) || report.microbenchmarks.length !== expectedMicrobenchmarks.size) {
    throw new Error('Published report does not contain the complete Criterion benchmark matrix')
  }
  for (const benchmark of report.microbenchmarks) {
    const key = `${benchmark.measurement}/${benchmark.input}`
    if (!expectedMicrobenchmarks.delete(key)) {
      throw new Error(`Published report contains an unexpected or duplicate Criterion benchmark: ${key}`)
    }
    if (!Number.isSafeInteger(benchmark.inputBytes) || benchmark.inputBytes <= 0) {
      throw new Error(`${key} does not record its input byte count`)
    }
    if (benchmark.inputBytes !== criterionInputs.get(benchmark.input)) {
      throw new Error(`${key} records an unexpected input byte count`)
    }
    if (!Array.isArray(benchmark.samplesSeconds) || benchmark.samplesSeconds.length === 0 || benchmark.samplesSeconds.some((sample) => !Number.isFinite(sample) || sample <= 0)) {
      throw new Error(`${key} does not contain raw samples`)
    }
    if (!Number.isFinite(benchmark.medianSeconds) || benchmark.medianSeconds <= 0) {
      throw new Error(`${key} does not contain a positive median estimate`)
    }
    assertDerivedValue(benchmark.throughputMibPerSecond, benchmark.inputBytes / benchmark.medianSeconds / 1024 / 1024, `${key} throughput`)
  }
  if (expectedMicrobenchmarks.size !== 0) {
    throw new Error(`Published report is missing Criterion benchmarks: ${[...expectedMicrobenchmarks].join(', ')}`)
  }
}

function assertDerivedValue(actual, expected, label) {
  if (!Number.isFinite(actual) || actual !== expected) {
    throw new Error(`${label} does not match its raw samples: expected ${expected}, received ${actual}`)
  }
}

function exactDependencyVersion(name) {
  const version = benchmarkPackage.devDependencies?.[name]
  if (typeof version !== 'string' || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) {
    throw new Error(`benchmark/package.json must pin ${name} to one exact version`)
  }
  return version
}

function requireCleanWorktree() {
  const status = capture('git', ['status', '--porcelain=v1', '--untracked-files=all'])
  if (status !== '') {
    throw new Error('benchmark:update requires a clean Git worktree so the report can identify one committed Worsier SHA')
  }
}

async function downloadVerified(url, destination, expectedSha256) {
  await mkdir(dirname(destination), { recursive: true })
  let existing = false
  try {
    existing = await hashFile(destination) === expectedSha256
  } catch (error) {
    if (error.code !== 'ENOENT') {
      throw error
    }
  }
  if (!existing) {
    const response = await fetch(url)
    if (!response.ok) {
      throw new Error(`Download failed with HTTP ${response.status}: ${url}`)
    }
    await writeFile(destination, Buffer.from(await response.arrayBuffer()))
  }
  const actualSha256 = await hashFile(destination)
  if (actualSha256 !== expectedSha256) {
    throw new Error(`Fixture checksum mismatch for ${url}: expected ${expectedSha256}, received ${actualSha256}`)
  }
}

async function copySourceTree(sourceRoot, destinationRoot) {
  const files = await findNamedFiles(sourceRoot)
  for (const source of files) {
    if (!sourceExtensions.has(extname(source)) || basename(source) === 'worker-configuration.d.ts') {
      continue
    }
    const destination = join(destinationRoot, relative(sourceRoot, source))
    await mkdir(dirname(destination), { recursive: true })
    await copyFile(source, destination)
  }
}

function projectTarget(tool, directory) {
  return tool === 'oxfmt' ? oxfmtProjectAlias(directory) : directory
}

function oxfmtProjectAlias(directory) {
  const name = relative(workDirectory, directory).replaceAll(/[^a-zA-Z0-9]+/g, '-').replaceAll(/^-|-$/g, '')
  return join('/tmp', `worsier-benchmark-${name}`)
}

async function ensureOxfmtAlias(directory) {
  const alias = oxfmtProjectAlias(directory)
  await rm(alias, { force: true })
  await symlink(directory, alias, 'dir')
}

async function findNamedFiles(directory, targetName) {
  const found = []
  const entries = await readdir(directory, { withFileTypes: true })
  for (const entry of entries) {
    const path = join(directory, entry.name)
    if (entry.isDirectory()) {
      found.push(...await findNamedFiles(path, targetName))
    } else if (entry.isFile() && (targetName === undefined || entry.name === targetName)) {
      found.push(path)
    }
  }
  return found.sort()
}

async function readJsonConfig(path) {
  return JSON.parse(await readFile(path, 'utf8'))
}

function runShell(command, label) {
  const result = spawnSync('/bin/sh', ['-c', command], { cwd: root, encoding: 'utf8', maxBuffer: 100 * 1024 * 1024 })
  if (result.status !== 0) {
    throw commandFailure(label, '/bin/sh', ['-c', command], result)
  }
  return result
}

function capture(command, args) {
  const result = run(command, args, { label: command })
  return result.stdout.trim()
}

function run(command, args, { label, inherit = false } = {}) {
  const result = spawnSync(command, args, {
    cwd: root,
    encoding: 'utf8',
    maxBuffer: 100 * 1024 * 1024,
    stdio: inherit ? 'inherit' : ['ignore', 'pipe', 'pipe']
  })
  if (result.status !== 0) {
    throw commandFailure(label ?? command, command, args, result)
  }
  return result
}

export function commandFailure(label, command, args, result) {
  const invocation = [command, ...args].join(' ')
  const stdout = typeof result.stdout === 'string' ? result.stdout : ''
  const stderr = typeof result.stderr === 'string' ? result.stderr : ''
  const spawnError = result.error instanceof Error ? result.error.stack ?? result.error.message : String(result.error ?? '')
  return new Error(`${label} failed with exit code ${result.status ?? 'unknown'}\nCommand: ${invocation}\nspawn error:\n${spawnError}\nstdout:\n${stdout}\nstderr:\n${stderr}`, result.error instanceof Error ? { cause: result.error } : undefined)
}

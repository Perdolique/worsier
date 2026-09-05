export const toolNames = ['worsier', 'prettier', 'oxfmt'] as const
export const scenarioNames = ['small', 'parser', 'projectWrite', 'projectCheck'] as const

export type ToolName = typeof toolNames[number]
export type ScenarioName = typeof scenarioNames[number]
export type ToolRecord<Value> = Record<ToolName, Value>

export interface Statistics {
  max: number
  mean: number
  median: number
  min: number
  samples: number[]
  stddev: number
}

export interface FileDescription {
  bytes: number
  files: number
  sha256: string
}

export interface SourceManifest {
  paths: string[]
  sha256?: string
}

export interface HashedSourceManifest extends SourceManifest {
  sha256: string
}

export interface BenchmarkFixture extends FileDescription {
  lineEndings?: 'crlf' | 'lf'
  revision?: string
  source: string
  tag?: string
}

export interface BenchmarkFixtures {
  [name: string]: BenchmarkFixture
  outline: BenchmarkFixture
  parser: BenchmarkFixture
  small: BenchmarkFixture
}

export interface BenchmarkCommands {
  check(tool: ToolName, directory: string): string
  stdin(tool: ToolName, input: string, output: string): string
  write(tool: ToolName, directory: string): string
}

export interface MeasurementSettings {
  rssRuns: number
  runs: number
  warmups: number
}

export interface BenchmarkSettings extends MeasurementSettings {
  cache: false
  concurrency: string
  endOfLine: 'lf'
  lineWidth: number
  semicolons: false
  trailingCommas: false
  worsierVerifyAst: true
}

export interface BenchmarkResult {
  command: string
  inputBytes: number
  maxSeconds: number
  meanSeconds: number
  medianSeconds: number
  minSeconds: number
  peakRssBytes: number
  peakRssSamplesBytes: number[]
  samplesSeconds: number[]
  stddevSeconds: number
  throughputMibPerSecond: number | null
}

export interface BenchmarkScenario {
  displayName: string
  inputBytes: number | null
  inputBytesByTool: ToolRecord<number> | null
  results: ToolRecord<BenchmarkResult>
}

export type BenchmarkScenarios = Record<ScenarioName, BenchmarkScenario>

export interface ToolInfo {
  displayName: string
  version: string
}

export interface BenchmarkEnvironment {
  architecture: string
  cargo: string
  cores: number
  cpu: string
  hyperfine: string
  machineModel: string
  memoryGb: number
  node: string
  osBuild: string
  osName: string
  osVersion: string
  pnpm: string
  powerMode: string
  powerSource: string
  rust: string
}

export interface BenchmarkValidation {
  baselineManifestHash: string
  fileCount: number
  idempotent: true
  outputBytes: ToolRecord<number>
  outputHashes: ToolRecord<Record<string, string>>
}

export interface MicrobenchmarkResult {
  input: string
  inputBytes: number
  measurement: string
  medianSeconds: number
  samplesSeconds: number[]
  throughputMibPerSecond: number | null
}

export interface BenchmarkSource {
  worsierSha: string
}

export interface BenchmarkReport {
  environment: BenchmarkEnvironment
  fixtures: BenchmarkFixtures
  generatedAt: string
  microbenchmarks: MicrobenchmarkResult[]
  scenarios: BenchmarkScenarios
  schemaVersion: 2
  settings: BenchmarkSettings
  source: BenchmarkSource
  tools: ToolRecord<ToolInfo>
  validation: BenchmarkValidation
}

export interface ScenarioDefinition {
  before?: () => Promise<unknown>
  bytes?: number
  bytesByTool?: ToolRecord<number>
  commands: ToolRecord<string>
  displayName: string
  name: ScenarioName
  prepareCommand?: string
  prepareRss?: (tool: ToolName) => Promise<void>
}

export interface CommandResult {
  error?: Error
  status: number | null
  stderr: string
  stdout: string
}

export interface RunOptions {
  inherit?: boolean
  label?: string
}

export type CaptureCommand = (command: string, args: string[]) => string

export interface CollectEnvironmentOptions {
  captureCommand?: CaptureCommand
  platform?: NodeJS.Platform
  systemMemory?: number
}

export interface WorsierSemicolonConfig {
  classMembers: 'asNeeded'
  statements: 'asNeeded'
  typeMembers: 'asNeeded'
}

export interface WorsierRulesConfig {
  quoteStyle: 'single'
  semicolons: WorsierSemicolonConfig
  trailingCommas: 'never'
}

export interface WorsierBenchmarkConfig {
  lineWidth: number
  rules: WorsierRulesConfig
  verifyAst: true
}

export interface FullSourceBenchmarkConfig {
  endOfLine: 'lf'
  printWidth: number
  semi: false
  trailingComma: 'none'
}

export interface FullSourceSemicolonConfig {
  semi: unknown
}

export interface WorsierSemicolonValues {
  classMembers?: unknown
  statements?: unknown
  typeMembers?: unknown
}

export interface WorsierSemicolonRules {
  semicolons: WorsierSemicolonValues
}

export interface WorsierSemicolonCheckConfig {
  rules: WorsierSemicolonRules
}

export interface BenchmarkPackage {
  devDependencies: Record<string, string>
}

export interface CriterionEstimate {
  median: CriterionPointEstimate
}

export interface CriterionPointEstimate {
  point_estimate: number
}

export interface CriterionSample {
  iters: number[]
  times: number[]
}

export interface CriterionBenchmark {
  throughput?: CriterionThroughput
}

export interface CriterionThroughput {
  Bytes?: number
}

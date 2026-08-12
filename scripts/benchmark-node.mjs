import { spawnSync } from 'node:child_process'
import { performance } from 'node:perf_hooks'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

const root = resolve(import.meta.dirname, '..')
const packageDirectory = join(root, 'packages/npm')
const packageUrl = pathToFileURL(join(packageDirectory, 'dist/index.js')).href
const cliPath = join(packageDirectory, 'bin/worsier.js')
const iterations = Number.parseInt(process.env.WORSIER_BENCH_ITERATIONS ?? '12', 10)

if (!Number.isSafeInteger(iterations) || iterations < 1) {
  throw new Error('WORSIER_BENCH_ITERATIONS must be a positive integer')
}

const apiProgram = [
  `import { format } from ${JSON.stringify(packageUrl)}`,
  `await format('cold-start.ts', "import{one,type Two}from'pkg';const value={items:[1,2,3]};", {})`
].join('; ')

benchmark('node_api_cold_start', ['--input-type=module', '--eval', apiProgram])
benchmark('node_cli_cold_start', [cliPath, '--version'])

function benchmark(name, args) {
  const samples = []
  for (let index = 0; index < iterations; index += 1) {
    const started = performance.now()
    const result = spawnSync(process.execPath, args, {
      cwd: root,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe']
    })
    const elapsed = performance.now() - started
    if (result.status !== 0) {
      throw new Error(
        `${process.execPath} ${args.join(' ')} failed\n${result.stdout}${result.stderr}`
      )
    }
    samples.push(elapsed)
  }

  samples.sort((left, right) => left - right)
  const median = percentile(samples, 0.5)
  const p95 = percentile(samples, 0.95)
  console.log(
    `${name}: median=${median.toFixed(2)}ms p95=${p95.toFixed(2)}ms iterations=${iterations}`
  )
}

function percentile(sorted, quantile) {
  const position = (sorted.length - 1) * quantile
  const lower = Math.floor(position)
  const upper = Math.ceil(position)
  const fraction = position - lower
  return sorted[lower] + (sorted[upper] - sorted[lower]) * fraction
}

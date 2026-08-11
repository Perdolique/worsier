import { createRequire } from 'node:module'

export interface NativeBinding {
  format(fileName: string, sourceText: string, configJson: string): Promise<string>
  runCli(args: string[]): Promise<number>
}

interface ProcessReportHeader {
  glibcVersionRuntime?: string
}

interface ProcessReport {
  header?: ProcessReportHeader
}

const require = createRequire(import.meta.url)

const platformPackages: Readonly<Record<string, string>> = {
  'darwin-arm64': 'worsier-darwin-arm64',
  'darwin-x64': 'worsier-darwin-x64',
  'linux-arm64-gnu': 'worsier-linux-arm64-gnu',
  'linux-arm64-musl': 'worsier-linux-arm64-musl',
  'linux-x64-gnu': 'worsier-linux-x64-gnu',
  'linux-x64-musl': 'worsier-linux-x64-musl',
  'win32-arm64-msvc': 'worsier-win32-arm64-msvc',
  'win32-x64-msvc': 'worsier-win32-x64-msvc'
}

let binding: NativeBinding | undefined

function libc(): 'gnu' | 'musl' {
  if (process.platform !== 'linux') {
    return 'gnu'
  }

  const report = process.report?.getReport() as ProcessReport | undefined
  return report?.header?.glibcVersionRuntime ? 'gnu' : 'musl'
}

export function loadBinding(): NativeBinding {
  if (binding) {
    return binding
  }

  const suffix = process.platform === 'linux' ? `-${libc()}` : process.platform === 'win32' ? '-msvc' : ''
  const target = `${process.platform}-${process.arch}${suffix}`
  const packageName = platformPackages[target]

  if (!packageName) {
    throw new Error(`Worsier does not support ${target}.`)
  }

  try {
    binding = require(packageName) as NativeBinding
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error)
    throw new Error(`Failed to load the Worsier native package ${packageName}: ${reason}`, { cause: error })
  }

  return binding
}

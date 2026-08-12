import { spawnSync } from 'node:child_process'

function platformName() {
  let suffix = ''
  if (process.platform === 'linux') {
    const report = process.report?.getReport()
    suffix = report?.header?.glibcVersionRuntime ? '-gnu' : '-musl'
  } else if (process.platform === 'win32') {
    suffix = '-msvc'
  }

  return `${process.platform}-${process.arch}${suffix}`
}

const platform = platformName()
const pnpm = platformCommand('pnpm', [
  'exec',
  'napi',
  'build',
  '--manifest-path',
  '../../crates/napi/Cargo.toml',
  '--output-dir',
  `../../npm/${platform}`,
  '--platform',
  '--release',
  '--no-js'
])
const result = spawnSync(
  pnpm.command,
  pnpm.args,
  {
    cwd: import.meta.dirname,
    encoding: 'utf8',
    stdio: 'inherit'
  }
)

if (result.status !== 0) {
  process.exitCode = result.status ?? 1
}

function platformCommand(command, args) {
  if (process.platform === 'win32') {
    return {
      command: process.env.ComSpec ?? 'cmd.exe',
      args: ['/d', '/s', '/c', `${command}.cmd`, ...args]
    }
  }

  return { command, args }
}

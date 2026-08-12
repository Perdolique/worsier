import assert from 'node:assert/strict'
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { basename, join, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'

const root = resolve(import.meta.dirname, '../../..')
const packageDirectory = join(root, 'packages/npm')
const platform = platformName()
const platformDirectory = join(root, `npm/${platform}`)
const temporary = await mkdtemp(join(tmpdir(), 'worsier-packed-'))
const tarballs = join(temporary, 'tarballs')

await mkdir(tarballs, { recursive: true })
run('pnpm', ['pack', '--pack-destination', tarballs], packageDirectory)
run('pnpm', ['pack', '--pack-destination', tarballs], platformDirectory)

const rootVersion = JSON.parse(await readFile(join(packageDirectory, 'package.json'), 'utf8')).version
const platformVersion = JSON.parse(await readFile(join(platformDirectory, 'package.json'), 'utf8')).version
const rootTarball = join(tarballs, `worsier-${rootVersion}.tgz`)
const platformTarball = join(tarballs, `worsier-${platform}-${platformVersion}.tgz`)
assertTarballIncludesLicense(rootTarball, 'worsier')
assertTarballIncludesLicense(platformTarball, `worsier-${platform}`)
const project = join(temporary, 'project')
await mkdir(project, { recursive: true })
await writeFile(join(project, 'package.json'), '{"type":"module","private":true}\n')
run('npm', ['install', '--ignore-scripts', rootTarball, platformTarball], project)

const executable = join(project, 'node_modules/worsier/bin/worsier.js')
const version = run(process.execPath, [executable, '--version'], project)
assert.equal(version.stdout.trim(), `worsier ${rootVersion}`)
run(process.execPath, [executable, '--init'], project)
await writeFile(join(project, 'sample.ts'), "import{value}from'pkg';const raw={items:[1,2]};")
run(process.execPath, [executable, '--write', 'sample.ts'], project)
assert.equal(
  await readFile(join(project, 'sample.ts'), 'utf8'),
  "import { value } from 'pkg';\n\nconst raw={items:[1,2]};"
)

const api = run(
  process.execPath,
  [
    '--input-type=module',
    '--eval',
    `import { format } from 'worsier'; console.log(await format('sample.ts', "import{packed}from'pkg';const raw=[1,2];", {}))`
  ],
  project
)
assert.equal(api.stdout, "import { packed } from 'pkg';\n\nconst raw=[1,2];\n")

console.log(`Packed installation smoke passed in ${basename(project)}`)

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

function assertTarballIncludesLicense(tarball, packageName) {
  const listing = run('tar', ['-tzf', tarball])
  const files = listing.stdout.split('\n')
  assert.ok(files.includes('package/LICENSE'), `${packageName} tarball must contain LICENSE`)
}

function run(command, args, cwd = root) {
  const executable = platformCommand(command, args)
  const result = spawnSync(executable.command, executable.args, {
    cwd,
    encoding: 'utf8',
    env: {
      ...process.env,
      npm_config_cache: join(temporary, 'npm-cache'),
      npm_config_recursive: undefined
    },
    stdio: ['ignore', 'pipe', 'pipe']
  })
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(' ')} failed\n${result.stdout}${result.stderr}`)
  }
  return result
}

function platformCommand(command, args) {
  if (process.platform === 'win32' && (command === 'npm' || command === 'pnpm')) {
    return {
      command: process.env.ComSpec ?? 'cmd.exe',
      args: ['/d', '/s', '/c', `${command}.cmd`, ...args]
    }
  }

  return { command, args }
}

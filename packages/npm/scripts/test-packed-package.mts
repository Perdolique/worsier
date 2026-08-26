import assert from 'node:assert/strict'
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { basename, join, resolve } from 'node:path'
import { spawnSync, type SpawnSyncReturns } from 'node:child_process'

import { tarListingContains } from './tar-listing.mts'

interface PlatformCommand {
  args: string[]
  command: string
}

interface ProcessReportHeader {
  glibcVersionRuntime?: string
}

interface ProcessReport {
  header?: ProcessReportHeader
}

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
assertTarballIncludesPublicConfig(rootTarball)
const project = join(temporary, 'project')
await mkdir(project, { recursive: true })
await writeFile(join(project, 'package.json'), '{"type":"module","private":true}\n')
run('npm', ['install', '--ignore-scripts', rootTarball, platformTarball], project)
const installedPackage = join(project, 'node_modules/worsier')
const installedSchema = JSON.parse(await readFile(join(installedPackage, 'configuration_schema.json'), 'utf8'))
assert.equal(installedSchema.properties.rules.default.objectPropertySpacing, true)
assert.equal(installedSchema.$defs.RulesConfig.properties.objectPropertySpacing.default, true)
assert.equal(installedSchema.properties.rules.default.statementSpacing.multilineCallStatements, 'separate')
assert.equal(installedSchema.$defs.RulesConfig.properties.statementSpacing.default.multilineCallStatements, 'separate')
assert.equal(installedSchema.$defs.StatementSpacingConfig.properties.multilineCallStatements.default, 'separate')
assert.equal(installedSchema.properties.rules.default.semicolons.typeMembers, 'always')
assert.equal(installedSchema.$defs.RulesConfig.properties.semicolons.default.typeMembers, 'always')
assert.equal(installedSchema.$defs.SemicolonConfig.properties.typeMembers.default, 'always')
assert.match(await readFile(join(installedPackage, 'dist/types.d.ts'), 'utf8'), /objectPropertySpacing\?: boolean/)
assert.match(await readFile(join(installedPackage, 'dist/types.d.ts'), 'utf8'), /multilineCallStatements\?: 'separate' \| 'compact' \| 'off'/)

const executable = join(project, 'node_modules/worsier/bin/worsier.js')
const version = run(process.execPath, [executable, '--version'], project)
assert.equal(version.stdout.trim(), `worsier ${rootVersion}`)
run(process.execPath, [executable, '--init'], project)
assert.equal(
  await readFile(join(project, 'worsier.jsonc'), 'utf8'),
  '{\n  "$schema": "./node_modules/worsier/configuration_schema.json",\n  "lineWidth": 120,\n  "verifyAst": true,\n  "rules": {\n    "importLayout": true,\n    "interfaceLayout": 0,\n    "objectPropertySpacing": true,\n    "statementSpacing": {\n      "controlFlowStatements": "separate",\n      "imports": "separate",\n      "multilineCallStatements": "separate",\n      "returnStatements": "separate",\n      "typeAliases": "separate",\n      "variableDeclarations": "separate"\n    },\n    "semicolons": {\n      "statements": "asNeeded",\n      "classMembers": "asNeeded",\n      "typeMembers": "always"\n    },\n    "trailingCommas": "never"\n  },\n  "ignorePatterns": []\n}\n'
)
await writeFile(
  join(project, 'worsier.jsonc'),
  '{\n  "$schema": "./node_modules/worsier/configuration_schema.json",\n  "lineWidth": 120,\n  "verifyAst": true,\n  "rules": {\n    // legacy layout\n    "imports": false,\n    "variables": false\n  },\n  "ignorePatterns": []\n}\n'
)
const updatedConfig = run(process.execPath, [executable, '--update-config'], project)
assert.match(updatedConfig.stdout, /Migrated rules\.imports/)
assert.match(updatedConfig.stdout, /Migrated rules\.variables/)
assert.equal(
  await readFile(join(project, 'worsier.jsonc'), 'utf8'),
  '{\n  "$schema": "./node_modules/worsier/configuration_schema.json",\n  "lineWidth": 120,\n  "verifyAst": true,\n  "rules": {\n    // legacy layout\n    "importLayout": false,\n    "interfaceLayout": 0,\n    "objectPropertySpacing": true,\n    "statementSpacing": {\n      "controlFlowStatements": "separate",\n      "imports": "off",\n      "multilineCallStatements": "separate",\n      "returnStatements": "separate",\n      "typeAliases": "separate",\n      "variableDeclarations": "off"\n    },\n    "semicolons": {\n      "statements": "asNeeded",\n      "classMembers": "asNeeded",\n      "typeMembers": "always"\n    },\n    "trailingCommas": "never"\n  },\n  "ignorePatterns": []\n}\n'
)
await writeFile(join(project, 'sample.ts'), 'const first=1;let second=2;')
run(process.execPath, [executable, '--write', 'sample.ts'], project)
assert.equal(
  await readFile(join(project, 'sample.ts'), 'utf8'),
  'const first=1;let second=2'
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
assert.equal(api.stdout, "import { packed } from 'pkg'\n\nconst raw=[1,2]\n")

const objectSpacing = run(
  process.execPath,
  [
    '--input-type=module',
    '--eval',
    `import { format } from 'worsier'; console.log(await format('sample.ts', 'const value={first:1,second:2};'))`
  ],
  project
)
assert.equal(objectSpacing.stdout, 'const value={\n  first:1,\n  second:2\n}\n')

const multilineCallSpacing = run(
  process.execPath,
  [
    '--input-type=module',
    '--eval',
    `import { format } from 'worsier'; console.log(await format('sample.ts', "async function submit() {\\n  prepare()\\n  await requestFetch('/api', {\\n    body: formData\\n  })\\n  finish()\\n}"))`
  ],
  project
)
assert.equal(
  multilineCallSpacing.stdout,
  "async function submit() {\n  prepare()\n\n  await requestFetch('/api', {\n    body: formData\n  })\n\n  finish()\n}\n"
)

const interfaceLayout = run(
  process.execPath,
  [
    '--input-type=module',
    '--eval',
    `import { format } from 'worsier'; console.log(await format('sample.ts', 'interface Shape { value: string; }'))`
  ],
  project
)
assert.equal(interfaceLayout.stdout, 'interface Shape {\n  value: string;\n}\n')

const variablesDisabled = run(
  process.execPath,
  [
    '--input-type=module',
    '--eval',
    `import { format } from 'worsier'; console.log(await format('sample.ts', 'const first=1;let second=2;', { rules: { statementSpacing: { variableDeclarations: 'off' } } }))`
  ],
  project
)
assert.equal(variablesDisabled.stdout, 'const first=1;let second=2\n')

const controlFlowSpacing = run(
  process.execPath,
  [
    '--input-type=module',
    '--eval',
    `import { format } from 'worsier'; console.log(await format('sample.ts', 'function f(){before();if(ok)work();after();}', { rules: { importLayout: false, interfaceLayout: 'off', objectPropertySpacing: false, statementSpacing: { controlFlowStatements: 'separate', imports: 'off', multilineCallStatements: 'off', returnStatements: 'off', typeAliases: 'off', variableDeclarations: 'off' }, semicolons: { statements: 'off', classMembers: 'off', typeMembers: 'off' }, trailingCommas: 'off' } }))`
  ],
  project
)
assert.equal(
  controlFlowSpacing.stdout,
  'function f(){\n  before();\n\n  if(ok)work();\n\n  after();\n}\n'
)

const compactTypeAliases = run(
  process.execPath,
  [
    '--input-type=module',
    '--eval',
    `import { format } from 'worsier'; console.log(await format('sample.ts', 'type A=1;type B={\\n value:string\\n};\\n\\n\\nrun();', { rules: { importLayout: false, interfaceLayout: 'off', objectPropertySpacing: false, statementSpacing: { controlFlowStatements: 'off', imports: 'off', multilineCallStatements: 'off', returnStatements: 'off', typeAliases: 'compact', variableDeclarations: 'off' }, semicolons: { statements: 'off', classMembers: 'off', typeMembers: 'off' }, trailingCommas: 'off' } }))`
  ],
  project
)
assert.equal(compactTypeAliases.stdout, 'type A=1;\ntype B={\n value:string\n};\nrun();\n')

const trailingAlways = run(
  process.execPath,
  [
    '--input-type=module',
    '--eval',
    `import { format } from 'worsier'; console.log(await format('sample.ts', 'const value={\\n  item: true\\n};', { rules: { importLayout: false, interfaceLayout: 'off', objectPropertySpacing: false, statementSpacing: { controlFlowStatements: 'off', imports: 'off', multilineCallStatements: 'off', returnStatements: 'off', typeAliases: 'off', variableDeclarations: 'off' }, semicolons: { statements: 'off', classMembers: 'off', typeMembers: 'off' }, trailingCommas: 'always' } }))`
  ],
  project
)
assert.equal(trailingAlways.stdout, 'const value={\n  item: true,\n};\n')

const granularSemicolons = run(
  process.execPath,
  [
    '--input-type=module',
    '--eval',
    `import { format } from 'worsier'; console.log(await format('sample.ts', 'const runtime=1;\\nclass Example { field=1; }\\ninterface Shape { value: string; }', { rules: { importLayout: false, interfaceLayout: 'off', objectPropertySpacing: false, statementSpacing: { controlFlowStatements: 'off', imports: 'off', multilineCallStatements: 'off', returnStatements: 'off', typeAliases: 'off', variableDeclarations: 'off' }, semicolons: { statements: 'off', classMembers: 'asNeeded', typeMembers: 'always' }, trailingCommas: 'off' } }))`
  ],
  project
)
assert.equal(
  granularSemicolons.stdout,
  'const runtime=1;\nclass Example { field=1 }\ninterface Shape { value: string; }\n'
)

console.log(`Packed installation smoke passed in ${basename(project)}`)

function platformName(): string {
  let suffix = ''
  if (process.platform === 'linux') {
    const report = process.report?.getReport() as ProcessReport | undefined
    suffix = report?.header?.glibcVersionRuntime ? '-gnu' : '-musl'
  } else if (process.platform === 'win32') {
    suffix = '-msvc'
  }

  return `${process.platform}-${process.arch}${suffix}`
}

function assertTarballIncludesLicense(tarball: string, packageName: string): void {
  const listing = run('tar', ['-tzf', tarball])
  assert.ok(
    tarListingContains(listing.stdout, 'package/LICENSE'),
    `${packageName} tarball must contain LICENSE`
  )
}

function assertTarballIncludesPublicConfig(tarball: string): void {
  const listing = run('tar', ['-tzf', tarball])
  assert.ok(tarListingContains(listing.stdout, 'package/configuration_schema.json'))
  assert.ok(tarListingContains(listing.stdout, 'package/dist/types.d.ts'))
}

function run(command: string, args: string[], cwd = root): SpawnSyncReturns<string> {
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

function platformCommand(command: string, args: string[]): PlatformCommand {
  if (process.platform === 'win32' && (command === 'npm' || command === 'pnpm')) {
    return {
      command: process.env.ComSpec ?? 'cmd.exe',
      args: ['/d', '/s', '/c', `${command}.cmd`, ...args]
    }
  }

  return { command, args }
}

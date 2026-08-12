import assert from 'node:assert/strict'
import { mkdtemp, readFile, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { basename, join, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'

const root = resolve(import.meta.dirname, '..')
const packageDirectory = join(root, 'packages/npm')
const platformDirectory = join(root, 'npm/linux-x64-gnu')
const temporary = await mkdtemp(join(tmpdir(), 'worsier-packed-'))
const tarballs = join(temporary, 'tarballs')

run('mkdir', ['-p', tarballs])
run('pnpm', ['pack', '--pack-destination', tarballs], packageDirectory)
run('pnpm', ['pack', '--pack-destination', tarballs], platformDirectory)

const rootTarball = join(tarballs, 'worsier-0.1.0.tgz')
const platformTarball = join(tarballs, 'worsier-linux-x64-gnu-0.1.0.tgz')
const project = join(temporary, 'project')
run('mkdir', ['-p', project])
await writeFile(join(project, 'package.json'), '{"type":"module","private":true}\n')
run('npm', ['install', '--ignore-scripts', rootTarball, platformTarball], project)

run(join(project, 'node_modules/.bin/worsier'), ['--init'], project)
await writeFile(join(project, 'sample.ts'), "import{value}from'pkg';const raw={items:[1,2]};")
run(join(project, 'node_modules/.bin/worsier'), ['--write', 'sample.ts'], project)
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

function run(command, args, cwd = root) {
  const result = spawnSync(command, args, {
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

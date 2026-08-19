import assert from 'node:assert/strict'
import { readFile, readdir } from 'node:fs/promises'
import { spawnSync } from 'node:child_process'

interface VersionedPackage {
  version: string
}

interface NpmPackage extends VersionedPackage {
  files: string[]
  name: string
  optionalDependencies: Record<string, string>
}

interface PlatformPackage extends VersionedPackage {
  files: string[]
  main: string
  name: string
}

interface CargoPackage extends VersionedPackage {
  id: string
  name: string
}

interface CargoMetadata {
  packages: CargoPackage[]
  workspace_members: string[]
}

interface ReleaseExtraFile {
  jsonpath: string
  path: string
  type: string
}

interface ReleaseComponent {
  'extra-files': Array<string | ReleaseExtraFile>
  'package-name': string
  'release-type': string
}

interface ReleasePleaseConfig {
  packages: Record<string, ReleaseComponent | undefined>
}

const root = new URL('..', import.meta.url)
const cargo = await readFile(new URL('Cargo.toml', root), 'utf8')
const license = await readFile(new URL('LICENSE', root), 'utf8')
const cargoVersion = cargo.match(/^version = "([^"]+)"$/m)?.[1]
assert.ok(cargoVersion, 'Cargo.toml must define workspace.package.version')

const rootPackage = JSON.parse(await readFile(new URL('package.json', root), 'utf8')) as VersionedPackage
const npmPackage = JSON.parse(await readFile(new URL('packages/npm/package.json', root), 'utf8')) as NpmPackage
assert.equal(rootPackage.version, npmPackage.version, 'Workspace and npm versions must match')
assert.equal(cargoVersion, npmPackage.version, 'Cargo and npm versions must match')
assert.ok(npmPackage.files.includes('LICENSE'), 'worsier package must include LICENSE')
assert.equal(
  await readFile(new URL('packages/npm/LICENSE', root), 'utf8'),
  license,
  'worsier package LICENSE must match the repository license'
)

const platformRoot = new URL('npm/', root)
const platformDirectories = await readdir(platformRoot, { withFileTypes: true })
const platformPackageNames = new Set<string>()
for (const directory of platformDirectories) {
  if (!directory.isDirectory()) {
    continue
  }

  const manifestPath = new URL(`${directory.name}/package.json`, platformRoot)
  const manifest = JSON.parse(await readFile(manifestPath, 'utf8')) as PlatformPackage
  assert.equal(manifest.version, npmPackage.version, `${manifest.name} version must match worsier`)
  assert.ok(manifest.files.includes(manifest.main), `${manifest.name} must include its native binding`)
  assert.ok(manifest.files.includes('LICENSE'), `${manifest.name} must include LICENSE`)
  assert.equal(
    await readFile(new URL(`${directory.name}/LICENSE`, platformRoot), 'utf8'),
    license,
    `${manifest.name} LICENSE must match the repository license`
  )
  platformPackageNames.add(manifest.name)
}

assert.deepEqual(
  new Set(Object.keys(npmPackage.optionalDependencies)),
  platformPackageNames,
  'worsier optionalDependencies must match the platform package manifests'
)

const metadata = spawnSync('cargo', ['metadata', '--locked', '--format-version', '1'], {
  cwd: root,
  encoding: 'utf8',
  stdio: ['ignore', 'pipe', 'pipe']
})
if (metadata.status !== 0) {
  throw new Error(`cargo metadata --locked failed\n${metadata.stdout}${metadata.stderr}`)
}
const cargoMetadata = JSON.parse(metadata.stdout) as CargoMetadata
const workspacePackageIds = new Set(cargoMetadata.workspace_members)
const workspacePackages = cargoMetadata.packages.filter((entry) => workspacePackageIds.has(entry.id))
for (const workspacePackage of workspacePackages) {
  assert.equal(workspacePackage.version, npmPackage.version, `${workspacePackage.name} version must match worsier`)
}

const releasePlease = JSON.parse(await readFile(new URL('release-please-config.json', root), 'utf8')) as ReleasePleaseConfig
const releaseComponent = releasePlease.packages['.']
assert.ok(releaseComponent, 'Release Please must consider commits from the complete repository')
assert.equal(releaseComponent['release-type'], 'node', 'Release Please must use the Node release strategy')
assert.equal(releaseComponent['package-name'], npmPackage.name, 'Release Please package name must match worsier')
assert.ok(releaseComponent['extra-files'].includes('/packages/npm/package.json'), 'Release Please must update the published npm package')

const releaseManifest = JSON.parse(await readFile(new URL('.release-please-manifest.json', root), 'utf8'))
assert.deepEqual(releaseManifest, { '.': npmPackage.version }, 'Release Please must track the root component version')

const releaseWorkflow = await readFile(new URL('.github/workflows/release.yml', root), 'utf8')
for (const output of ['release_created', 'sha', 'tag_name']) {
  assert.ok(releaseWorkflow.includes(`steps.release.outputs.${output}`), `Release workflow must use the root ${output} output`)
}
assert.ok(!releaseWorkflow.includes("steps.release.outputs['packages/npm--"), 'Release workflow must not use path-prefixed outputs')

const cargoLockUpdate = releaseComponent['extra-files'].find(
  (entry): entry is ReleaseExtraFile => typeof entry === 'object' && entry.path === '/Cargo.lock'
)
assert.ok(cargoLockUpdate, 'Release Please must update Cargo.lock')
for (const workspacePackage of workspacePackages) {
  assert.ok(cargoLockUpdate.jsonpath.includes(`@.name.value == '${workspacePackage.name}'`), `Release Please Cargo.lock update must include ${workspacePackage.name}`)
}

const fuzzMetadata = spawnSync(
  'cargo',
  ['metadata', '--manifest-path', 'fuzz/Cargo.toml', '--locked', '--no-deps', '--format-version', '1'],
  {
    cwd: root,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe']
  }
)
if (fuzzMetadata.status !== 0) {
  throw new Error(`fuzz cargo metadata --locked failed\n${fuzzMetadata.stdout}${fuzzMetadata.stderr}`)
}

console.log(`All package versions match ${npmPackage.version}`)

import assert from 'node:assert/strict'
import { readFile, readdir } from 'node:fs/promises'
import { spawnSync } from 'node:child_process'

const root = new URL('..', import.meta.url)
const cargo = await readFile(new URL('Cargo.toml', root), 'utf8')
const license = await readFile(new URL('LICENSE', root), 'utf8')
const cargoVersion = cargo.match(/^version = "([^"]+)"$/m)?.[1]
assert.ok(cargoVersion, 'Cargo.toml must define workspace.package.version')

const npmPackage = JSON.parse(await readFile(new URL('packages/npm/package.json', root), 'utf8'))
assert.equal(cargoVersion, npmPackage.version, 'Cargo and npm versions must match')
assert.ok(npmPackage.files.includes('LICENSE'), 'worsier package must include LICENSE')
assert.equal(
  await readFile(new URL('packages/npm/LICENSE', root), 'utf8'),
  license,
  'worsier package LICENSE must match the repository license'
)

const platformRoot = new URL('npm/', root)
const platformDirectories = await readdir(platformRoot, { withFileTypes: true })
const platformPackageNames = new Set()
for (const directory of platformDirectories) {
  if (!directory.isDirectory()) {
    continue
  }

  const manifestPath = new URL(`${directory.name}/package.json`, platformRoot)
  const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
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
const cargoMetadata = JSON.parse(metadata.stdout)
const workspacePackageIds = new Set(cargoMetadata.workspace_members)
const workspacePackages = cargoMetadata.packages.filter((entry) => workspacePackageIds.has(entry.id))
for (const workspacePackage of workspacePackages) {
  assert.equal(workspacePackage.version, npmPackage.version, `${workspacePackage.name} version must match worsier`)
}

const releasePlease = JSON.parse(await readFile(new URL('release-please-config.json', root), 'utf8'))
const cargoLockUpdate = releasePlease.packages['packages/npm']['extra-files'].find((entry) => typeof entry === 'object' && entry.path === '/Cargo.lock')
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

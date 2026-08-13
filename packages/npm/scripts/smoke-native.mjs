import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { resolve } from 'node:path'

const addonPath = process.argv[2]
if (!addonPath) {
  throw new Error('Provide a native addon path')
}

const require = createRequire(import.meta.url)
const binding = require(resolve(addonPath))
const output = await binding.format('smoke.ts', "import{value}from'pkg';const raw={items:[1,2]};", '{}')
assert.equal(output, "import { value } from 'pkg';\n\nconst raw={items:[1,2]};")
const variablesDisabled = await binding.format(
  'smoke.ts',
  'const first=1;let second=2;',
  '{"rules":{"statementSpacing":{"variableDeclarations":"off"}}}'
)
assert.equal(variablesDisabled, 'const first=1;let second=2;')
const trailingAlways = await binding.format(
  'smoke.ts',
  'const value={\n  item: true\n};',
  '{"rules":{"importLayout":false,"statementSpacing":{"imports":"off","variableDeclarations":"off"},"trailingCommas":"always"}}'
)
assert.equal(trailingAlways, 'const value={\n  item: true,\n};')
console.log(`Native smoke passed for ${addonPath}`)

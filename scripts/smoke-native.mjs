import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { resolve } from 'node:path'

const addonPath = process.argv[2]
if (!addonPath) {
  throw new Error('Provide a native addon path')
}

const require = createRequire(import.meta.url)
const binding = require(resolve(addonPath))
const output = await binding.format('smoke.ts', 'const value={items:[1,2]};', '{}')
assert.equal(output, 'const value = { items: [1, 2] };\n')
console.log(`Native smoke passed for ${addonPath}`)

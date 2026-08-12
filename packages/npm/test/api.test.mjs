import assert from 'node:assert/strict'
import { cp, mkdtemp } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import test from 'node:test'

import { format } from '../dist/index.js'

test('formats through the asynchronous native API', async () => {
  const source = "import{one,type Two}from'pkg';const value={items:[1,2]};"
  const output = await format('sample.ts', source, {})
  assert.equal(output, "import { one, type Two } from 'pkg';\n\nconst value={items:[1,2]};")

  const variablesOnly = await format('sample.ts', source, {
    rules: { imports: false }
  })
  assert.equal(
    variablesOnly,
    "import{one,type Two}from'pkg';\n\nconst value={items:[1,2]};"
  )

  const importsOnly = await format(
    'sample.ts',
    "import{value}from'pkg';const first=1;let second=2;",
    { rules: { variables: false } }
  )
  assert.equal(importsOnly, "import { value } from 'pkg';\n\nconst first=1;let second=2;")

  const disabled = await format('sample.ts', source, {
    rules: { imports: false, variables: false }
  })
  assert.equal(disabled, source)
})

test('maps native failures to stable error codes', async () => {
  await assert.rejects(format('sample.ts', 'const value = @;', {}), {
    code: 'PARSE_ERROR'
  })
  await assert.rejects(format('sample.ts', 'const value = 1;', { lineWidth: 0 }), {
    code: 'CONFIG_ERROR'
  })
  await assert.rejects(format('sample.flow', 'const value = 1;', {}), {
    code: 'UNSUPPORTED_SOURCE'
  })
  await assert.rejects(
    format('sample.ts', 'const value = 1;', { quoteStyle: 'single' }),
    (error) => error.code === 'CONFIG_ERROR' && error.message.includes('quoteStyle')
  )
})

test('binding import is lazy and missing packages have a targeted diagnostic', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'worsier-binding-'))
  const bindingPath = join(directory, 'binding.mjs')
  await cp(new URL('../dist/binding.js', import.meta.url), bindingPath)
  const { loadBinding } = await import(pathToFileURL(bindingPath).href)

  assert.throws(loadBinding, /Failed to load the Worsier native package worsier-/)
})

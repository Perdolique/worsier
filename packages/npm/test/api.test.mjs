import assert from 'node:assert/strict'
import { cp, mkdtemp } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import test from 'node:test'

import { format } from '../dist/index.js'

test('formats through the asynchronous native API', async () => {
  const output = await format('sample.ts', 'const value={items:[1,2]};', {})
  assert.equal(output, 'const value = { items: [1, 2] };\n')
})

test('maps native failures to stable error codes', async () => {
  await assert.rejects(format('sample.ts', 'const value = @;', {}), {
    code: 'PARSE_ERROR'
  })
  await assert.rejects(format('sample.ts', 'const value = 1;', { lineWidth: 0 }), {
    code: 'CONFIG_ERROR'
  })
  await assert.rejects(
    format('sample.ts', 'const value = 1;', { objects: { unknown: true } }),
    (error) => error.code === 'CONFIG_ERROR' && error.message.includes('objects.unknown')
  )
})

test('binding import is lazy and missing packages have a targeted diagnostic', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'worsier-binding-'))
  const bindingPath = join(directory, 'binding.mjs')
  await cp(new URL('../dist/binding.js', import.meta.url), bindingPath)
  const { loadBinding } = await import(pathToFileURL(bindingPath).href)

  assert.throws(loadBinding, /Failed to load the Worsier native package worsier-/)
})

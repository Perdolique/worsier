import { writeFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'

import { compileFromFile } from 'json-schema-to-typescript'

const schemaUrl = new URL('../packages/npm/configuration_schema.json', import.meta.url)
const outputUrl = new URL('../packages/npm/src/types.ts', import.meta.url)
const bannerComment = '// Generated from the Rust FormatConfig JSON Schema. Do not edit.\n'
const generatedTypes = await compileFromFile(fileURLToPath(schemaUrl), {
  additionalProperties: false,
  bannerComment,
  style: {
    semi: false,
    singleQuote: true,
    tabWidth: 2,
    trailingComma: 'none'
  }
})

await writeFile(outputUrl, generatedTypes)

import assert from 'node:assert/strict'
import { cp, mkdtemp } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import test from 'node:test'

import { format } from '../dist/index.js'

test('formats through the asynchronous native API', async () => {
  const source = "import{one,type Two}from'pkg';const value={items:[1,2]};"
  const output = await format('sample.ts', source)
  assert.equal(output, "import { one, type Two } from 'pkg'\n\nconst value={items:[1,2]}")
  assert.equal(await format('sample.ts', source, {}), output)

  const variablesOnly = await format('sample.ts', source, {
    rules: {
      importLayout: false,
      statementSpacing: { imports: 'off' }
    }
  })
  assert.equal(
    variablesOnly,
    "import{one,type Two}from'pkg'\n\nconst value={items:[1,2]}"
  )

  const importsOnly = await format(
    'sample.ts',
    "import{value}from'pkg';const first=1;let second=2;",
    { rules: { statementSpacing: { variableDeclarations: 'off' } } }
  )
  assert.equal(importsOnly, "import { value } from 'pkg'\n\nconst first=1;let second=2")

  const disabled = await format('sample.ts', source, {
    rules: {
      importLayout: false,
      interfaceLayout: 'off',
      statementSpacing: { imports: 'off', returnStatements: 'off', typeAliases: 'off', variableDeclarations: 'off' },
      semicolons: { statements: 'off', classMembers: 'off', typeMembers: 'off' },
      trailingCommas: 'off'
    }
  })
  assert.equal(disabled, source)

  const partialNested = await format('sample.ts', 'const first=1;work();', {
    rules: { statementSpacing: { imports: 'compact' } }
  })
  assert.equal(partialNested, 'const first=1\n\nwork()')
})

test('formats inline Vue scripts through the native API', async () => {
  const source = '<template>{{ "<template>" }}</template>\n<i18n>{"message":"<!--"}</i18n>\n<script setup lang="ts">import{value}from\'pkg\';const count:number=1;</script>\n<style>.x{color:red}</style>'
  const output = '<template>{{ "<template>" }}</template>\n<i18n>{"message":"<!--"}</i18n>\n<script setup lang="ts">import { value } from \'pkg\'\n\nconst count:number=1</script>\n<style>.x{color:red}</style>'

  assert.equal(await format('component.vue', source), output)
  assert.equal(await format('component.vue', output), output)
  await assert.rejects(format('component.vue', '<script>const value = @;</script>'), {
    code: 'PARSE_ERROR'
  })
  await assert.rejects(format('component.vue', '<script>const value=1;'), {
    code: 'PARSE_ERROR'
  })
})

test('formats interface layouts through the native API', async () => {
  const source = 'interface Shape { value: string; run(): void; }'
  assert.equal(
    await format('sample.ts', source),
    'interface Shape {\n  value: string\n  run(): void\n}'
  )

  const isolatedRules = {
    importLayout: false,
    statementSpacing: { imports: 'off', returnStatements: 'off', typeAliases: 'off', variableDeclarations: 'off' },
    semicolons: { statements: 'off', classMembers: 'off', typeMembers: 'off' },
    trailingCommas: 'off'
  }
  assert.equal(
    await format('sample.ts', source, {
      rules: { ...isolatedRules, interfaceLayout: 1 }
    }),
    'interface Shape {\n  value: string;\n  run(): void;\n}'
  )
  assert.equal(
    await format('sample.ts', source, {
      rules: { ...isolatedRules, interfaceLayout: 2 }
    }),
    source
  )
  assert.equal(
    await format('sample.ts', source, {
      rules: { ...isolatedRules, interfaceLayout: 'off' }
    }),
    source
  )

  const directive =
    'interface Shape {\n  first: string; // @ts-ignore\n  second: MissingOne; third: MissingTwo;\n}'
  assert.equal(
    await format('sample.ts', directive),
    'interface Shape {\n  first: string // @ts-ignore\n  second: MissingOne; third: MissingTwo\n}'
  )

  const multilineContainer =
    'function scope() {\n  before(); interface Local { value: string; } after();\n}'
  assert.equal(
    await format('sample.ts', multilineContainer, {
      rules: { ...isolatedRules, interfaceLayout: 0 }
    }),
    'function scope() {\n  before();\n  interface Local {\n    value: string;\n  }\n  after();\n}'
  )

  const leadingComment = 'interface Shape { first: string;\n// second\nsecond: number; }'
  assert.equal(
    await format('sample.ts', leadingComment, {
      rules: { ...isolatedRules, interfaceLayout: 0 }
    }),
    'interface Shape {\n  first: string;\n  // second\n  second: number;\n}'
  )
})

test('formats type alias spacing through the native API', async () => {
  const source = 'type A=1;type B={\n value:string\n};\n\n\nrun();'
  const output = await format('sample.ts', source, {
    rules: {
      importLayout: false,
      interfaceLayout: 'off',
      statementSpacing: { imports: 'off', returnStatements: 'off', typeAliases: 'compact', variableDeclarations: 'off' },
      semicolons: { statements: 'off', classMembers: 'off', typeMembers: 'off' },
      trailingCommas: 'off'
    }
  })
  assert.equal(output, 'type A=1;\ntype B={\n value:string\n};\nrun();')
})

test('formats return statement spacing through the native API', async () => {
  const source = 'function f(){work();return value;}'
  const output = await format('sample.ts', source, {
    rules: {
      importLayout: false,
      interfaceLayout: 'off',
      statementSpacing: { imports: 'off', returnStatements: 'separate', typeAliases: 'off', variableDeclarations: 'off' },
      semicolons: { statements: 'off', classMembers: 'off', typeMembers: 'off' },
      trailingCommas: 'off'
    }
  })
  assert.equal(output, 'function f(){\n  work();\n\n  return value;\n}')
})

test('formats granular semicolon groups through the native API', async () => {
  const source =
    'const runtime=1;\nclass Example {\n  field=1;\n}\ninterface Shape {\n  value: string;\n}'
  const output = await format('sample.ts', source, {
    rules: {
      importLayout: false,
      statementSpacing: { imports: 'off', returnStatements: 'off', typeAliases: 'off', variableDeclarations: 'off' },
      semicolons: { statements: 'off', classMembers: 'asNeeded', typeMembers: 'always' },
      trailingCommas: 'off'
    }
  })
  assert.equal(
    output,
    'const runtime=1;\nclass Example {\n  field=1\n}\ninterface Shape {\n  value: string;\n}'
  )
})

test('formats trailing commas through the native API', async () => {
  const withoutCommas = 'const value = {\n  items: [\n    1\n  ]\n};'
  const withCommas = 'const value = {\n  items: [\n    1,\n  ],\n};'
  const disabledRules = {
    importLayout: false,
    statementSpacing: { imports: 'off', returnStatements: 'off', typeAliases: 'off', variableDeclarations: 'off' },
    semicolons: { statements: 'off', classMembers: 'off', typeMembers: 'off' }
  }

  assert.equal(await format('sample.ts', withCommas, { rules: disabledRules }), withoutCommas)
  assert.equal(
    await format('sample.ts', withoutCommas, {
      rules: { ...disabledRules, trailingCommas: 'always' }
    }),
    withCommas
  )
  assert.equal(
    await format('sample.ts', withCommas, {
      rules: { ...disabledRules, trailingCommas: 'off' }
    }),
    withCommas
  )
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
  for (const [rules, path] of [
    [{ imports: true }, 'rules.imports'],
    [{ variables: true }, 'rules.variables'],
    [{ interfaceLayout: -1 }, 'rules.interfaceLayout'],
    [{ interfaceLayout: 1.5 }, 'rules.interfaceLayout'],
    [{ interfaceLayout: 'always' }, 'rules.interfaceLayout'],
    [{ semicolons: 'always' }, 'rules.semicolons'],
    [{ semicolons: { statements: 'never' } }, 'rules.semicolons.statements'],
    [{ semicolons: { extra: 'off' } }, 'rules.semicolons.extra'],
    [{ trailingCommas: 'multiline' }, 'rules.trailingCommas'],
    [{ statementSpacing: { imports: 'preserve' } }, 'rules.statementSpacing.imports'],
    [{ statementSpacing: { returnStatements: 'preserve' } }, 'rules.statementSpacing.returnStatements'],
    [{ statementSpacing: { typeAliases: 'preserve' } }, 'rules.statementSpacing.typeAliases'],
    [{ statementSpacing: { extra: 'off' } }, 'rules.statementSpacing.extra']
  ]) {
    await assert.rejects(
      format('sample.ts', 'const value = 1;', { rules }),
      (error) => error.code === 'CONFIG_ERROR' && error.message.includes(path)
    )
  }
})

test('binding import is lazy and missing packages have a targeted diagnostic', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'worsier-binding-'))
  const bindingPath = join(directory, 'binding.mjs')
  await cp(new URL('../dist/binding.js', import.meta.url), bindingPath)
  const { loadBinding } = await import(pathToFileURL(bindingPath).href)

  assert.throws(loadBinding, /Failed to load the Worsier native package worsier-/)
})

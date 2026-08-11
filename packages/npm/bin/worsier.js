#!/usr/bin/env node

import('../dist/binding.js')
  .then(({ loadBinding }) => loadBinding().runCli(process.argv.slice(2)))
  .then((exitCode) => {
    process.exitCode = exitCode
  })
  .catch((error) => {
    console.error(error instanceof Error ? error.message : error)
    process.exitCode = 2
  })

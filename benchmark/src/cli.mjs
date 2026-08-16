#!/usr/bin/env node

import { restoreProjectCopies, runBenchmark, smokeTools, verifyPublishedResults } from './runner.mjs'

const command = process.argv[2]

try {
  if (command === 'run') {
    await runBenchmark()
    console.log('Draft benchmark written to benchmark/.work/latest.{json,md}')
  } else if (command === 'update') {
    await runBenchmark({ publish: true })
    console.log('Published benchmark results and README table updated')
  } else if (command === 'verify') {
    await verifyPublishedResults()
    console.log('Published benchmark results are in sync')
  } else if (command === 'smoke') {
    await smokeTools()
    console.log('Worsier, Prettier, and Oxfmt smoke tests passed')
  } else if (command === 'restore-project') {
    await restoreProjectCopies()
  } else {
    throw new Error('Usage: node benchmark/src/cli.mjs <run|update|verify|smoke|restore-project>')
  }
} catch (error) {
  console.error(error instanceof Error ? error.stack ?? error.message : error)
  process.exitCode = 1
}

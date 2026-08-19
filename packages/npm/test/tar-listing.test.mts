import assert from 'node:assert/strict'
import test from 'node:test'

import { tarListingContains } from '../scripts/tar-listing.mts'

test('recognizes tar entries with Unix and Windows line endings', () => {
  assert.equal(tarListingContains('package/LICENSE\n', 'package/LICENSE'), true)
  assert.equal(tarListingContains('package/LICENSE\r\n', 'package/LICENSE'), true)
  assert.equal(tarListingContains('package/README.md\r\n', 'package/LICENSE'), false)
})

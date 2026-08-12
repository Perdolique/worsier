export function tarListingContains(listing, expectedPath) {
  return listing.split(/\r?\n/).includes(expectedPath)
}

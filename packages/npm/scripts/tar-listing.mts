export function tarListingContains(listing: string, expectedPath: string): boolean {
  return listing.split(/\r?\n/).includes(expectedPath)
}

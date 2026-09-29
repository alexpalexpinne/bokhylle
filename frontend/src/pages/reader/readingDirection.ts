export type ReadingDirection = 'ltr' | 'rtl'

export function resolveReadingDirection({
  profileOverride,
  bookDirection,
  seriesDirection,
  embeddedDirection,
}: {
  profileOverride: ReadingDirection | null
  bookDirection: ReadingDirection | null
  seriesDirection: ReadingDirection | null
  embeddedDirection: ReadingDirection | null
}): ReadingDirection {
  return profileOverride ?? bookDirection ?? seriesDirection ?? embeddedDirection ?? 'ltr'
}

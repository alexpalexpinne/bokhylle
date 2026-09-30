export const PROFILE_MARKS = [
  { id: 'fox', label: 'Fox' },
  { id: 'owl', label: 'Owl' },
  { id: 'cat', label: 'Cat' },
  { id: 'bear', label: 'Bear' },
  { id: 'whale', label: 'Whale' },
  { id: 'book', label: 'Book' },
  { id: 'tree', label: 'Tree' },
  { id: 'mountain', label: 'Mountain' },
  { id: 'moon', label: 'Moon' },
  { id: 'leaf', label: 'Leaf' },
] as const

export type ProfileMarkId = typeof PROFILE_MARKS[number]['id']

export function profileMarkId(id: string | null | undefined): ProfileMarkId | null {
  return PROFILE_MARKS.find((item) => item.id === id)?.id ?? null
}

// Use the bundled list rather than turning arbitrary account data into a URL.
export function profileMarkSrc(id: string | null | undefined) {
  const mark = profileMarkId(id)
  return mark ? `/profile-marks/${mark}.svg` : null
}

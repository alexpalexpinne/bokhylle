export function timestamp(seconds: number | null | undefined) {
  return seconds == null ? '—' : new Date(seconds * 1000).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })
}

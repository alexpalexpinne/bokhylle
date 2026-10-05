// Book data stays in memory, scoped to a profile and a history entry. Longer
// lived view state contains only counts, positions and navigation targets.
const entries = new Map<string, { value: unknown; storedAt: number }>()
let generation = 0
let sessionGeneration = 0
const viewKey = (key: string) => key.startsWith('position:') || key.startsWith('view:')

export function browseGeneration() { return generation }
export function browseSessionGeneration() { return sessionGeneration }

export function readBrowseState<T>(key: string): T | undefined {
  const entry = entries.get(key)
  if (!entry || Date.now() - entry.storedAt >= (viewKey(key) ? 3600_000 : 60_000)) {
    entries.delete(key)
    return undefined
  }
  return entry.value as T
}

export function saveBrowseState(key: string, value: unknown, expectedGeneration: number) {
  if ((viewKey(key) ? sessionGeneration : generation) !== expectedGeneration) return
  if (!entries.has(key) && entries.size >= 90) entries.delete(entries.keys().next().value!)
  entries.set(key, { value, storedAt: Date.now() })
}

export function invalidateBrowseData() {
  generation += 1
  for (const key of entries.keys()) if (!viewKey(key)) entries.delete(key)
}

export function clearBrowseState() {
  generation += 1
  sessionGeneration += 1
  entries.clear()
}

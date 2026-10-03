import type { CollectionDetail } from '../api/collections'
import type {
  AuthorSummary, BookSummary, ContinueReadingItem, HomeRail, SpotlightItem, Updates,
} from '../api/library'

export type HomeSnapshot = {
  spotlight: SpotlightItem[]
  recommendations: SpotlightItem[]
  updates: Updates | null
  recent: BookSummary[]
  continueReading: ContinueReadingItem[]
  highlights: BookSummary[]
  authors: AuthorSummary[]
  shelves: CollectionDetail[]
  rails: HomeRail[]
  householdBooks: number
  loadedAt: number
}

const PREFIX = 'bokhylle.home.v1.'
const TTL_MS = 60_000
const snapshots = new Map<string, HomeSnapshot>()
let generation = 0

export function homeSnapshotGeneration() {
  return generation
}

export function emptyHomeSnapshot(): HomeSnapshot {
  return {
    spotlight: [], recommendations: [], updates: null, recent: [], continueReading: [],
    highlights: [], authors: [], shelves: [], rails: [], householdBooks: 0, loadedAt: 0,
  }
}

export function hasHomeContent(view: HomeSnapshot): boolean {
  const reading = view.continueReading.filter((item) =>
    Number.isFinite(item.percentage) && item.percentage > 0 && item.percentage < 0.995)
  return [view.spotlight, view.recommendations, view.recent, reading,
    view.highlights, view.authors, view.shelves, view.rails, view.updates?.discoveries ?? []]
    .some((items) => items.length > 0)
}

function validSnapshot(value: unknown): value is HomeSnapshot {
  if (!value || typeof value !== 'object') return false
  const snapshot = value as Record<string, unknown>
  return ['spotlight', 'recommendations', 'recent', 'continueReading', 'highlights',
    'authors', 'shelves', 'rails'].every((field) => Array.isArray(snapshot[field])) &&
    typeof snapshot.householdBooks === 'number' &&
    typeof snapshot.loadedAt === 'number' && Number.isFinite(snapshot.loadedAt) &&
    (snapshot.updates === null || (typeof snapshot.updates === 'object' &&
      snapshot.updates !== null && Array.isArray((snapshot.updates as Updates).discoveries)))
}

export function readHomeSnapshot(key: string): HomeSnapshot | undefined {
  let snapshot = snapshots.get(key)
  try {
    if (!snapshot) {
      const saved: unknown = JSON.parse(sessionStorage.getItem(PREFIX + key) ?? 'null')
      if (validSnapshot(saved)) snapshot = saved
    }
    const age = snapshot ? Date.now() - snapshot.loadedAt : Infinity
    if (age < 0 || age >= TTL_MS) {
      snapshots.delete(key)
      sessionStorage.removeItem(PREFIX + key)
      return undefined
    }
  } catch {
    // Storage can be disabled; in-memory snapshots still work.
    if (!snapshot || Date.now() < snapshot.loadedAt || Date.now() - snapshot.loadedAt >= TTL_MS) return undefined
  }
  if (snapshot) snapshots.set(key, snapshot)
  return snapshot
}

export function saveHomeSnapshot(key: string, snapshot: HomeSnapshot, expectedGeneration = generation) {
  // An outstanding refresh must not restore data after sign-out or expiry.
  if (generation !== expectedGeneration) return
  snapshots.set(key, snapshot)
  try {
    sessionStorage.setItem(PREFIX + key, JSON.stringify(snapshot))
  } catch {
    // Session persistence is best-effort; never delay Home for it.
  }
}

export function clearHomeSnapshots() {
  generation += 1
  snapshots.clear()
  try {
    for (const key of Object.keys(sessionStorage)) {
      if (key.startsWith(PREFIX)) sessionStorage.removeItem(key)
    }
  } catch {
    // No persisted snapshots exist when session storage is unavailable.
  }
}

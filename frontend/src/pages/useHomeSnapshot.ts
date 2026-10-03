import { useEffect, useRef, useState } from 'react'
import { fetchCollection, fetchCollections } from '../api/collections'
import { ApiError } from '../api/client'
import {
  fetchAuthors, fetchBooks, fetchContinueReading, fetchHighlights, fetchHomeRails,
  fetchRecent, fetchSpotlight, fetchUpdates,
} from '../api/library'
import { heroBlurb } from '../lib/blurb'
import {
  emptyHomeSnapshot, hasHomeContent, homeSnapshotGeneration, readHomeSnapshot, saveHomeSnapshot,
  type HomeSnapshot,
} from '../lib/homeSnapshot'

// Reveal initial sections in their final order. A later response can fill the
// next reserved slot, but cannot insert a section above books already shown.
export const HOME_SECTIONS = [
  'spotlight', 'updates', 'continueReading', 'recent', 'highlights', 'rails', 'shelves', 'authors',
] as const
export type HomeSection = typeof HOME_SECTIONS[number] | 'householdBooks'
const ALL_SECTIONS: HomeSection[] = [...HOME_SECTIONS, 'householdBooks']

function pendingSections(pending: boolean): Record<HomeSection, boolean> {
  return Object.fromEntries(ALL_SECTIONS.map((section) => [section, pending])) as Record<HomeSection, boolean>
}

export function useHomeSnapshot({ cacheKey, isChild, canDiscover, fromOnboarding }: {
  cacheKey: string
  isChild: boolean
  canDiscover: boolean
  fromOnboarding: boolean
}) {
  const [initial] = useState(() => {
    const saved = fromOnboarding ? undefined : readHomeSnapshot(cacheKey)
    return saved && hasHomeContent(saved) ? saved : undefined
  })
  const [view, setView] = useState<HomeSnapshot>(initial ?? emptyHomeSnapshot)
  const [pending, setPending] = useState(() => pendingSections(!initial))
  const [refreshPending, setRefreshPending] = useState(!isChild || canDiscover)
  const [error, setError] = useState<string | null>(null)
  const manualRails = useRef<HomeSnapshot['rails'] | null>(null)

  useEffect(() => {
    let cancelled = false
    const generation = homeSnapshotGeneration()
    let merged = initial ?? emptyHomeSnapshot()
    let remaining = ALL_SECTIONS.length
    let completedAt = 0
    let refreshedSpotlight: Pick<HomeSnapshot, 'spotlight' | 'recommendations'> | undefined
    const validSession = () => generation === homeSnapshotGeneration()
    const withManualChanges = (snapshot: HomeSnapshot) => manualRails.current === null
      ? snapshot : { ...snapshot, rails: manualRails.current }
    const save = () => {
      if (remaining === 0 && validSession()) {
        saveHomeSnapshot(cacheKey, withManualChanges({ ...merged, ...refreshedSpotlight, loadedAt: completedAt }), generation)
      }
    }
    const load = async (section: HomeSection, request: Promise<Partial<HomeSnapshot>>) => {
      try {
        const patch = await request
        if (!validSession()) return
        merged = { ...merged, ...patch }
      } catch (caught) {
        if (validSession() && !initial && !cancelled && section === 'recent') {
          setError(caught instanceof ApiError ? caught.message : 'Could not load your library')
        }
      } finally {
        remaining -= 1
        if (remaining === 0) completedAt = Date.now()
        if (!cancelled && validSession() && !initial) {
          setView(withManualChanges(merged))
          setPending((current) => ({ ...current, [section]: false }))
        }
        save()
      }
    }
    const spotlightPatch = (data: Awaited<ReturnType<typeof fetchSpotlight>>) => ({
      spotlight: data.items.filter((item) =>
        (!isChild || item.source === 'shelf' || (canDiscover && item.source === 'discover')) &&
        item.blurb && heroBlurb(item.blurb) !== null).slice(0, 5),
      recommendations: isChild && !canDiscover ? [] : data.recommendations ?? [],
    })

    // The first request is entirely local/cache work. Catalogue refreshes are
    // saved for the next visit and never replace visible recommendations.
    const cachedSpotlight = fetchSpotlight(true).then(spotlightPatch)
    void load('spotlight', cachedSpotlight)
    void cachedSpotlight.catch(() => undefined).then(async () => {
      if (!validSession() || (isChild && !canDiscover)) return
      try {
        refreshedSpotlight = spotlightPatch(await fetchSpotlight())
        save()
        // A new profile with no books or saved suggestions has nothing to
        // reshuffle. Let its first catalogue suggestions fill the empty Home.
        if (!cancelled && validSession() && !initial && !hasHomeContent(merged)) {
          merged = { ...merged, ...refreshedSpotlight }
          setView(withManualChanges(merged))
        }
      } catch {
        // Keep the current selection if a catalogue is unavailable.
      } finally {
        if (!cancelled && validSession()) setRefreshPending(false)
      }
    })

    void load('recent', fetchRecent(12).then((recent) => ({ recent })))
    void load('highlights', fetchHighlights(12).then((highlights) => ({ highlights })))
    void load('continueReading', fetchContinueReading().then((continueReading) => ({ continueReading })))
    void load('rails', fetchHomeRails().then((rails) => ({ rails })))
    void load('authors', isChild ? Promise.resolve({ authors: [] }) : fetchAuthors().then((authors) => ({ authors })))
    void load('householdBooks', isChild ? Promise.resolve({ householdBooks: 0 }) :
      fetchBooks('recent', 1, 1, { mine: false }).then((page) => ({ householdBooks: page.total })))
    void load('updates', isChild ? Promise.resolve({ updates: null }) : fetchUpdates().then((updates) => ({ updates })))
    void load('shelves', isChild ? Promise.resolve({ shelves: [] }) : fetchCollections().then(async (collections) => ({
      shelves: (await Promise.allSettled(collections.filter((collection) => collection.bookCount > 0)
        .slice(0, 4).map((collection) => fetchCollection(collection.id))))
        .flatMap((result) => result.status === 'fulfilled' ? [result.value] : []),
    })))

    return () => { cancelled = true }
  }, [cacheKey, isChild, canDiscover, fromOnboarding, initial])

  const store = (next: HomeSnapshot) => {
    // Hiding/restoring a subject is an explicit change on this visit. A slower
    // collection or recommendation response must not undo it.
    manualRails.current = next.rails
    saveHomeSnapshot(cacheKey, next)
    return next
  }
  const visible = (section: typeof HOME_SECTIONS[number]) =>
    HOME_SECTIONS.slice(0, HOME_SECTIONS.indexOf(section) + 1).every((key) => !pending[key])
  const nextSection = HOME_SECTIONS.find((section) => pending[section])
  return { view, setView, pending, refreshPending, error, setError, store, visible, nextSection }
}

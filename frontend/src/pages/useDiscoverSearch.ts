import { type FormEvent, useEffect, useRef, useState } from 'react'
import { useSearchParams } from 'react-router-dom'
import { ApiError } from '../api/client'
import {
  type DiscoverPage,
  type DiscoveryResult,
  type SearchType,
  fetchDiscoverOrchestration,
} from '../api/discover'
import type { AuthorHit } from '../api/library'

type SearchState = {
  key: string
  items: DiscoveryResult[]
  error: string | null
}

// Results, author suggestions and the continuation token are cached together
// for the session, so restoring a search renders books and authors in the
// same commit instead of inserting one after the other.
type SearchCacheEntry = {
  items: DiscoveryResult[]
  error: string | null
  next: string | null
  authors: AuthorHit[]
  storedAt: number
}

const SEARCH_CACHE_CAP = 50
const SEARCH_CACHE_TTL_MS = 5 * 60_000
const searchCache = new Map<string, SearchCacheEntry>()

function cachedSearch(key: string): SearchCacheEntry | undefined {
  const entry = searchCache.get(key)
  if (entry && Date.now() - entry.storedAt >= SEARCH_CACHE_TTL_MS) {
    searchCache.delete(key)
    return undefined
  }
  return entry
}

function cacheSearch(key: string, entry: Omit<SearchCacheEntry, 'storedAt'>) {
  if (!searchCache.has(key) && searchCache.size >= SEARCH_CACHE_CAP) {
    const oldest = searchCache.keys().next().value
    if (oldest !== undefined) {
      searchCache.delete(oldest)
    }
  }
  searchCache.set(key, { ...entry, storedAt: Date.now() })
}

/// Patches one provider book across every cached search page, so a mutation
/// cannot leave the cache contradicting what the user just did.
function patchCachedResult(userId: number, provider: string, providerKey: string, patch: Partial<DiscoveryResult>) {
  for (const [key, entry] of searchCache) {
    if (!key.startsWith(`${userId}|`)) {
      continue
    }
    const index = entry.items.findIndex((item) => item.provider === provider && item.providerKey === providerKey)
    if (index < 0) {
      continue
    }
    const items = entry.items.slice()
    items[index] = { ...items[index], ...patch }
    searchCache.set(key, { ...entry, items })
  }
}

export function useDiscoverSearch({
  userId,
  defaultLanguages,
  demo,
  detailOnly,
  setNotice,
}: {
  userId: number
  defaultLanguages: string[]
  demo: boolean | null
  detailOnly: boolean
  setNotice: (message: string | null) => void
}) {
  const [searchParams, setSearchParams] = useSearchParams()
  const [type, setType] = useState<SearchType>(() => {
    const value = searchParams.get('type')
    return value === 'title' || value === 'author' || value === 'isbn' || value === 'subject'
      ? value
      : 'any'
  })
  const [query, setQuery] = useState(() => searchParams.get('q') ?? '')
  // Live input is authoritative: debounced searches and URL restores must
  // never write an older query back into the field.
  const inputTouched = useRef(false)
  const [requestedKey, setRequestedKey] = useState<string | null>(null)
  const [completedKey, setCompletedKey] = useState<string | null>(null)
  const [result, setResult] = useState<SearchState | null>(null)
  const [continuation, setContinuation] = useState<string | null>(null)
  const [authorState, setAuthorState] = useState<{
    key: string
    items: AuthorHit[]
    partial: boolean
  } | null>(null)
  const requestSeq = useRef(0)
  const [loadingMore, setLoadingMore] = useState(false)
  const loadingMoreRef = useRef(false)
  const [loadMoreError, setLoadMoreError] = useState<string | null>(null)
  const [externalNotice, setExternalNotice] = useState<string | null>(null)
  const [allLanguages, setAllLanguages] = useState(() => {
    try {
      return sessionStorage.getItem('bokhylle.discover.allLanguages') === '1'
    } catch {
      return false
    }
  })

  const loading = requestedKey !== null && completedKey !== requestedKey
  // Cached results carry personal annotations (onShelf, following) and were
  // backfilled for a language mode, so the key scopes both.
  const languageMode = allLanguages ? 'all' : defaultLanguages.join(',')
  const searchKeyOf = (kind: SearchType, text: string) =>
    `${userId}|${kind}|${text.trim()}|${languageMode}`

  function setInput(value: string) {
    inputTouched.current = true
    setQuery(value)
  }

  function patchSearchResult(provider: string, providerKey: string, patch: Partial<DiscoveryResult>) {
    patchCachedResult(userId, provider, providerKey, patch)
    setResult((current) =>
      current
        ? {
            ...current,
            items: current.items.map((item) =>
              item.provider === provider && item.providerKey === providerKey ? { ...item, ...patch } : item,
            ),
          }
        : current,
    )
  }

  async function runSearch(text: string, kind: SearchType) {
    const trimmed = text.trim()
    if (!trimmed) {
      return
    }

    setType(kind)
    setSearchParams({ q: trimmed, type: kind }, { replace: true })

    try {
      sessionStorage.setItem(`bokhylle.discover.last.${userId}`, JSON.stringify({ q: trimmed, type: kind }))
    } catch {
      // Session persistence is best-effort.
    }

    const key = searchKeyOf(kind, trimmed)
    const cached = demo ? undefined : cachedSearch(key)
    if (result?.key === key && requestedKey === key && cached) {
      return
    }
    setRequestedKey(key)
    setLoadMoreError(null)
    setNotice(null)
    setExternalNotice(null)

    // Invalidate older requests even when this search can be served from cache.
    const generation = ++requestSeq.current
    if (cached) {
      setContinuation(cached.next)
      setResult({ key, items: cached.items, error: cached.error })
      setAuthorState({ key, items: cached.authors, partial: false })
      setCompletedKey(key)
      return
    }

    const wantsAuthors = kind === 'any' || kind === 'author'
    const authorsOf = (page: DiscoverPage) => [...page.authors.local, ...page.authors.external]
    if (demo) {
      try {
        const page = await fetchDiscoverOrchestration({ q: trimmed, type: kind, localOnly: true })
        if (generation !== requestSeq.current) return
        setResult({ key, items: page.books, error: null })
        setAuthorState({ key, items: wantsAuthors ? authorsOf(page) : [], partial: false })
        setContinuation(null)
        setCompletedKey(key)
      } catch (caught) {
        if (generation !== requestSeq.current) return
        setResult({ key, items: [], error: caught instanceof ApiError ? caught.message : 'Search failed' })
        setAuthorState({ key, items: [], partial: false })
        setCompletedKey(key)
      }
      return
    }
    let fullLanded = false
    let localSnapshot: { items: DiscoveryResult[]; authors: AuthorHit[] } | null = null

    // Local fast path: catalogue books and authors paint without provider
    // latency; the full call replaces both when it lands. Nothing is cached
    // until the full answer settles, so a revisit cannot pin a partial view.
    void fetchDiscoverOrchestration({ q: trimmed, type: kind, localOnly: true })
      .then((page) => {
        if (generation !== requestSeq.current || fullLanded) {
          return
        }
        const authors = wantsAuthors ? authorsOf(page) : []
        localSnapshot = { items: page.books, authors }
        setContinuation(null)
        setResult({ key, items: page.books, error: null })
        if (wantsAuthors) {
          setAuthorState({ key, items: authors, partial: true })
        }
      })
      .catch((caught: unknown) => console.warn('discover.local_fast_path_failed', caught))

    try {
      const page = await fetchDiscoverOrchestration({ q: trimmed, type: kind })
      let items = page.books
      let next = page.next
      const preferred = defaultLanguages
      const languageActive = !allLanguages && preferred.length > 0
      // Preferred-language filtering is client-side, so backfill provider
      // pages instead of showing a sparse page and asking for Load more.
      if (languageActive) {
        let pages = 0
        while (
          next &&
          pages < 3 &&
          items.filter((item) =>
            item.languages?.some((language) => preferred.includes(language)) ||
            (item.language !== null && preferred.includes(item.language)),
          )
            .length < 18
        ) {
          const more = await fetchDiscoverOrchestration({
            q: trimmed,
            type: kind,
            continuation: next,
          })
          items = [...items, ...more.books]
          next = more.next
          pages += 1
        }
      }
      if (generation !== requestSeq.current) {
        return
      }
      fullLanded = true
      setExternalNotice(null)
      const authors = wantsAuthors ? authorsOf(page) : []
      setContinuation(next)
      cacheSearch(key, { items, error: null, next, authors })
      setResult({ key, items, error: null })
      setAuthorState({ key, items: authors, partial: false })
      setCompletedKey(key)
    } catch (caught) {
      if (generation !== requestSeq.current) {
        return
      }
      fullLanded = true
      // The local snapshot is still useful knowledge: keep it and say the
      // online half failed instead of blanking the page.
      const snapshot = localSnapshot as {
        items: DiscoveryResult[]
        authors: AuthorHit[]
      } | null
      if (snapshot) {
        setResult({ key, items: snapshot.items, error: null })
        if (wantsAuthors) {
          setAuthorState({ key, items: snapshot.authors, partial: true })
        }
        setExternalNotice('Showing your catalogue — online results unavailable.')
        setCompletedKey(key)
        return
      }
      const message = caught instanceof ApiError ? caught.message : 'Search failed'
      setResult({ key, items: [], error: message })
      setAuthorState({ key, items: [], partial: false })
      setCompletedKey(key)
    }
  }

  async function loadMore() {
    if (!continuation || loadingMoreRef.current || !result || result.key !== requestedKey) {
      return
    }
    const generation = requestSeq.current
    loadingMoreRef.current = true
    setLoadingMore(true)
    setLoadMoreError(null)
    try {
      const page = await fetchDiscoverOrchestration({
        q: query.trim(),
        type,
        continuation,
      })
      if (generation !== requestSeq.current) {
        return
      }
      const items = [...result.items, ...page.books]
      setResult((current) =>
        current && current.key === result.key ? { ...current, items } : current,
      )
      setContinuation(page.next)
      const cached = cachedSearch(result.key)
      if (cached) {
        cacheSearch(result.key, { ...cached, items, next: page.next })
      }
    } catch {
      if (generation === requestSeq.current) setLoadMoreError('Could not load more results. Your current books are still shown.')
    } finally {
      loadingMoreRef.current = false
      setLoadingMore(false)
    }
  }

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    await runSearch(query, type)
  }

  function retrySearch() {
    searchCache.delete(searchKeyOf(type, query.trim()))
    void runSearch(query, type)
  }

  // Restore a search from the URL (links, back button) or, when the URL has
  // none, the last search of this session so the nav link is not a reset.
  useEffect(() => {
    if (detailOnly) return
    if (inputTouched.current) {
      return
    }
    const initial = searchParams.get('q')
    if (initial && requestedKey === null && result === null) {
      void runSearch(initial, type)
      return
    }
    if (!initial && requestedKey === null && result === null) {
      try {
        const stored = JSON.parse(sessionStorage.getItem(`bokhylle.discover.last.${userId}`) ?? 'null')
        if (stored?.q) {
          setQuery(stored.q)
          setType(stored.type ?? 'any')
          void runSearch(stored.q, stored.type ?? 'any')
        }
      } catch {
        // Ignore malformed session state.
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // Search as you type: debounce keystrokes, render cache hits instantly and
  // keep the previous results while a new query is in flight.
  useEffect(() => {
    const trimmed = query.trim()
    if (trimmed.length < 3) {
      return
    }
    const key = searchKeyOf(type, trimmed)
    const cached = cachedSearch(key)
    if (cached) {
      ++requestSeq.current
      setRequestedKey(key)
      setNotice(null)
      setContinuation(cached.next)
      setResult({ key, items: cached.items, error: cached.error })
      setAuthorState({ key, items: cached.authors, partial: false })
      setCompletedKey(key)
      return
    }
    const timer = setTimeout(() => {
      void runSearch(trimmed, type)
    }, 250)
    return () => clearTimeout(timer)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, type, languageMode])

  function updateAuthors(update: (items: AuthorHit[]) => AuthorHit[]) {
    setAuthorState((current) => {
      if (!current) {
        return current
      }
      const items = update(current.items)
      const cached = cachedSearch(current.key)
      if (cached) {
        cacheSearch(current.key, { ...cached, authors: items })
      }
      return { ...current, items }
    })
  }

  function updateAllLanguages(value: boolean) {
    setAllLanguages(value)
    try {
      if (value) {
        sessionStorage.setItem('bokhylle.discover.allLanguages', '1')
      } else {
        sessionStorage.removeItem('bokhylle.discover.allLanguages')
      }
    } catch {
      // Session persistence is best-effort.
    }
  }

  return {
    searchParams,
    type,
    setType,
    query,
    setInput,
    requestedKey,
    result,
    authorState,
    continuation,
    loadingMore,
    loadMoreError,
    retrySearch,
    loading,
    allLanguages,
    updateAllLanguages,
    externalNotice,
    setExternalNotice,
    patchSearchResult,
    runSearch,
    loadMore,
    handleSubmit,
    updateAuthors,
  }
}

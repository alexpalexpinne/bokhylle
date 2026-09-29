import { useEffect, useRef, useState } from 'react'
import { Link, useLocation, useSearchParams } from 'react-router-dom'
import { ArrowRight, SlidersHorizontal, X } from 'lucide-react'
import { ApiError } from '../api/client'
import {
  type AuthorSummary,
  type BookFacets,
  type BookSort,
  type BookSummary,
  fetchAuthors,
  claimShelf,
  fetchBookFacets,
  fetchBooks,
  searchBooks,
  setAuthorFollow,
} from '../api/library'
import { type CollectionSummary, fetchCollections } from '../api/collections'
import { type HouseholdMember, fetchHouseholdMembers } from '../api/users'
import { AuthorAvatar } from '../components/AuthorAvatar'
import { LetterIndex } from '../components/ui/LetterIndex'
import { SearchField } from '../components/ui/SearchField'
import { ScopeTabs } from '../components/ui/ScopeTabs'
import { SegmentedControl } from '../components/ui/SegmentedControl'
import { BookGrid } from '../components/BookGrid'
import { ComicShelf } from '../components/ComicShelf'
import { Button, ButtonLink } from '../components/ui/Button'
import { FacetPicker } from '../components/ui/FacetPicker'
import { useAuth } from '../auth/useAuth'
import { useMutation } from '../lib/useMutation'
import { PageHeader } from '../components/ui/PageHeader'

type Mode = 'books' | 'authors'
type Category = 'all' | 'books' | 'comics'

type BooksResult = {
  key: string
  scope: string
  items: BookSummary[]
  total: number
  letters: string[]
  pageSize: number
  error: string | null
}

type AuthorsResult = {
  key: 'authors'
  items: AuthorSummary[]
  error: string | null
}

const sortOptions: { value: BookSort; label: string }[] = [
  { value: 'recent', label: 'Recently added' },
  { value: 'title', label: 'Title' },
  { value: 'author', label: 'Author' },
]

function DelayedLoadingStatus({ message }: { message: string }) {
  const [visible, setVisible] = useState(false)

  useEffect(() => {
    const timer = window.setTimeout(() => setVisible(true), 200)
    return () => window.clearTimeout(timer)
  }, [])

  return visible ? <p role="status" className="mb-5 text-sm text-ink-muted">{message}</p> : null
}

export function Library() {
  const { user, demo } = useAuth()
  const location = useLocation()
  const isChild = user?.profileType === 'child'

  const [searchParams, setSearchParams] = useSearchParams()
  const followingOnly = searchParams.get('following') === '1'
  // Initialise from the URL so deep links are not stripped by the first
  // URL-sync pass before the restore effect runs.
  const [mode, setMode] = useState<Mode>(() =>
    searchParams.get('mode') === 'authors' ? 'authors' : 'books',
  )
  const [category, setCategory] = useState<Category>(() => {
    const value = searchParams.get('category')
    return value === 'books' || value === 'comics' ? value : 'all'
  })
  const authorQuery = searchParams.get('authors_q') ?? ''
  const authorLetter = searchParams.get('authors_letter') ?? ''
  const [query, setQuery] = useState(() => searchParams.get('q') ?? '')
  const [sort, setSort] = useState<BookSort>(() => {
    const value = searchParams.get('sort')
    return value === 'title' || value === 'author' ? value : 'recent'
  })
  const [page, setPage] = useState(() => {
    const value = Number(searchParams.get('page') ?? '1')
    return Number.isFinite(value) && value > 0 ? value : 1
  })

  const [booksResult, setBooksResult] = useState<BooksResult | null>(null)
  const [authorsResult, setAuthorsResult] = useState<AuthorsResult | null>(null)
  const [followError, setFollowError] = useState<string | null>(null)
  const claimMutation = useMutation()
  const [facets, setFacets] = useState<BookFacets | null>(null)
  const [collections, setCollections] = useState<CollectionSummary[]>([])
  const [mine, setMine] = useState(() => searchParams.get('scope') !== 'household')
  // An assigned child id when an administrator browses that shelf.
  const [member, setMember] = useState<number | null>(null)
  const [members, setMembers] = useState<HouseholdMember[]>([])
  const [reload, setReload] = useState(0)
  const [format, setFormat] = useState(() => searchParams.get('format') ?? '')
  const [language, setLanguage] = useState(() => searchParams.get('language') ?? '')
  const [series, setSeries] = useState(() => searchParams.get('series') ?? '')
  const [subject, setSubject] = useState(() => searchParams.get('subject') ?? '')
  const [collection, setCollection] = useState(() => searchParams.get('collection') ?? '')
  const [letter, setLetter] = useState(() => searchParams.get('letter') ?? '')
  const [missing, setMissing] = useState(() => searchParams.get('missing') ?? '')
  const [appended, setAppended] = useState<BookSummary[]>([])
  const [nextPage, setNextPage] = useState(2)
  const [loadingMore, setLoadingMore] = useState(false)
  const sentinelRef = useRef<HTMLDivElement | null>(null)
  const writtenQueryRef = useRef('')

  const trimmedQuery = query.trim()
  const memberName = member
    ? (members.find((entry) => entry.id === member)?.displayName ?? 'Member')
    : null
  const booksKey = `${trimmedQuery}|${sort}|${page}|${mine}|${member}|${reload}|${category}|${format}|${language}|${series}|${subject}|${collection}|${letter}|${missing}`
  const scopeKey = `${mine}|${member}`

  useEffect(() => {
    if (mode !== 'books') {
      return
    }

    let cancelled = false

    const requestKey = `${trimmedQuery}|${sort}|${page}|${mine}|${member}|${reload}|${category}|${format}|${language}|${series}|${subject}|${collection}|${letter}|${missing}`
    const kind = category === 'all' ? undefined : category === 'books' ? 'books' : 'comics'
    const timer = setTimeout(
      () => {
        const request = trimmedQuery
          ? searchBooks(trimmedQuery, {
              mine,
              user: member ?? undefined,
              kind,
              format,
              language,
              series,
              subject,
              collection: collection ? Number(collection) : undefined,
              missing: missing || undefined,
            }).then((items) => ({
              items,
              total: items.length,
              letters: [] as string[],
              pageSize: items.length,
            }))
          : fetchBooks(sort, page, 24, {
              mine,
              user: member ?? undefined,
              kind,
              format,
              language,
              series,
              subject: subject || undefined,
              collection: collection ? Number(collection) : undefined,
              letter: letter || undefined,
              missing: missing || undefined,
            })

        request
          .then((result) => {
            if (!cancelled) {
              setBooksResult({
                key: requestKey,
                scope: `${mine}|${member}`,
                items: result.items,
                total: result.total,
                letters: result.letters ?? [],
                pageSize: result.pageSize,
                error: null,
              })
            }
          })
          .catch((caught: unknown) => {
            if (!cancelled) {
              setBooksResult({
                key: requestKey,
                scope: `${mine}|${member}`,
                items: [],
                total: 0,
                letters: [],
                pageSize: 24,
                error: caught instanceof ApiError ? caught.message : 'Failed to load library',
              })
            }
          })
      },
      trimmedQuery ? 125 : 0,
    )

    return () => {
      cancelled = true
      clearTimeout(timer)
    }
  }, [mode, trimmedQuery, sort, page, mine, member, reload, category, format, language, series, subject, collection, letter, missing])

  useEffect(() => {
    setAppended([])
    setNextPage(2)
  }, [booksKey])

  // Keep the URL in sync with the current view so back/forward and shared
  // links restore search, sort, filters and page.
  useEffect(() => {
    const next = new URLSearchParams()
    if (query) next.set('q', query)
    if (member) next.set('scope', `user-${member}`)
    else if (!mine) next.set('scope', 'household')
    if (sort !== 'recent') next.set('sort', sort)
    if (page > 1) next.set('page', String(page))
    if (format) next.set('format', format)
    if (language) next.set('language', language)
    if (series) next.set('series', series)
    if (subject) next.set('subject', subject)
    if (collection) next.set('collection', collection)
    if (letter) next.set('letter', letter)
    if (missing) next.set('missing', missing)
    if (mode === 'authors') next.set('mode', 'authors')
    if (category !== 'all') next.set('category', category)
    if (mode === 'authors' && followingOnly) next.set('following', '1')

    if (next.toString() !== searchParams.toString()) {
      writtenQueryRef.current = query
      setSearchParams(next, { replace: true })
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, sort, page, mine, member, category, format, language, series, subject, collection, letter, missing, mode, followingOnly])

  useEffect(() => {
    // Ignore the echo of our own URL write so a late navigation can never
    // roll the input back; real back/forward still restores the query.
    const urlQuery = searchParams.get('q') ?? ''
    if (urlQuery !== writtenQueryRef.current) {
      setQuery(urlQuery)
    }
    const scopeParam = searchParams.get('scope')
    const memberMatch = scopeParam ? /^user-(\d+)$/.exec(scopeParam) : null
    const mayBrowseChild = user?.role === 'admin' || (demo === true && !isChild)
    setMember(memberMatch && mayBrowseChild ? Number(memberMatch[1]) : null)
    setMine(scopeParam !== 'household' && !(memberMatch && mayBrowseChild))
    const sortParam = searchParams.get('sort')
    setSort(sortParam === 'title' || sortParam === 'author' ? sortParam : 'recent')
    const pageParam = Number(searchParams.get('page') ?? '1')
    setPage(Number.isFinite(pageParam) && pageParam > 0 ? pageParam : 1)
    setFormat(searchParams.get('format') ?? '')
    setLanguage(searchParams.get('language') ?? '')
    setSeries(searchParams.get('series') ?? '')
    setSubject(searchParams.get('subject') ?? '')
    setCollection(searchParams.get('collection') ?? '')
    setLetter(searchParams.get('letter') ?? '')
    setMissing(searchParams.get('missing') ?? '')
    setMode(searchParams.get('mode') === 'authors' && !isChild ? 'authors' : 'books')
    const categoryParam = searchParams.get('category')
    setCategory(categoryParam === 'books' || categoryParam === 'comics' ? categoryParam : 'all')
  }, [searchParams, isChild, user?.role, demo])

  useEffect(() => {
    if (user?.role !== 'admin' && !demo) {
      setMembers([])
      return
    }

    let cancelled = false
    fetchHouseholdMembers()
      .then((value) => {
        if (!cancelled) {
          setMembers(value.members.filter((entry) => entry.id !== user?.id))
        }
      })
      .catch((caught: unknown) => console.warn('library.read_failed', caught))

    return () => {
      cancelled = true
    }
  }, [user?.role, user?.id, demo])

  useEffect(() => {
    if (mode !== 'books') {
      return
    }

    let cancelled = false
    if (!isChild) fetchBookFacets(mine, member ?? undefined)
      .then((value) => {
        if (!cancelled) {
          setFacets(value)
        }
      })
      .catch((caught: unknown) => console.warn('library.read_failed', caught))
    if (!isChild) fetchCollections()
      .then((value) => {
        if (!cancelled) {
          setCollections(value)
        }
      })
      .catch((caught: unknown) => console.warn('library.read_failed', caught))

    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode, mine, member, followingOnly, isChild])

  useEffect(() => {
    if (mode !== 'authors' || isChild) {
      return
    }

    let cancelled = false

    fetchAuthors(mine, followingOnly, member ?? undefined)
      .then((items) => {
        if (!cancelled) {
          setAuthorsResult({ key: 'authors', items, error: null })
        }
      })
      .catch((caught: unknown) => {
        if (!cancelled) {
          setAuthorsResult({
            key: 'authors',
            items: [],
            error: caught instanceof ApiError ? caught.message : 'Failed to load authors',
          })
        }
      })

    return () => {
      cancelled = true
    }
  }, [mode, mine, followingOnly, isChild, member])

  const booksLoading = mode === 'books' && booksResult?.key !== booksKey
  const baseBooks = booksResult?.key === booksKey ? booksResult.items : []
  const books = appended.length > 0 ? [...baseBooks, ...appended] : baseBooks
  const displayedBooks = booksLoading
    ? booksResult?.scope === scopeKey ? booksResult.items : []
    : books
  const total = booksResult?.key === booksKey ? booksResult.total : 0
  const booksError = booksResult?.key === booksKey ? booksResult.error : null
  const booksLoaded = booksResult?.key === booksKey
  const hasMore = !trimmedQuery && booksLoaded && books.length < total

  function setAuthorState(authorId: number, update: Partial<AuthorSummary>) {
    setAuthorsResult((current) =>
      current
        ? {
            ...current,
            items: current.items.map((item) =>
              item.id === authorId ? { ...item, ...update } : item,
            ),
          }
        : current,
    )
  }

  async function toggleAuthorFollow(author: AuthorSummary) {
    const next = !author.following
    setAuthorState(author.id, { following: next, autoAcquire: next ? author.autoAcquire : false })
    try {
      await setAuthorFollow(author.id, next)
      if (followingOnly && !next) {
        setAuthorsResult((current) =>
          current
            ? { ...current, items: current.items.filter((item) => item.id !== author.id) }
            : current,
        )
      }
    } catch {
      setAuthorState(author.id, {
        following: author.following,
        autoAcquire: author.autoAcquire,
      })
      setFollowError('Could not update that follow. Try again.')
    }
  }

  async function loadMore() {
    if (loadingMore || !hasMore) {
      return
    }
    const requestKey = booksKey
    setLoadingMore(true)
    try {
      const result = await fetchBooks(sort, nextPage, 24, {
        mine,
        user: member ?? undefined,
        kind: category === 'all' ? undefined : category === 'books' ? 'books' : 'comics',
        format,
        language,
        series,
        subject: subject || undefined,
        collection: collection ? Number(collection) : undefined,
        letter: letter || undefined,
        missing: missing || undefined,
      })
      setAppended((current) => (booksKey === requestKey ? [...current, ...result.items] : current))
      if (booksKey === requestKey) {
        setNextPage((current) => current + 1)
      }
    } catch {
      // Load more is best-effort; the button remains for a retry.
    } finally {
      setLoadingMore(false)
    }
  }

  useEffect(() => {
    const node = sentinelRef.current
    if (!node || !hasMore) {
      return
    }
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          void loadMore()
        }
      },
      { rootMargin: '600px' },
    )
    observer.observe(node)
    return () => observer.disconnect()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hasMore, loadingMore, appended.length, sort, mine, category, format, language, series, subject, collection, letter, missing])
  const filtersActive = Boolean(format || language || series || subject || collection || missing)
  const groupedComics = mode === 'books' && category === 'comics' && !trimmedQuery && !filtersActive && !letter
  const hasComics = facets?.publicationKinds?.some((item) => (item.value === 'comic' || item.value === 'manga') && item.count > 0) ?? false
  const hasBooks = facets?.publicationKinds?.some((item) => (item.value === 'book' || item.value === 'unknown') && item.count > 0) ?? false
  const isEmptyShelf = booksLoaded && !trimmedQuery && !filtersActive && total === 0

  const authorsLoading = mode === 'authors' && authorsResult === null
  const authors = authorsResult?.items ?? []
  const bookLetters = new Set(
    (booksResult?.key === booksKey ? booksResult.letters : []) ?? [],
  )
  const authorLetters = new Set(
    authors.map((author) => author.name.slice(0, 1).toLowerCase()),
  )
  const letterForBook = (book: BookSummary) =>
    (sort === 'author' ? (book.authors[0] ?? book.title) : book.title).slice(0, 1).toLowerCase()
  const filteredAuthors = authors.filter((author) => {
    const name = author.name.toLowerCase()
    if (authorQuery && !name.includes(authorQuery.toLowerCase())) {
      return false
    }
    if (authorLetter && !name.startsWith(authorLetter)) {
      return false
    }
    return true
  })
  const authorsError = authorsResult?.error ?? null

  const collectionName = (value: string) =>
    collections.find((item) => String(item.id) === value)?.name ?? value

  const missingLabels: Record<string, string> = {
    cover: 'Missing covers',
    description: 'Missing descriptions',
    language: 'Missing language',
  }

  const activeFilters = [
    format ? { key: 'format', label: format.toUpperCase(), clear: () => { setFormat(''); setPage(1) } } : null,
    language
      ? {
          key: 'language',
          label: language === 'und' ? 'Unknown' : language.toUpperCase(),
          clear: () => { setLanguage(''); setPage(1) },
        }
      : null,
    series ? { key: 'series', label: series, clear: () => { setSeries(''); setPage(1) } } : null,
    subject
      ? {
          key: 'subject',
          label:
            facets?.subjects.find((facet) => facet.normalized === subject)?.name ?? subject,
          clear: () => { setSubject(''); setPage(1) },
        }
      : null,
    collection
      ? {
          key: 'collection',
          label: collectionName(collection),
          clear: () => { setCollection(''); setPage(1) },
        }
      : null,
    missing
      ? {
          key: 'missing',
          label: missingLabels[missing] ?? 'Missing metadata',
          clear: () => { setMissing(''); setPage(1) },
        }
      : null,
  ].filter((filter): filter is NonNullable<typeof filter> => filter !== null)

  const activeFilterCount = activeFilters.length

  function clearAllFilters() {
    setFormat('')
    setLanguage('')
    setSeries('')
    setSubject('')
    setCollection('')
    setMissing('')
    setPage(1)
  }

  function closeFilters(event: { currentTarget: HTMLElement }) {
    const details = event.currentTarget.closest('details')
    if (details instanceof HTMLDetailsElement) {
      details.open = false
    }
  }

  const description =
    mode === 'authors'
      ? memberName
        ? `Authors on ${memberName}'s shelf.`
        : mine
          ? 'Authors on your shelf.'
          : 'Authors in the household library.'
      : trimmedQuery
        ? `Results for "${trimmedQuery}"`
        : category === 'comics'
          ? 'Comics and manga, arranged by series.'
        : booksLoaded && total > 0
          ? `${total} ${total === 1 ? 'book' : 'books'} ${
              memberName
                ? `on ${memberName}'s shelf`
                : mine
                  ? 'on your shelf'
                  : 'in the household library'
            }`
          : isChild
            ? 'Books an adult adds to your shelf will appear here.'
            : memberName
              ? `Everything ${memberName} has collected.`
              : mine
                ? 'Everything you have collected, ready to open or send to your reader.'
                : 'Every book in the household library.'

  return (
    <section>
      <PageHeader
        eyebrow="Library"
        title={
          mode === 'authors'
            ? 'Authors'
            : memberName
              ? `${memberName}'s shelf`
              : mine
                ? 'Your shelf'
                : 'Household library'
        }
        description={description}
        actions={!isChild ? <>
          {user?.role === 'admin' && <Link to="/library/review" className="text-xs font-medium text-accent hover:text-accent-strong">Review imports</Link>}
          <Link
            to={mode === 'authors' ? '/library' : '/library?mode=authors'}
            className="text-xs font-medium text-accent hover:text-accent-strong"
          >{mode === 'authors' ? 'Browse library' : 'Browse authors'}</Link>
        </> : undefined}
      />

      {mode === 'books' && (booksLoaded && total > 0 || hasBooks || hasComics) && <div className="mt-6">
        <ScopeTabs
          ariaLabel="Publication type"
          value={category}
          onChange={(value) => {
            setCategory(value)
            setPage(1)
            setLetter('')
            setFormat('')
            setSeries('')
            if (value === 'comics' && sort === 'author') setSort('recent')
          }}
          options={[
            { value: 'all', label: 'All' },
            ...(hasBooks || category === 'books' ? [{ value: 'books' as const, label: 'Books' }] : []),
            ...(hasComics || category === 'comics' ? [{ value: 'comics' as const, label: 'Comics & Manga' }] : []),
          ]}
        />
      </div>}

      {mode === 'authors' && followError && (
        <p className="mt-3 text-sm text-danger">{followError}</p>
      )}

      {mode === 'authors' && (
        <div className="mt-6 flex flex-wrap items-center gap-2">
          <SegmentedControl
            ariaLabel="Author view"
            value={followingOnly ? 'following' : 'all'}
            onChange={(value) => {
              const next = new URLSearchParams(searchParams)
              next.set('mode', 'authors')
              if (value === 'following') {
                next.set('following', '1')
              } else {
                next.delete('following')
              }
              setSearchParams(next, { replace: true })
            }}
            options={[
              { value: 'all', label: 'All authors' },
              { value: 'following', label: 'Following' },
            ]}
          />
        </div>
      )}

      <div className="sticky top-14 z-30 -mx-4 mt-6 bg-canvas/90 px-4 py-3 backdrop-blur sm:-mx-6 sm:px-6 lg:-mx-8 lg:px-8">
        {mode === 'books' && (
          <div className="w-full sm:max-w-md">
            <SearchField
              value={query}
              onChange={(value) => {
                setQuery(value)
                setPage(1)
              }}
              placeholder={mine ? 'Search your shelf' : 'Search the household library'}
              ariaLabel={mine ? 'Search your shelf' : 'Search the household library'}
              action={query ? (
                <button type="button" onClick={() => { setQuery(''); setPage(1) }} aria-label="Clear search" className="text-ink-faint hover:text-accent">
                  <X size={14} aria-hidden />
                </button>
              ) : undefined}
            />
          </div>
        )}
        {mode === 'authors' && (
          <div className="w-full sm:max-w-md">
            <SearchField
              value={authorQuery}
              onChange={(value) => {
                const next = new URLSearchParams(searchParams)
                next.set('mode', 'authors')
                if (value) {
                  next.set('authors_q', value)
                  next.delete('authors_letter')
                } else {
                  next.delete('authors_q')
                }
                setSearchParams(next, { replace: true })
              }}
              placeholder="Search your authors…"
              ariaLabel="Search authors"
            />
          </div>
        )}

        {((mode === 'books' && booksLoaded) || (mode === 'authors' && !followingOnly)) &&
          !trimmedQuery &&
          !isEmptyShelf && (
          <div className="mt-3 flex flex-wrap items-center gap-2">
            {!isChild && (
            <div className="pr-2">
              <ScopeTabs
                ariaLabel="Scope"
                value={member ? `user-${member}` : mine ? 'mine' : 'household'}
                onChange={(value) => {
                  const nextMember = /^user-(\d+)$/.exec(value)
                  setMember(nextMember ? Number(nextMember[1]) : null)
                  setMine(value === 'mine')
                  setPage(1)
                }}
                options={[
                  { value: 'mine', label: 'My shelf' },
                  { value: 'household', label: 'Household' },
                  ...members.map((entry) => ({
                    value: `user-${entry.id}`,
                    label: entry.displayName,
                  })),
                ]}
              />
            </div>
            )}
            {!isChild && mode === 'books' && facets && facets.formats.length > 1 && <select
              aria-label="Filter by format"
              value={format}
              onChange={(event) => { setFormat(event.target.value); setPage(1) }}
              className="rounded-card bg-surface-2 px-3 py-2 text-xs text-ink-soft outline-none focus-visible:outline-2 focus-visible:outline-focus"
            >
              <option value="">All formats</option>
              {facets.formats.map((facet) => <option key={facet.value} value={facet.value}>{facet.value.toUpperCase()} ({facet.count})</option>)}
            </select>}
            {!isChild && mode === 'books' && facets && facets.languages.length > 1 && <select
              aria-label="Filter by language"
              value={language}
              onChange={(event) => { setLanguage(event.target.value); setPage(1) }}
              className="rounded-card bg-surface-2 px-3 py-2 text-xs text-ink-soft outline-none focus-visible:outline-2 focus-visible:outline-focus"
            >
              <option value="">All languages</option>
              {facets.languages.map((facet) => <option key={facet.value} value={facet.value}>{facet.value === 'und' ? 'Unknown' : facet.value.toUpperCase()} ({facet.count})</option>)}
            </select>}
            <select
              aria-label="Sort by"
              value={sort}
              onChange={(event) => {
                setSort(event.target.value as BookSort)
                setPage(1)
              }}
              className="bg-transparent text-xs text-ink-muted outline-none transition-colors hover:text-ink focus-visible:outline-2 focus-visible:outline-focus"
            >
              {sortOptions.filter((option) => category !== 'comics' || option.value !== 'author').map((option) => (
                <option key={option.value} value={option.value}>
                  Sort: {option.label}
                </option>
              ))}
            </select>

            {!isChild && (
            <>
            <details className="relative">
              <summary className="flex cursor-pointer list-none items-center gap-1.5 text-xs font-medium text-ink-muted transition-colors hover:text-ink [&::-webkit-details-marker]:hidden">
                <SlidersHorizontal size={13} aria-hidden />
                More filters
                {Boolean(series || subject || collection || missing) && (
                  <span className="text-accent">({[series, subject, collection, missing].filter(Boolean).length})</span>
                )}
              </summary>

              <div className="mt-3 w-[min(21rem,calc(100vw-2.5rem))] space-y-4 rounded-panel bg-surface-2 p-4 lg:absolute lg:left-0 lg:z-30 lg:mt-2 lg:shadow-modal">
                {collections.length > 0 && (
                  <label className="block">
                    <span className="text-xs uppercase tracking-[0.14em] text-ink-faint">
                      Collection
                    </span>
                    <select
                      aria-label="Filter by collection"
                      value={collection}
                      onChange={(event) => {
                        setCollection(event.target.value)
                        setPage(1)
                        closeFilters(event)
                      }}
                      className="mt-2 w-full rounded-card bg-surface-3 px-3 py-2 text-xs text-ink-soft outline-none focus-visible:outline-2 focus-visible:outline-focus"
                    >
                      <option value="">All collections</option>
                      {collections.map((item) => (
                        <option key={item.id} value={String(item.id)}>
                          {item.name} ({item.bookCount})
                        </option>
                      ))}
                    </select>
                  </label>
                )}

                {facets && facets.series.length > 0 && (
                  <FacetPicker
                    label="Series"
                    allLabel="All series"
                    value={series}
                    options={facets.series.map((facet) => ({
                      value: facet.value,
                      label: facet.value,
                      count: facet.count,
                    }))}
                    onChange={(value) => {
                      setSeries(value)
                      setPage(1)
                    }}
                  />
                )}

                {facets && facets.subjects.length > 0 && (
                  <FacetPicker
                    label="Subject"
                    allLabel="All subjects"
                    value={subject}
                    options={facets.subjects.map((facet) => ({
                      value: facet.normalized,
                      label: facet.name,
                      count: facet.count,
                    }))}
                    onChange={(value) => {
                      setSubject(value)
                      setPage(1)
                    }}
                  />
                )}

                {activeFilterCount > 0 && (
                  <button
                    type="button"
                    onClick={(event) => {
                      clearAllFilters()
                      closeFilters(event)
                    }}
                    className="text-xs text-ink-muted transition-colors hover:text-ink"
                  >
                    Clear all filters
                  </button>
                )}
              </div>
            </details>

            {activeFilters.map((filter) => (
              <button
                key={filter.key}
                type="button"
                onClick={filter.clear}
                className="inline-flex items-center gap-1.5 rounded-full bg-surface-2 px-3 py-1.5 text-xs text-ink-soft transition-colors hover:bg-surface-3 hover:text-ink"
              >
                {filter.label}
                <X size={12} aria-hidden />
              </button>
            ))}
            </>
            )}
          </div>
        )}
      </div>

      {booksError && (
        <p className="mt-6 border-l-2 border-danger pl-4 text-sm text-danger">{booksError}</p>
      )}

      {mode === 'books' && sort !== 'recent' && !groupedComics && (
        <LetterIndex
          active={letter}
          available={bookLetters}
          onSelect={(value) => {
            setLetter(value)
            setAppended([])
          }}
        />
      )}
      {mode === 'authors' && !authorQuery && (
        <LetterIndex
          active={authorLetter}
          available={authorLetters}
          onSelect={(value) => {
            const next = new URLSearchParams(searchParams)
            next.set('mode', 'authors')
            if (value) {
              next.set('authors_letter', value)
            } else {
              next.delete('authors_letter')
            }
            setSearchParams(next, { replace: true })
          }}
        />
      )}

      <div className="mt-8">
        {mode === 'books' ? groupedComics ? (
          <ComicShelf mine={mine} user={member ?? undefined} sort={sort === 'title' ? 'title' : 'recent'} />
        ) : (
          <>
            {booksLoading && (
              <DelayedLoadingStatus
                key={booksKey}
                message={trimmedQuery ? 'Searching your library…' : 'Loading your library…'}
              />
            )}
            {(!booksLoading || displayedBooks.length > 0) && (
            <BookGrid
              books={displayedBooks}
              appearance="shelf"
              letterFor={letterForBook}
              className={booksLoading ? 'opacity-60 transition-opacity' : undefined}
              emptyMessage={
                trimmedQuery
                  ? memberName
                    ? `No books on ${memberName}'s shelf match your search.`
                    : mine
                      ? 'No books on your shelf match your search.'
                      : 'No books in the household library match your search.'
                  : filtersActive
                    ? 'No books match these filters.'
                    : isChild
                      ? 'Ask an adult to add books to your shelf.'
                      : memberName
                        ? `${memberName}'s shelf is empty.`
                        : mine
                          ? 'Your shelf is empty. Find your first book and Bokhylle will fetch it for you.'
                          : 'The household library is empty.'
              }
              emptyAction={
                isChild ? undefined : !trimmedQuery && mine && !filtersActive ? (
                  <div className="flex flex-wrap items-center justify-center gap-3">
                    <Button
                      variant="primary"
                      size="md"
                      disabled={claimMutation.busyKey === 'claim'}
                      onClick={() =>
                        void claimMutation.run(
                          'claim',
                          claimShelf,
                          'Could not add the household books',
                          () => setReload((value) => value + 1),
                        )
                      }
                    >
                      {claimMutation.busyKey === 'claim'
                        ? 'Adding…'
                        : 'Add all household books to my shelf'}
                    </Button>
                    <ButtonLink to="/library?scope=household" variant="secondary" size="md">
                      Browse household library
                    </ButtonLink>
                    {claimMutation.error && (
                      <p className="w-full text-center text-sm text-danger">
                        {claimMutation.error}
                      </p>
                    )}
                  </div>
                ) : trimmedQuery ? (
                  mine ? (
                    <Button
                      variant="primary"
                      size="md"
                      onClick={() => {
                        setMine(false)
                        setPage(1)
                      }}
                    >
                      Search household for “{trimmedQuery}”
                    </Button>
                  ) : (
                    <ButtonLink
                      to={`/discover?q=${encodeURIComponent(trimmedQuery)}`}
                      variant="primary"
                      size="md"
                    >
                      Search Discover for “{trimmedQuery}”
                      <ArrowRight size={15} aria-hidden />
                    </ButtonLink>
                  )
                ) : !filtersActive ? (
                  <ButtonLink to="/discover" variant="primary" size="md">
                    Discover books
                    <ArrowRight size={15} aria-hidden />
                  </ButtonLink>
                ) : undefined
              }
            />
            )}
            {!trimmedQuery && hasMore && (
              <div ref={sentinelRef} className="mt-8 flex items-center justify-center gap-4">
                <Button
                  variant="secondary"
                  size="sm"
                  disabled={loadingMore}
                  onClick={() => void loadMore()}
                >
                  {loadingMore ? 'Loading…' : `Load more (${books.length} of ${total})`}
                </Button>
              </div>
            )}
          </>
        ) : (
          <>
            {authorsError && (
              <p className="mb-6 border-l-2 border-danger pl-4 text-sm text-danger">
                {authorsError}
              </p>
            )}
            {authorsLoading ? (
              <div className="grid gap-x-8 sm:grid-cols-2 lg:grid-cols-3">
                {Array.from({ length: 6 }).map((_, index) => (
                  <div key={index} className="h-16 animate-pulse border-b border-line" />
                ))}
              </div>
            ) : authors.length === 0 ? (
              <p className="border-l-2 border-line pl-4 text-sm text-ink-muted">
                No authors yet. They appear here as your shelf grows.
              </p>
            ) : (
              <div className="grid gap-x-8 sm:grid-cols-2 lg:grid-cols-3">
                {filteredAuthors.length === 0 && (
                  <p className="py-6 text-sm text-ink-muted">
                    No matching authors here.{' '}
                    <Link to="/discover" className="text-accent hover:text-accent-strong">
                      Find authors in Discover →
                    </Link>
                  </p>
                )}
                {filteredAuthors.map((author) => (
                  <div
                    key={author.id}
                    data-letter={author.name.slice(0, 1).toLowerCase()}
                    className="flex flex-wrap items-center gap-3 border-b border-line py-4"
                  >
                    <Link
                      to={`/authors/${author.id}`}
                      state={{ authorReturnTo: `${location.pathname}${location.search}${location.hash}` }}
                      className="group flex min-w-0 flex-1 items-center gap-3"
                    >
                      <AuthorAvatar
                        authorId={author.id}
                        name={author.name}
                        className="h-10 w-10 text-base"
                      />
                      <span className="min-w-0">
                        <span className="block truncate font-display text-base text-ink transition-colors group-hover:text-accent">
                          {author.name}
                        </span>
                        <span className="mt-0.5 block font-sans text-[11px] uppercase tracking-[0.14em] text-ink-faint">
                          {author.bookCount} {author.bookCount === 1 ? 'book' : 'books'}
                          {author.following ? ' · Following' : ''}
                          {author.autoAcquire ? ' · Automatic new releases' : ''}
                        </span>
                      </span>
                    </Link>
                    <div className="flex items-center gap-2">
                      <Button
                        variant={author.following ? 'secondary' : 'ghost'}
                        size="sm"
                        onClick={() => void toggleAuthorFollow(author)}
                      >
                        {author.following ? 'Following' : 'Follow'}
                      </Button>

                    </div>
                  </div>
                ))}
              </div>
            )}
          </>
        )}
      </div>
    </section>
  )
}

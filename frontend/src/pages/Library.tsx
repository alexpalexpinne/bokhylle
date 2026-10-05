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
  fetchBook,
  searchBooks,
  setAuthorFollow,
  setBooksSharing,
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
import { RetryNotice } from '../components/ui/RetryNotice'
import { browseGeneration, browseSessionGeneration, readBrowseState, saveBrowseState } from '../lib/browseState'

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
  key: string
  items: AuthorSummary[]
  error: string | null
}

type LibraryVisit = { booksResult: BooksResult | null; authorsResult: AuthorsResult | null; appended: BookSummary[]; nextPage: number; reload: number }

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
  const { user } = useAuth()
  return <LibraryView key={JSON.stringify([user?.id, user?.role, user?.profileType])} />
}

function LibraryView() {
  const { user, demo } = useAuth()
  const location = useLocation()
  const isChild = user?.profileType === 'child'
  const visitKey = `library:${JSON.stringify([user?.id, user?.role, user?.profileType])}:${location.key}:${location.search}`
  const [saved] = useState(() => readBrowseState<LibraryVisit>(visitKey))
  const dataGeneration = useRef(browseGeneration())
  const sessionGeneration = useRef(browseSessionGeneration())

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
  const [savedPages] = useState(() => readBrowseState<number>(`view:${visitKey}`) ?? saved?.nextPage ?? page + 1)

  const [booksResult, setBooksResult] = useState<BooksResult | null>(saved?.booksResult ?? null)
  const [authorsResult, setAuthorsResult] = useState<AuthorsResult | null>(saved?.authorsResult ?? null)
  const [followError, setFollowError] = useState<string | null>(null)
  const claimMutation = useMutation()
  const sharingMutation = useMutation()
  const [selectingBooks, setSelectingBooks] = useState(false)
  const [ownedBookIds, setOwnedBookIds] = useState<Set<number>>(new Set())
  const [ownershipLoading, setOwnershipLoading] = useState(false)
  const [selectedBookIds, setSelectedBookIds] = useState<Set<number>>(new Set())
  const [sharingNotice, setSharingNotice] = useState<string | null>(null)
  const [facets, setFacets] = useState<BookFacets | null>(null)
  const [collections, setCollections] = useState<CollectionSummary[]>([])
  const [mine, setMine] = useState(() => searchParams.get('scope') !== 'household')
  // An assigned child id when an administrator browses that shelf.
  const [member, setMember] = useState<number | null>(null)
  const [members, setMembers] = useState<HouseholdMember[]>([])
  const [reload, setReload] = useState(saved?.reload ?? 0)
  const [authorsRetry, setAuthorsRetry] = useState(0)
  const [refreshingBooks, setRefreshingBooks] = useState(false)
  const [refreshingAuthors, setRefreshingAuthors] = useState(false)
  const [format, setFormat] = useState(() => searchParams.get('format') ?? '')
  const [language, setLanguage] = useState(() => searchParams.get('language') ?? '')
  const [series, setSeries] = useState(() => searchParams.get('series') ?? '')
  const [subject, setSubject] = useState(() => searchParams.get('subject') ?? '')
  const [collection, setCollection] = useState(() => searchParams.get('collection') ?? '')
  const [letter, setLetter] = useState(() => searchParams.get('letter') ?? '')
  const [missing, setMissing] = useState(() => searchParams.get('missing') ?? '')
  const [appended, setAppended] = useState<BookSummary[]>(saved?.appended ?? [])
  const [nextPage, setNextPage] = useState(saved?.nextPage ?? savedPages)
  const loadedPage = useRef(nextPage - 1)
  const [loadingMore, setLoadingMore] = useState(false)
  const [loadMoreError, setLoadMoreError] = useState<string | null>(null)
  const [restoreError, setRestoreError] = useState<string | null>(null)
  const loadingMoreRef = useRef(false)
  const loadMoreFailed = useRef(false)
  const sentinelRef = useRef<HTMLDivElement | null>(null)
  const writtenQueryRef = useRef('')

  const trimmedQuery = query.trim()
  const memberName = member
    ? (members.find((entry) => entry.id === member)?.displayName ?? 'Member')
    : null
  const booksKey = `${trimmedQuery}|${sort}|${page}|${mine}|${member}|${category}|${format}|${language}|${series}|${subject}|${collection}|${letter}|${missing}`
  const scopeKey = `${mine}|${member}`
  const authorsKey = `${mine}|${followingOnly}|${member}`
  const firstBooksKey = useRef(booksKey)
  const currentBooksKey = useRef(booksKey)
  currentBooksKey.current = booksKey
  const previousBooksKey = useRef(booksKey)

  useEffect(() => {
    saveBrowseState(visitKey, { booksResult, authorsResult, appended, nextPage, reload }, dataGeneration.current)
    if (booksResult?.key === booksKey) saveBrowseState(`view:${visitKey}`, nextPage, sessionGeneration.current)
  }, [visitKey, booksResult, authorsResult, appended, nextPage, reload, booksKey])

  useEffect(() => {
    if (mode !== 'books') {
      return
    }

    let cancelled = false
    const generation = browseSessionGeneration()

    const requestKey = `${trimmedQuery}|${sort}|${page}|${mine}|${member}|${category}|${format}|${language}|${series}|${subject}|${collection}|${letter}|${missing}`
    const kind = category === 'all' ? undefined : category === 'books' ? 'books' : 'comics'
    const timer = setTimeout(
      () => {
        setRefreshingBooks(true)
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
          .then(async (result) => {
            if (!cancelled && generation === browseSessionGeneration()) {
              dataGeneration.current = browseGeneration()
              setBooksResult({
                key: requestKey,
                scope: `${mine}|${member}`,
                items: result.items,
                total: result.total,
                letters: result.letters ?? [],
                pageSize: result.pageSize,
                error: null,
              })
              // Revalidate every restored page, rather than keeping old book
              // metadata indefinitely or forcing Back to return to page one.
              if (!trimmedQuery && requestKey === firstBooksKey.current && savedPages > page + 1) {
                try {
                  const pages = await Promise.all(Array.from({ length: Math.max(0, Math.min(savedPages - page - 1, Math.ceil(result.total / 24) - page)) }, (_, index) => fetchBooks(sort, page + index + 1, 24, {
                    mine, user: member ?? undefined, kind, format, language, series, subject: subject || undefined,
                    collection: collection ? Number(collection) : undefined, letter: letter || undefined, missing: missing || undefined,
                  })))
                  if (cancelled || generation !== browseSessionGeneration() || currentBooksKey.current !== requestKey) return
                  setAppended(pages.flatMap((entry) => entry.items))
                  loadedPage.current = page + pages.length
                  setNextPage(loadedPage.current + 1)
                } catch {
                  if (!cancelled && currentBooksKey.current === requestKey) setRestoreError('Could not restore all your books. Please try again.')
                }
              }
            }
          })
          .finally(() => { if (!cancelled && generation === browseSessionGeneration()) setRefreshingBooks(false) })
          .catch((caught: unknown) => {
            if (!cancelled && generation === browseSessionGeneration()) {
              setBooksResult((previous) => ({
                key: requestKey,
                scope: `${mine}|${member}`,
                items: previous?.key === requestKey ? previous.items : [],
                total: previous?.key === requestKey ? previous.total : 0,
                letters: previous?.key === requestKey ? previous.letters : [],
                pageSize: previous?.key === requestKey ? previous.pageSize : 24,
                error: caught instanceof ApiError ? caught.message : 'Failed to load library',
              }))
            }
          })
      },
      trimmedQuery ? 125 : 0,
    )

    return () => {
      cancelled = true
      clearTimeout(timer)
    }
  }, [mode, trimmedQuery, sort, page, mine, member, reload, category, format, language, series, subject, collection, letter, missing, savedPages])

  useEffect(() => {
    if (previousBooksKey.current === booksKey) return
    previousBooksKey.current = booksKey
    setAppended([])
    setNextPage(page + 1)
    loadedPage.current = page
    loadMoreFailed.current = false
    setLoadMoreError(null)
    setRestoreError(null)
  }, [booksKey, page])

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
    if (mode === 'authors' && authorQuery) next.set('authors_q', authorQuery)
    if (mode === 'authors' && authorLetter) next.set('authors_letter', authorLetter)
    if (category !== 'all') next.set('category', category)
    if (mode === 'authors' && followingOnly) next.set('following', '1')

    if (next.toString() !== searchParams.toString()) {
      writtenQueryRef.current = query
      setSearchParams(next, { replace: true })
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, sort, page, mine, member, category, format, language, series, subject, collection, letter, missing, mode, followingOnly, authorQuery, authorLetter])

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

    const generation = browseSessionGeneration()
    async function load() {
      setRefreshingAuthors(true)
      await fetchAuthors(mine, followingOnly, member ?? undefined)
      .then((items) => {
        if (!cancelled && generation === browseSessionGeneration()) {
          dataGeneration.current = browseGeneration()
          setAuthorsResult({ key: authorsKey, items, error: null })
        }
      })
      .catch((caught: unknown) => {
        if (!cancelled && generation === browseSessionGeneration()) {
          setAuthorsResult((previous) => ({
            key: authorsKey,
            items: previous?.key === authorsKey ? previous.items : [],
            error: caught instanceof ApiError ? caught.message : 'Failed to load authors',
          }))
        }
      })
      .finally(() => { if (!cancelled && generation === browseSessionGeneration()) setRefreshingAuthors(false) })
    }
    void load()

    return () => {
      cancelled = true
    }
  }, [mode, mine, followingOnly, isChild, member, authorsRetry, authorsKey])

  const booksLoading = mode === 'books' && booksResult?.key !== booksKey
  const baseBooks = booksResult?.key === booksKey ? booksResult.items : []
  const books = appended.length > 0 ? [...baseBooks, ...appended] : baseBooks
  const displayedBooks = booksLoading
    ? booksResult?.scope === scopeKey ? booksResult.items : []
    : books
  const displayedBookKey = displayedBooks.map((book) => book.id).join(',')
  useEffect(() => {
    if (!selectingBooks) return
    let cancelled = false
    async function loadOwnership() {
      setOwnershipLoading(true)
      const ids = displayedBookKey.split(',').filter(Boolean).map(Number)
      const results = await Promise.allSettled(ids.map((id) => fetchBook(id)))
      if (cancelled) return
      const owned = new Set<number>()
      for (const result of results) {
        if (result.status === 'fulfilled' && result.value.sharing) owned.add(result.value.id)
      }
      setOwnedBookIds(owned)
      setSelectedBookIds(new Set())
      setOwnershipLoading(false)
      if (results.some((result) => result.status === 'rejected')) setSharingNotice('Some books could not be checked. Only confirmed books you acquired can be selected.')
    }
    void loadOwnership()
    return () => { cancelled = true }
  }, [selectingBooks, displayedBookKey])
  const total = booksResult?.key === booksKey ? booksResult.total : 0
  const booksError = booksResult?.key === booksKey ? booksResult.error : null
  const booksLoaded = booksResult?.key === booksKey
  const hasMore = !trimmedQuery && booksLoaded && nextPage <= Math.ceil(total / Math.max(1, booksResult?.pageSize ?? 24))

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

  async function loadMore(retry = false) {
    if (loadingMoreRef.current || refreshingBooks || !hasMore || nextPage <= loadedPage.current || (loadMoreFailed.current && !retry)) {
      return
    }
    const requestKey = booksKey
    const generation = browseSessionGeneration()
    loadingMoreRef.current = true
    loadMoreFailed.current = false
    setLoadingMore(true)
    setLoadMoreError(null)
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
      if (currentBooksKey.current === requestKey && generation === browseSessionGeneration()) {
        // An observer callback from the old render can still be queued when
        // this fast response lands. Mark the page synchronously before React
        // commits the new list and continuation.
        loadedPage.current = nextPage
        setAppended((current) => [...current, ...result.items])
        setNextPage(nextPage + 1)
      }
    } catch {
      if (currentBooksKey.current === requestKey && generation === browseSessionGeneration()) {
        loadMoreFailed.current = true
        setLoadMoreError('Could not load more books. Your current books are still shown.')
      }
    } finally {
      loadingMoreRef.current = false
      setLoadingMore(false)
    }
  }

  useEffect(() => {
    const node = sentinelRef.current
    if (!node || !hasMore || loadMoreError || restoreError || refreshingBooks) {
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
  }, [hasMore, loadingMore, loadMoreError, restoreError, refreshingBooks, appended.length, sort, mine, category, format, language, series, subject, collection, letter, missing])
  const filtersActive = Boolean(format || language || series || subject || collection || missing)
  const groupedComics = mode === 'books' && category === 'comics' && !trimmedQuery && !filtersActive && !letter && !selectingBooks
  const hasComics = facets?.publicationKinds?.some((item) => (item.value === 'comic' || item.value === 'manga') && item.count > 0) ?? false
  const hasBooks = facets?.publicationKinds?.some((item) => (item.value === 'book' || item.value === 'unknown') && item.count > 0) ?? false
  const isEmptyShelf = booksLoaded && !trimmedQuery && !filtersActive && total === 0

  const authorsLoading = mode === 'authors' && authorsResult?.key !== authorsKey
  const authors = authorsResult?.key === authorsKey ? authorsResult.items : []
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
  const authorsError = authorsResult?.key === authorsKey ? authorsResult.error : null

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
            facets?.subjects.find((facet) => (facet.normalized === subject || facet.aliases?.includes(subject)))?.name ?? subject,
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

      <div className="sticky top-[var(--app-header-height)] z-30 -mx-4 mt-6 bg-canvas/90 px-4 py-3 backdrop-blur sm:-mx-6 sm:px-6 lg:-mx-8 lg:px-8">
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
                    value={facets.subjects.find((facet) => facet.normalized === subject || facet.aliases?.includes(subject))?.normalized ?? subject}
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
        <RetryNotice className="mt-6" message={booksError} busy={refreshingBooks} onRetry={() => setReload((value) => value + 1)} />
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
        {mode === 'books' && mine && !member && !isChild && !demo && <div className="mb-4">
          <details>
            <summary className="flex min-h-11 w-fit cursor-pointer list-none items-center text-sm text-ink-muted hover:text-ink [&::-webkit-details-marker]:hidden">More</summary>
            <Button variant="ghost" size="sm" disabled={!!sharingMutation.busyKey} onClick={() => { setSelectingBooks((current) => !current); setSelectedBookIds(new Set()); setOwnedBookIds(new Set()); setOwnershipLoading(true); setSharingNotice(null) }}>{selectingBooks ? 'Done selecting' : 'Change sharing for several books'}</Button>
          </details>
          {selectingBooks && <div className="mt-3 space-y-3 border-y border-line py-3">
            <div className="flex flex-wrap items-center gap-3">
              <span className="text-xs text-ink-muted">{selectedBookIds.size} selected</span>
              <Button variant="ghost" size="sm" disabled={ownershipLoading || !!sharingMutation.busyKey} onClick={() => setSelectedBookIds(new Set(displayedBooks.filter((book) => ownedBookIds.has(book.id)).map((book) => book.id)))}>Select visible books I acquired</Button>
              {(['private', 'shared'] as const).map((sharing) => <Button key={sharing} size="sm" disabled={ownershipLoading || !selectedBookIds.size || !!sharingMutation.busyKey} onClick={() => void sharingMutation.run(sharing, () => setBooksSharing([...selectedBookIds], sharing), 'Could not update sharing', () => { setSelectedBookIds(new Set()); setReload((current) => current + 1); setSharingNotice(`Your sharing for the selected books is now ${sharing}. Independent owners keep their access and sharing choice.`) })}>{sharingMutation.busyKey === sharing ? 'Saving…' : sharing === 'private' ? 'Make private' : 'Share with household'}</Button>)}
            </div>
            <p className="text-xs text-ink-muted">Only books you acquired can be changed. Books added from someone else's shared collection belong to their owner.</p>
            {ownershipLoading && <p role="status" className="text-xs text-ink-muted">Checking which books you acquired…</p>}
          </div>}
          {sharingNotice && <p role="status" className="mt-3 text-sm text-ink-soft">{sharingNotice}</p>}
          {sharingMutation.error && <p role="alert" className="mt-3 text-sm text-danger">{sharingMutation.error}</p>}
        </div>}
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
            {(!booksError || displayedBooks.length > 0) && (!booksLoading || displayedBooks.length > 0) && (
            <BookGrid
              books={displayedBooks}
              selectedIds={selectedBookIds}
              selectableIds={ownedBookIds}
              selectionDisabled={ownershipLoading || !!sharingMutation.busyKey}
              onSelect={selectingBooks && mine && !member && !isChild ? (bookId, selected) => setSelectedBookIds((current) => { const next = new Set(current); if (selected) next.add(bookId); else next.delete(bookId); return next }) : undefined}
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
            {loadMoreError && <RetryNotice className="mt-6" message={loadMoreError} busy={loadingMore} onRetry={() => void loadMore(true)} />}
            {restoreError && <RetryNotice className="mt-6" message={restoreError} busy={refreshingBooks} onRetry={() => { setRestoreError(null); setReload((value) => value + 1) }} />}
            {!trimmedQuery && hasMore && !loadMoreError && !restoreError && (
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
              <RetryNotice className="mb-6" message={authorsError} busy={refreshingAuthors} onRetry={() => setAuthorsRetry((value) => value + 1)} />
            )}
            {authorsLoading ? (
              <div className="grid gap-x-8 sm:grid-cols-2 lg:grid-cols-3">
                {Array.from({ length: 6 }).map((_, index) => (
                  <div key={index} className="h-16 animate-pulse border-b border-line" />
                ))}
              </div>
            ) : authorsError && authors.length === 0 ? null : authors.length === 0 ? (
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

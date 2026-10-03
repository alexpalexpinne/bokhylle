import { useEffect, useState } from 'react'
import { Link, useLocation, useNavigate } from 'react-router-dom'
import {
  ArrowRight,
  Check,
  ChevronDown,
  Heart,
  Loader2,
  Search,
  Send,
  SlidersHorizontal,
} from 'lucide-react'
import { ApiError } from '../api/client'
import { AuthorAvatar } from '../components/AuthorAvatar'
import { EmptyState } from '../components/ui/EmptyState'
import {
  type AuthorHit,
  ensureDiscoverAuthor,
  followDiscoverAuthor,
  setAuthorFollow,
} from '../api/library'
import {
  type AcquisitionStatus,
  createAcquisitionForBook,
  createAcquisitionFromDiscovery,
} from '../api/acquisitions'
import { deliverBook, fetchDefaultReader } from '../api/delivery'
import { createBookRequest } from '../api/requests'
import { startDemoGet } from '../api/demo'
import {
  type DiscoveryDetail,
  type DiscoveryResult,
  type DiscoveryStatus,
  type SearchType,
  discoverCoverUrl,
  fetchDiscoverBook,
  likeExternalBook,
} from '../api/discover'
import {
  type BookDetail as BookDetailData,
  addBookToShelf,
  coverUrl,
  fetchBook,
  setBookPreference,
} from '../api/library'
import { BookCover } from '../components/BookCover'
import { BookSharingMarker } from '../components/BookSharingMarker'
import { ReleaseChoices } from '../components/ReleaseChoices'
import { DemoSendDialog } from '../components/DemoSendDialog'
import { useAuth } from '../auth/useAuth'
import { Button, ButtonLink } from '../components/ui/Button'
import { Modal } from '../components/ui/Modal'
import { MetaLine } from '../components/ui/MetaLine'
import { descriptionText } from '../lib/descriptionText'
import { PageHeader } from '../components/ui/PageHeader'
import { SearchField } from '../components/ui/SearchField'
import { SectionMark } from '../components/ui/SectionMark'
import { useDiscoverSearch } from './useDiscoverSearch'
import { ShelfGrid } from '../components/ShelfGrid'
import { ShelfCover } from '../components/ShelfRail'

type SheetFormat = 'epub' | 'any'

const searchTypes: { value: SearchType; label: string; hint: string }[] = [
  { value: 'any', label: 'Anywhere', hint: 'Title, author or ISBN' },
  { value: 'title', label: 'Title', hint: 'Search by book title' },
  { value: 'author', label: 'Author', hint: 'Search by author name' },
  { value: 'isbn', label: 'ISBN', hint: 'Search by ISBN' },
  { value: 'subject', label: 'Subject', hint: 'Browse an Open Library subject' },
]

function shownLanguage(result: DiscoveryResult, preferred: string[]): string | null {
  return result.languages?.find((language) => preferred.includes(language)) ?? result.language
}

/// The card shows one of three states; map the acquisition state the backend
/// actually returned instead of assuming a download started.
function discoveryStatusFromAcquisition(status: AcquisitionStatus): DiscoveryStatus {
  if (status === 'READY') {
    return 'IN_LIBRARY'
  }
  if (
    status === 'CANCELLED' ||
    status === 'NO_RELEASE_FOUND' ||
    status === 'DOWNLOAD_FAILED' ||
    status === 'IMPORT_FAILED'
  ) {
    return 'NOT_IN_LIBRARY'
  }
  return 'DOWNLOADING'
}

export function Discover({ detailOnly = false }: { detailOnly?: boolean }) {
  const [advanced, setAdvanced] = useState(false)
  const [shelfBusy, setShelfBusy] = useState<string | null>(null)
  const [adding, setAdding] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [selected, setSelected] = useState<DiscoveryResult | null>(null)
  const [detail, setDetail] = useState<DiscoveryDetail | null>(null)
  const [detailLoading, setDetailLoading] = useState(false)
  const [preferredFormat, setPreferredFormat] = useState<SheetFormat>('any')
  const { user, demo } = useAuth()
  const [choosingId, setChoosingId] = useState<string | null>(null)
  const [choiceBusy, setChoiceBusy] = useState(false)
  const [hasReader, setHasReader] = useState<boolean | null>(null)
  const [householdReader, setHouseholdReader] = useState<string | null>(null)
  const [formatOpen, setFormatOpen] = useState(false)
  const [ownedDetail, setOwnedDetail] = useState<BookDetailData | null>(null)
  const [detailError, setDetailError] = useState<string | null>(null)
  const [sending, setSending] = useState<'library' | 'reader' | null>(null)
  const [externalLikedId, setExternalLikedId] = useState<number | null>(null)
  const [demoSendBook, setDemoSendBook] = useState<{ id: number; title: string } | null>(null)
  const navigate = useNavigate()
  const location = useLocation()

  const defaultLanguages = user?.preferredLanguages?.length
    ? user.preferredLanguages
    : user?.preferredLanguage
      ? [user.preferredLanguage]
      : user?.defaultLanguage
        ? [user.defaultLanguage]
        : []
  const {
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
  } = useDiscoverSearch({
    userId: user?.id ?? 0,
    defaultLanguages,
    demo,
    detailOnly,
    setNotice,
  })
  const activeType = searchTypes.find((item) => item.value === type) ?? searchTypes[0]

  /// Applies a server-confirmed change to the visible result, the open sheet
  /// and the cached pages, so all three agree after a mutation.
  function patchResult(provider: string, providerKey: string, patch: Partial<DiscoveryResult>) {
    patchSearchResult(provider, providerKey, patch)
    setSelected((current) =>
      current && current.provider === provider && current.providerKey === providerKey ? { ...current, ...patch } : current,
    )
    setDetail((current) => {
      if (!current || current.provider !== provider || current.providerKey !== providerKey) {
        return current
      }
      const next = { ...current }
      if (patch.status !== undefined) {
        next.status = patch.status
      }
      if (patch.ownedBookId !== undefined) {
        next.ownedBookId = patch.ownedBookId
      }
      if (patch.ownedFileId !== undefined) {
        next.ownedFileId = patch.ownedFileId
      }
      if (patch.onShelf !== undefined) {
        next.onShelf = patch.onShelf
      }
      return next
    })
  }

  // Deep link: a provider + providerKey opens that exact book's sheet.
  useEffect(() => {
    const providerKey = searchParams.get('providerKey')
    const provider = searchParams.get('provider') ?? 'openlibrary'
    if (!providerKey || selected) {
      return
    }
    openDetail({
      provider,
      providerKey,
      title: '',
      authors: [],
      languages: [],
      ratingAverage: null,
      ratingCount: null,
      editionCount: null,
      popularity: null,
      year: null,
      language: null,
      isbn10: null,
      isbn13: null,
      series: null,
      seriesNumber: null,
      coverId: null,
      status: 'NOT_IN_LIBRARY',
      ownedBookId: null,
      ownedFileId: null,
      onShelf: false,
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  async function openAuthorHit(hit: AuthorHit) {
    const authorReturnTo = `${location.pathname}${location.search}${location.hash}`
    if (hit.authorId !== null) {
      navigate(`/authors/${hit.authorId}`, { state: { authorReturnTo } })
      return
    }
    try {
      const created = await ensureDiscoverAuthor({
        name: hit.name,
        provider: hit.provider,
        providerKey: hit.providerKey,
      })
      updateAuthors((items) =>
        items.map((item) =>
          item.name === hit.name && item.providerKey === hit.providerKey
            ? { ...item, authorId: created.authorId }
            : item,
        ),
      )
      navigate(`/authors/${created.authorId}`, { state: { authorReturnTo } })
    } catch (caught) {
      setNotice(caught instanceof ApiError ? caught.message : 'Could not open that author')
    }
  }

  async function toggleAuthorHit(hit: AuthorHit) {
    const next = !hit.following
    const matches = (item: AuthorHit) =>
      item.name === hit.name && item.providerKey === hit.providerKey
    const patch = (update: Partial<AuthorHit>) =>
      updateAuthors((items) =>
        items.map((item) => (matches(item) ? { ...item, ...update } : item)),
      )

    patch({ following: next })
    try {
      if (hit.authorId !== null) {
        await setAuthorFollow(hit.authorId, next)
      } else if (next) {
        // A transient provider hit becomes durable only by following it.
        const created = await followDiscoverAuthor({
          name: hit.name,
          provider: hit.provider,
          providerKey: hit.providerKey,
        })
        patch({ authorId: created.authorId })
      }
    } catch {
      patch({ following: hit.following })
    }
  }

  function openDetail(item: DiscoveryResult) {
    setChoosingId(null)
    setSelected(item)
    setHasReader(null)
    setDetail(null)
    setOwnedDetail(null)
    setExternalLikedId(null)
    setDetailError(null)
    setDetailLoading(true)
    // The profile preference decides; the sheet only changes it on demand.
    setPreferredFormat(user?.preferredFormat === 'any' ? 'any' : 'epub')
    setFormatOpen(false)
    setNotice(null)

    if (item.ownedBookId) {
      // Owned books open from local data; no Open Library round-trip.
      fetchBook(item.ownedBookId)
        .then(setOwnedDetail)
        .catch(() => setDetailError('Could not load this book from your library.'))
        .finally(() => setDetailLoading(false))
    } else {
      fetchDiscoverBook(item.providerKey, item.provider)
        .then((loaded) => {
          setDetail(loaded)
          setSelected((current) => current?.provider === item.provider && current.providerKey === item.providerKey
            ? { ...current, ...loaded }
            : current)
        })
        .catch(() =>
          setDetailError('Details could not be loaded. You can still get the book.'),
        )
        .finally(() => setDetailLoading(false))
    }

    if (demo) {
      setHasReader(false)
    } else {
      fetchDefaultReader()
        .then((reader) => {
          setHasReader(reader.address !== null)
          setHouseholdReader(reader.source === 'household' ? reader.address : null)
        })
        .catch((caught: unknown) => console.warn('discover.default_reader.load_failed', caught))
    }
  }

  function closeDetail() {
    if (choiceBusy) return
    setChoosingId(null)
    setSelected(null)
    if ((location.state as { backgroundLocation?: unknown } | null)?.backgroundLocation) {
      navigate(-1)
    } else if (searchParams.has('providerKey')) {
      const next = new URLSearchParams(searchParams)
      next.delete('providerKey')
      next.delete('provider')
      navigate(`/discover${next.size ? `?${next}` : ''}`, { replace: true })
    }
  }

  function searchAuthor(author: string) {
    if (detailOnly) {
      navigate(`/discover?q=${encodeURIComponent(author)}&type=author`, { replace: true })
      return
    }
    setInput(author)
    void runSearch(author, 'author')
  }

  async function sendOwned() {
    if (!ownedBookId || !ownedFileId) {
      return
    }
    if (demo) {
      setDemoSendBook({ id: ownedBookId, title: ownedDetail?.title || detail?.title || selected?.title || 'Book' })
      return
    }
    setSending('reader')
    setNotice(null)
    try {
      const delivery = await deliverBook(ownedBookId, ownedFileId)
      setNotice(
        delivery.status === 'SENT'
          ? `Sent "${detail?.title ?? selected?.title ?? 'the book'}" to your reader.`
          : (delivery.errorMessage ?? 'The delivery failed'),
      )
      if (!detailOnly) setSelected(null)
    } catch (caught) {
      setNotice(caught instanceof ApiError ? caught.message : 'Could not send this book')
    } finally {
      setSending(null)
    }
  }

  async function tryDemoGet(bookId: number, providerKey?: string, sendWhenReady = false) {
    if (providerKey) setShelfBusy(providerKey)
    setExternalNotice(null)
    try {
      await startDemoGet(bookId, sendWhenReady)
      navigate('/activity')
    } catch (caught) {
      setExternalNotice(caught instanceof ApiError ? caught.message : 'Could not start the demo Get')
    } finally {
      setShelfBusy(null)
    }
  }

  async function add(
    item: DiscoveryResult,
    options: { preferredFormat?: string; sendToReader?: boolean } = {},
    action: 'quick' | 'library' | 'reader' = 'quick',
  ) {
    if (adding === item.providerKey || sending !== null || item.status === 'DOWNLOADING') return
    if (action === 'quick') {
      if (!demo && user?.canAcquire !== false && user?.acquisitionMode === 'ask') openDetail(item)
      setAdding(item.providerKey)
    } else {
      setSending(action)
    }
    setNotice(null)

    try {
      if (!demo && user?.canAcquire === false) {
        const requested = await createBookRequest(item.provider, item.providerKey)
        setNotice(requested.duplicate
          ? `You already asked for "${item.title}".`
          : `Asked for "${item.title}" — an administrator can approve it.`)
        if (!detailOnly) setSelected(null)
        return
      }
      // A book Bokhylle already knows is acquired by id, so no provider
      // resolution is needed (including local-only entries).
      let status: AcquisitionStatus
      let duplicate = false
      const title = item.title.trim() || detail?.title.trim() || ownedDetail?.title.trim()
      const bookLabel = title ? `"${title}"` : 'the book'
      let ownedBookId = item.ownedBookId
      let acquisitionId: string
      if (item.ownedBookId) {
        const created = await createAcquisitionForBook(item.ownedBookId, options)
        status = created.status
        duplicate = created.duplicate
        acquisitionId = created.id
      } else {
        const created = await createAcquisitionFromDiscovery(item.provider, item.providerKey, options)
        status = created.status
        duplicate = created.duplicate
        ownedBookId = created.bookId
        acquisitionId = created.id
      }
      patchResult(item.provider, item.providerKey, {
        status: discoveryStatusFromAcquisition(status),
        ownedBookId,
      })
      setNotice(
        duplicate
          ? `${bookLabel} is already on its way — follow its progress in Activity.`
          : options.sendToReader
            ? `On its way — ${bookLabel} will be emailed to your reader when it is ready.`
            : `Getting ${bookLabel} — it will appear on your shelf.`,
      )
      if (user?.acquisitionMode === 'ask') {
        setChoosingId(acquisitionId)
        return
      }
      if (!detailOnly) setSelected(null)
    } catch (caught) {
      setNotice(caught instanceof ApiError ? caught.message : 'Could not add this book')
    } finally {
      setAdding(null)
      setSending(null)
    }
  }

  const items = result?.items ?? []
  const fresh = result?.key === requestedKey
  const error = fresh ? result.error : null
  const searched = requestedKey !== null && !loading
  const revalidating = loading && result?.key !== requestedKey && items.length > 0
  const effectiveLanguages = allLanguages ? [] : defaultLanguages
  const languageActive = effectiveLanguages.length > 0
  const visibleItems = languageActive
    ? items.filter(
        (item) =>
          item.status === 'IN_LIBRARY' ||
          item.languages?.some((language) => effectiveLanguages.includes(language)) ||
          (item.language !== null && effectiveLanguages.includes(item.language)),
      )
    : items
  const authorHits =
    authorState && (authorState.key === requestedKey || revalidating)
      ? authorState.items
      : []
  const visibleAuthors = authorHits
  // The reserved strip stays until the full answer settles, even when the
  // local fast path already returned zero authors.
  const authorsPending =
    (type === 'any' || type === 'author') &&
    requestedKey !== null &&
    !(revalidating && authorState !== null) &&
    (authorState?.key !== requestedKey || authorState.partial)
  const selectedStatus = detail?.status ?? selected?.status ?? null
  const ownedBookId = detail?.ownedBookId ?? selected?.ownedBookId ?? null
  const ownedFileId = detail?.ownedFileId ?? selected?.ownedFileId ?? null
  const onShelf = ownedDetail?.onShelf ?? detail?.onShelf ?? selected?.onShelf ?? false
  const selectedCover = ownedBookId ? coverUrl(ownedBookId)
    : (detail?.coverId ?? selected?.coverId) ? discoverCoverUrl((detail?.coverId ?? selected?.coverId)!, detail?.title || selected?.title || '', selected?.provider)
      : null
  const liked =
    ownedDetail?.preference === 'liked' ||
    detail?.liked === true ||
    externalLikedId !== null

  async function toggleLiked() {
    const next = !liked
    try {
      if (next && !ownedBookId) {
        if (!selected) {
          return
        }
        const created = await likeExternalBook(selected.providerKey, selected.provider)
        setExternalLikedId(created.bookId)
        setNotice(`Liked "${selected.title}".`)
        return
      }
      const targetId = ownedBookId ?? externalLikedId
      if (!targetId) {
        return
      }
      await setBookPreference(targetId, next ? 'liked' : null)
      if (!next && externalLikedId !== null) {
        setExternalLikedId(null)
      }
      setDetail((current) => (current ? { ...current, liked: next } : current))
      setOwnedDetail((current) =>
        current ? { ...current, preference: next ? 'liked' : null } : current,
      )
    } catch (caught) {
      setNotice(caught instanceof ApiError ? caught.message : 'Could not update that like')
    }
  }

  const likeButton = ownedBookId || externalLikedId !== null || (selected && selectedStatus === 'NOT_IN_LIBRARY') ? (
    <Button variant="secondary" className="order-last border border-ink/20 sm:order-first sm:mr-auto" aria-pressed={liked} onClick={() => void toggleLiked()}>
      <Heart size={15} fill={liked ? 'currentColor' : 'none'} aria-hidden />
      {liked ? 'Liked' : 'Like'}
    </Button>
  ) : null

  async function addOwnedToShelf() {
    if (!ownedBookId) {
      return
    }
    try {
      await addBookToShelf(ownedBookId)
      setOwnedDetail((current) => (current ? { ...current, onShelf: true } : current))
      if (selected) {
        patchResult(selected.provider, selected.providerKey, { onShelf: true })
      }
    } catch (caught) {
      setNotice(caught instanceof ApiError ? caught.message : 'Could not add it to your shelf')
    }
  }

  /// Card-level "Add to my shelf": busy, readable failure, and a patch that
  /// keeps the visible card, the sheet and the cache consistent.
  async function addResultToShelf(item: DiscoveryResult) {
    if (!item.ownedBookId) {
      return
    }
    setShelfBusy(item.providerKey)
    setNotice(null)
    try {
      await addBookToShelf(item.ownedBookId)
      patchResult(item.provider, item.providerKey, { onShelf: true })
    } catch (caught) {
      setNotice(caught instanceof ApiError ? caught.message : 'Could not add it to your shelf')
    } finally {
      setShelfBusy(null)
    }
  }

  const authorResults = (visibleAuthors.length > 0 || (authorsPending && items.length > 0)) && (
    <div role="region" aria-label="Author results" className="mt-8">
      <p className="font-sans text-[11px] uppercase tracking-[0.18em] text-ink-faint">
        Authors
      </p>
      {visibleAuthors.length > 0 ? (
        <div className="mt-4 flex flex-wrap gap-x-5 gap-y-6">
          {visibleAuthors.slice(0, type === 'author' ? 6 : 3).map((hit) => (
            <div key={`${hit.name}|${hit.providerKey ?? ''}`} className="flex w-28 flex-col items-center text-center">
              {hit.authorId === null ? (
                <button type="button" onClick={() => void openAuthorHit(hit)} className="transition-transform hover:-translate-y-0.5">
                  <AuthorAvatar name={hit.name} provider={hit.provider} providerKey={hit.providerKey} className="h-16 w-16 text-xl" />
                </button>
              ) : (
                <Link to={`/authors/${hit.authorId}`} state={{ authorReturnTo: `${location.pathname}${location.search}${location.hash}` }} className="transition-transform hover:-translate-y-0.5">
                  <AuthorAvatar authorId={hit.authorId} name={hit.name} className="h-16 w-16 text-xl" />
                </Link>
              )}
              {hit.authorId === null ? (
                <button type="button" onClick={() => void openAuthorHit(hit)} className="mt-3 line-clamp-2 min-h-[2.5rem] text-sm font-medium text-ink transition-colors hover:text-accent">
                  {hit.name}
                </button>
              ) : (
                <Link to={`/authors/${hit.authorId}`} state={{ authorReturnTo: `${location.pathname}${location.search}${location.hash}` }} className="mt-3 line-clamp-2 min-h-[2.5rem] text-sm font-medium text-ink transition-colors hover:text-accent">
                  {hit.name}
                </Link>
              )}
              <span className="mt-0.5 block min-h-[1rem] text-xs text-ink-faint">
                {hit.bookCount > 0 ? `${hit.bookCount} in your library` : 'Not in your library'}
              </span>
              <Button variant={hit.following ? 'secondary' : 'ghost'} size="sm" className="mt-2" onClick={() => void toggleAuthorHit(hit)}>
                {hit.following ? 'Following' : 'Follow'}
              </Button>
            </div>
          ))}
        </div>
      ) : (
        <p role="status" className="mt-4 text-sm text-ink-muted">Looking for authors…</p>
      )}
    </div>
  )

  return (
    <section>
      {!detailOnly && <>
      <PageHeader
        eyebrow="Discover"
        title="Find something worth reading"
        description={demo
          ? 'Search the prepared sample library. Try Get to move a book to your shelf; no outside download occurs.'
          : 'Search a world of books and add the ones you want. Bokhylle fetches them and files them on your shelf.'}
        actions={!demo ? <Link to="/catalogues" className="inline-flex items-center gap-1 text-sm text-accent hover:text-accent-strong">Browse catalogues <ArrowRight size={15} /></Link> : undefined}
      />
      {demo && <p className="mt-4 text-xs text-ink-muted">Good titles to try: <button type="button" className="text-accent hover:underline" onClick={() => { setInput('Little Women'); void runSearch('Little Women', 'title') }}>Little Women</button> · <button type="button" className="text-accent hover:underline" onClick={() => { setInput('Black Beauty'); void runSearch('Black Beauty', 'title') }}>Black Beauty</button> · <button type="button" className="text-accent hover:underline" onClick={() => { setInput('The Invisible Man'); void runSearch('The Invisible Man', 'title') }}>The Invisible Man</button></p>}

      <form onSubmit={(event) => void handleSubmit(event)} className="mt-8">
        <SearchField
          value={query}
          onChange={setInput}
          placeholder={activeType.hint}
          ariaLabel="Search books"
          action={
            <Button
              type="submit"
              variant="primary"
              size="sm"
              aria-label="Search books"
              disabled={loading || !query.trim()}
            >
              {loading ? (
                <>
                  <Loader2 size={15} className="animate-spin" aria-hidden />
                  <span className="hidden sm:inline">Searching</span>
                </>
              ) : (
                <>
                  <Search size={15} className="sm:hidden" aria-hidden />
                  <span className="hidden sm:inline">Search</span>
                </>
              )}
            </Button>
          }
        />

        <div className="mt-3 flex items-center gap-2">
          <button
            type="button"
            onClick={() => setAdvanced((current) => !current)}
            aria-expanded={advanced}
            className="inline-flex items-center gap-1.5 text-xs font-medium text-ink-muted transition-colors hover:text-ink"
          >
            <SlidersHorizontal size={13} aria-hidden />
            {type === 'any' ? 'Advanced' : activeType.label}
            <ChevronDown
              size={13}
              aria-hidden
              className={`transition-transform duration-150 ${advanced ? 'rotate-180' : ''}`}
            />
          </button>

          {defaultLanguages.length > 0 && (
            <div className="ml-auto flex items-center gap-3">
              <span className="text-xs text-ink-faint">
                {allLanguages
                  ? 'All languages'
                  : `Preferred · ${defaultLanguages.map((value) => value.toUpperCase()).join('/')}`}
              </span>
              <button
                type="button"
                aria-pressed={allLanguages}
                onClick={() => updateAllLanguages(!allLanguages)}
                className="text-xs font-medium text-ink-muted transition-colors hover:text-ink"
              >
                {allLanguages ? 'Preferred only' : 'Search all languages'}
              </button>
            </div>
          )}
        </div>

        {advanced && (
          <div className="mt-4 flex flex-wrap gap-2">
            {searchTypes.map((item) => {
              const selected = item.value === type
              return (
                <button
                  key={item.value}
                  type="button"
                  aria-pressed={selected}
                  onClick={() => setType(item.value)}
                  className={`rounded-full px-3.5 py-1.5 text-xs font-medium transition-colors ${
                    selected
                      ? 'bg-accent text-accent-ink'
                      : 'bg-surface-2 text-ink-soft hover:bg-surface-3 hover:text-ink'
                  }`}
                >
                  {item.label}
                </button>
              )
            })}
          </div>
        )}
      </form>

      <p className="sr-only" role="status">
        {loading
          ? 'Searching…'
          : searched
            ? `${visibleItems.length} results${languageActive ? ` in ${effectiveLanguages.map((value) => value.toUpperCase()).join('/')}` : ''}`
            : ''}
      </p>

      {notice && (
        <p className="mt-6 flex items-center gap-2 rounded-card bg-surface px-4 py-3 text-sm text-ink-soft">
          <Check size={15} className="shrink-0 text-success" aria-hidden />
          {notice}
        </p>
      )}

      {error && <p className="mt-6 rounded-card bg-surface px-4 py-3 text-sm text-danger">{error}</p>}

      {loading && items.length === 0 && (
        <p role="status" className="mt-8 text-sm text-ink-muted">Searching books and authors…</p>
      )}

      {revalidating && (
        <p className="mt-6 text-xs text-ink-faint">
          Searching…
        </p>
      )}

      {!loading && searched && items.length === 0 && visibleAuthors.length === 0 && !error && (
        <EmptyState
          className="mt-10"
          title={type === 'author' ? 'No authors found' : 'No books found'}
          message="Try a different spelling, or switch the search scope under Advanced."
        />
      )}

      {!loading && searched && items.length > 0 && visibleItems.length === 0 && !error && (
        <div className="mt-10 flex flex-col items-start gap-3 border-l-2 border-line pl-5">
          <p className="text-sm text-ink-soft">
            Nothing in {effectiveLanguages.map((value) => value.toUpperCase()).join('/')} for this
            search.
          </p>
          <Button variant="ghost" size="sm" onClick={() => updateAllLanguages(true)}>
            Show all languages
          </Button>
        </div>
      )}

      {!loading && requestedKey === null && (
        <EmptyState
          className="mt-10"
          title="What do you feel like reading?"
          message="Search by title, author or ISBN to see books you can add to your shelf."
        />
      )}

      {externalNotice && (
        <p
          role="status"
          className="mt-6 border-l-2 border-warning pl-4 text-sm text-ink-soft"
        >
          {externalNotice}
        </p>
      )}

      {visibleItems.length > 0 && (
        <div className={revalidating ? 'mt-4' : 'mt-10'}>
          <SectionMark
            number={String(visibleItems.length).padStart(2, '0')}
            title="Results"
            action={
              languageActive && visibleItems.length !== items.length ? (
                <span className="text-xs text-ink-faint">
                  of {items.length}
                </span>
              ) : undefined
            }
          />
        </div>
      )}

      {visibleItems.length > 0 && (
        <ShelfGrid className={`mt-7 ${revalidating ? 'opacity-60 transition-opacity' : ''}`}>
          {visibleItems.map((item) => (
            <DiscoveryCard
              key={item.providerKey}
              result={item}
              preferredLanguages={effectiveLanguages}
              status={item.status}
              adding={adding === item.providerKey}
              shelfBusy={shelfBusy === item.providerKey}
              onAdd={() => void add(item)}
              canAcquire={user?.canAcquire !== false}
              demo={demo === true}
              onAddToShelf={() => item.ownedBookId && demo
                ? void tryDemoGet(item.ownedBookId, item.providerKey)
                : void addResultToShelf(item)}
              onOpen={() => openDetail(item)}
              onAuthor={(author) => {
                setInput(author)
                void runSearch(author, 'author')
              }}
            />
          ))}
        </ShelfGrid>
      )}
      {continuation && !revalidating && visibleItems.length > 0 && (
        <div className="mt-8 flex justify-center">
          <Button
            variant="secondary"
            size="sm"
            disabled={loadingMore}
            onClick={() => void loadMore()}
          >
            {loadingMore ? 'Loading…' : 'Load more results'}
          </Button>
        </div>
      )}
      {authorResults}
      </>}

      {selected && !demoSendBook && (
        <Modal
          title={ownedDetail?.title || detail?.title || selected.title || 'Loading book…'}
          description={
            detail
              ? [detail.year ?? selected.year, shownLanguage({ ...selected, language: detail.language, languages: detail.languages }, effectiveLanguages)?.toUpperCase()].filter(Boolean).join(' · ')
              : undefined
          }
          onClose={closeDetail}
          wide
          headerAside={!choosingId && !demo && user?.profileType !== 'child' && selectedStatus === 'NOT_IN_LIBRARY' ? (
            <BookSharingMarker value={user?.defaultBookSharing ?? 'shared'}
              label={`Default: ${user?.defaultBookSharing === 'private' ? 'private' : 'shared with household'}`} />
          ) : undefined}
          footer={
            <div role="group" aria-label="Book choices" className="grid w-full grid-cols-2 gap-2 [&>button]:h-auto [&>button]:min-h-11 [&>button]:min-w-0 [&>button]:px-3 [&>button]:py-2.5 [&>a]:h-auto [&>a]:min-h-11 [&>a]:min-w-0 [&>a]:px-3 [&>a]:py-2.5 [&_svg]:shrink-0 sm:flex sm:flex-wrap sm:items-center sm:justify-end">
            {
            choosingId ? (
              <Button variant="ghost" disabled={choiceBusy} onClick={closeDetail}>Close</Button>
            ) : selectedStatus === 'NOT_IN_LIBRARY' && demo ? (
              <>
                {likeButton}
                <Button variant="ghost" onClick={closeDetail}>Close</Button>
              </>
            ) : selectedStatus === 'NOT_IN_LIBRARY' ? (
              <>
                {likeButton}
                <Button
                  variant={hasReader ? 'secondary' : 'primary'}
                  className={hasReader ? 'border border-ink/20' : 'order-first col-span-2 sm:order-none'}
                  disabled={detailLoading || sending !== null}
                  onClick={() => void add(selected, { preferredFormat }, 'library')}
                >
                  {sending === 'library' ? user?.canAcquire === false ? 'Asking…' : 'Getting…' : user?.canAcquire === false ? 'Ask to add' : 'Get for my shelf'}
                </Button>
                {hasReader && user?.canAcquire !== false && (
                  <Button
                    variant="primary"
                    className="order-first col-span-2 sm:order-none"
                    disabled={detailLoading || sending !== null}
                    onClick={() =>
                      void add(
                        selected,
                        {
                          preferredFormat: preferredFormat === 'any' ? undefined : preferredFormat,
                          sendToReader: true,
                        },
                        'reader',
                      )
                    }
                  >
                    {sending === 'reader' ? (
                      <Loader2 size={14} className="animate-spin" aria-hidden />
                    ) : (
                      <Send size={14} aria-hidden />
                    )}
                    Get &amp; Send to My Reader
                  </Button>
                )}
              </>
            ) : selectedStatus === 'IN_LIBRARY' && ownedBookId ? (
              <>
                {likeButton}
                {(!demo || onShelf) && <ButtonLink
                  to={`/library/${ownedBookId}`}
                  variant={onShelf && !hasReader && !demo ? 'primary' : 'secondary'}
                  className={onShelf && !hasReader && !demo ? 'order-first col-span-2 sm:order-none' : 'border border-ink/20'}
                >
                  View book
                </ButtonLink>}
                {onShelf && demo ? (
                  <Button variant="primary" className="order-first col-span-2 sm:order-none" disabled={sending !== null} onClick={() => void sendOwned()}>
                    <Send size={14} aria-hidden /> Send to Demo Kindle
                  </Button>
                ) : onShelf && hasReader ? (
                  <Button
                    variant="primary"
                    className="order-first col-span-2 sm:order-none"
                    disabled={!ownedFileId || sending !== null}
                    onClick={() => void sendOwned()}
                  >
                    {sending === 'reader' ? (
                      <Loader2 size={14} className="animate-spin" aria-hidden />
                    ) : (
                      <Send size={14} aria-hidden />
                    )}
                    Send to My Reader
                  </Button>
                ) : !onShelf ? (
                  <>
                    <Button variant={demo ? 'secondary' : 'primary'} className={demo ? 'border border-ink/20' : 'order-first col-span-2 sm:order-none'} disabled={detailLoading || shelfBusy !== null} onClick={() => demo ? void tryDemoGet(ownedBookId, selected.providerKey) : void addOwnedToShelf()}>
                      {demo ? 'Get for my shelf' : 'Add to my shelf'}
                    </Button>
                    {demo && <Button variant="primary" className="order-first col-span-2 sm:order-none" disabled={detailLoading || shelfBusy !== null} onClick={() => void tryDemoGet(ownedBookId, selected.providerKey, true)}>
                      <Send size={14} aria-hidden /> Get &amp; Send to Kindle
                    </Button>}
                  </>
                ) : null}
              </>
            ) : demo && selectedStatus === 'DOWNLOADING' ? (
              <ButtonLink to="/activity" variant="primary" className="order-first col-span-2 sm:order-none">Follow in Activity</ButtonLink>
            ) : (
              <Button variant="ghost" onClick={closeDetail}>
                Close
              </Button>
            )}
            </div>
          }
        >
          {choosingId ? <ReleaseChoices key={choosingId} acquisitionId={choosingId} onBusyChange={setChoiceBusy} onSelected={() => { setSelected(null); setChoosingId(null); navigate('/activity') }} /> : <>
          {detailOnly && notice && (
            <p role="status" className="mb-5 border-l-2 border-success pl-4 text-sm text-ink-soft">{notice}</p>
          )}
          {demo && selectedStatus === 'NOT_IN_LIBRARY' && (
            <p className="mb-5 border-l-2 border-accent pl-4 text-sm text-ink-soft">This title is outside the prepared sample library. Try Get on one of the sample books to follow the demo activity.</p>
          )}
          <div className="flex items-start gap-5">
            <div className="w-28 shrink-0 self-start overflow-hidden rounded-[3px] bg-surface-2 shadow-card sm:w-32">
              {selectedCover ? (
                <BookCover
                  src={selectedCover}
                  loading="eager"
                  className="aspect-[2/3] w-full"
                />
              ) : (
                <div className="flex aspect-[2/3] items-center justify-center p-3 text-center font-display text-sm text-ink-faint">
                  {selected.title}
                </div>
              )}
            </div>

            <div className="min-w-0 flex-1">
              {(() => {
                const authors = detail?.authors ?? ownedDetail?.authors ?? selected.authors
                if (authors.length === 0) {
                  return null
                }
                return (
                <p className="mb-3 flex flex-wrap gap-x-2 gap-y-1 text-sm">
                  {authors.map((author) => (
                    <button
                      key={author}
                      type="button"
                      onClick={() => {
                        searchAuthor(author)
                      }}
                      className="text-accent transition-colors hover:text-accent-strong"
                    >
                      {author}
                    </button>
                  ))}
                </p>
                )
              })()}

              {detailLoading && (
                <div className="animate-pulse space-y-2">
                  <div className="h-3 w-3/4 rounded bg-surface-2" />
                  <div className="h-3 w-1/2 rounded bg-surface-2" />
                  <div className="h-3 w-2/3 rounded bg-surface-2" />
                </div>
              )}

              {detailError && (
                <p className="border-l-2 border-danger pl-3 text-sm text-danger">{detailError}</p>
              )}

              <dl className="mt-2">
                {detail?.publisher && <DetailRow label="Publisher" value={detail.publisher} />}
                {detail?.series && (
                  <DetailRow
                    label="Series"
                    value={`${detail.series}${detail.seriesNumber ? ` · Book ${detail.seriesNumber}` : ''}`}
                  />
                )}
                {detail?.isbn13 && <DetailRow label="ISBN" value={detail.isbn13} />}
                {detail?.status === 'IN_LIBRARY' && (
                  <DetailRow label="Status" value="Already in your library" />
                )}
                {detail?.status === 'DOWNLOADING' && (
                  <DetailRow label="Status" value="Already on its way" />
                )}
              </dl>

              {selectedStatus === 'NOT_IN_LIBRARY' && (
                <div className="mt-4">
                  {!formatOpen ? (
                    <div className="flex items-center justify-between gap-3">
                      <div className="flex items-baseline gap-2">
                        <p className="font-sans text-[11px] uppercase tracking-[0.16em] text-ink-faint">
                          Format
                        </p>
                        <p className="font-sans text-[11px] uppercase tracking-[0.16em] text-ink">
                          {preferredFormat === 'epub' ? 'EPUB' : 'Any compatible'}
                        </p>
                      </div>
                      <button
                        type="button"
                        onClick={() => setFormatOpen(true)}
                        className="font-sans text-[11px] font-medium uppercase tracking-[0.16em] text-accent transition-colors hover:text-accent-strong"
                      >
                        Change
                      </button>
                    </div>
                  ) : (
                    <>
                      <p className="font-sans text-[11px] uppercase tracking-[0.16em] text-ink-faint">
                        Format
                      </p>
                      <div className="mt-2 flex gap-2">
                        {(
                          [
                            ['epub', 'EPUB'],
                            ['any', 'Any compatible'],
                          ] as const
                        ).map(([value, label]) => (
                          <button
                            key={value}
                            type="button"
                            aria-pressed={preferredFormat === value}
                            onClick={() => {
                              setPreferredFormat(value)
                              setFormatOpen(false)
                            }}
                            className={`rounded-[3px] px-3.5 py-1.5 font-sans text-[11px] font-medium uppercase tracking-[0.16em] transition-colors ${
                              preferredFormat === value
                                ? 'bg-accent text-accent-ink'
                                : 'bg-surface-2 text-ink-soft hover:bg-surface-3 hover:text-ink'
                            }`}
                          >
                            {label}
                          </button>
                        ))}
                      </div>
                      <p className="mt-2 text-xs text-ink-faint">
                        Any compatible tries EPUB first and only falls back to PDF when
                        nothing good is found.
                      </p>
                    </>
                  )}
                </div>
              )}

              {!demo && hasReader === false && (selectedStatus === 'NOT_IN_LIBRARY' || selectedStatus === 'IN_LIBRARY') && (
                <p className="mt-4 text-xs text-ink-faint">
                  To send books to a device,{' '}
                  <Link to="/profile/readers" className="text-accent hover:text-accent-strong">
                    add a reader
                  </Link>
                  .
                </p>
              )}
              {householdReader && selectedStatus === 'NOT_IN_LIBRARY' && (
                <p className="mt-4 text-xs text-ink-faint">
                  Will be sent to the household reader ({householdReader}).
                </p>
              )}
            </div>
          </div>

          {(detail?.description ?? ownedDetail?.description) && (
            <p className="mt-5 max-h-52 overflow-y-auto whitespace-pre-line text-sm leading-relaxed text-ink-soft">
              {descriptionText(detail?.description ?? ownedDetail?.description ?? '')}
            </p>
          )}
          </>}
        </Modal>
      )}
      {demoSendBook && <DemoSendDialog bookId={demoSendBook.id} title={demoSendBook.title} onClose={() => setDemoSendBook(null)} />}
    </section>
  )
}

function DetailRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="grid gap-1 border-b border-line py-2 sm:grid-cols-[9rem_1fr] sm:gap-2">
      <dt className="font-sans text-[11px] uppercase tracking-[0.16em] text-ink-faint">
        {label}
      </dt>
      <dd className="min-w-0 break-words text-sm text-ink-soft">{value}</dd>
    </div>
  )
}

function DiscoveryCard({
  result,
  preferredLanguages,
  status,
  adding,
  shelfBusy,
  onAdd,
  onAddToShelf,
  onOpen,
  onAuthor,
  demo,
  canAcquire,
}: {
  result: DiscoveryResult
  preferredLanguages: string[]
  status: DiscoveryStatus
  adding: boolean
  shelfBusy: boolean
  onAdd: () => void
  onAddToShelf: () => void
  onOpen: () => void
  onAuthor: (author: string) => void
  demo: boolean
  canAcquire: boolean
}) {
  return (
    <article className="shelf-book group">
      {/* Only this region opens the details; the author and action controls
          below stay independent interactive elements. */}
      <button
        type="button"
        onClick={onOpen}
        aria-label={`Open details for ${result.title}`}
        className="block w-full rounded-[3px] text-left focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
      >
        <ShelfCover
          cover={result.ownedBookId ? coverUrl(result.ownedBookId) : result.coverId ? discoverCoverUrl(result.coverId, result.title, result.provider) : null}
          fallback={<span className="line-clamp-5 px-3 text-center font-display text-sm leading-snug text-ink-faint">{result.title}</span>}
        />
        <div className="shelf-book-info">
          <h3 className="line-clamp-2 font-display text-base leading-snug text-ink">
            {result.title}
          </h3>
        </div>
      </button>
      <p className="mt-1 line-clamp-1 text-xs text-ink-muted">
        {result.authors.length > 0 ? (
          result.authors.map((author, index) => (
            <span key={author}>
              {index > 0 && ', '}
              <button
                type="button"
                onClick={() => onAuthor(author)}
                className="transition-colors hover:text-accent"
              >
                {author}
              </button>
            </span>
          ))
        ) : (
          'Unknown author'
        )}
      </p>
      <MetaLine
        className="mt-1"
        items={[result.year ? String(result.year) : null, shownLanguage(result, preferredLanguages)?.toUpperCase()]}
      />
      {result.series && (
        <p className="mt-1 line-clamp-1 text-xs text-ink-faint">
          {result.series}
          {result.seriesNumber ? ` · Book ${result.seriesNumber}` : ''}
        </p>
      )}
      {status === 'IN_LIBRARY' && (
        <p className="mt-2 font-sans text-[10px] uppercase tracking-[0.16em] text-success">
          {result.onShelf ? 'On my shelf' : 'In library'}
        </p>
      )}
      {status === 'DOWNLOADING' && (
        <p className="mt-2 font-sans text-[10px] uppercase tracking-[0.16em] text-accent">On its way</p>
      )}
      {status === 'NOT_IN_LIBRARY' && !demo && (
        <button
          type="button"
          disabled={adding}
          onClick={onAdd}
          className="mt-2 inline-flex items-center gap-1 font-sans text-[11px] font-medium uppercase tracking-[0.18em] text-accent transition-colors hover:text-accent-strong disabled:opacity-50"
        >
          {adding ? canAcquire ? 'Getting…' : 'Asking…' : canAcquire ? 'Get' : 'Ask to add'}
          {!adding && <ArrowRight size={12} aria-hidden />}
        </button>
      )}
      {status === 'IN_LIBRARY' && !result.onShelf && result.ownedBookId && (
        <button
          type="button"
          disabled={shelfBusy}
          onClick={onAddToShelf}
          className="mt-2 inline-flex items-center gap-1 font-sans text-[11px] font-medium uppercase tracking-[0.18em] text-accent transition-colors hover:text-accent-strong disabled:opacity-50"
        >
          {shelfBusy ? 'Starting…' : demo ? 'Try Get' : 'Add to my shelf'}
        </button>
      )}
    </article>
  )
}

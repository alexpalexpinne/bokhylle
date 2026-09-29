import { useEffect, useState } from 'react'
import { Link, useSearchParams } from 'react-router-dom'
import { Search } from 'lucide-react'
import { ApiError } from '../api/client'
import { discoverCoverUrl } from '../api/discover'
import {
  type RequestableBook,
  type RequestBookDetail,
  createBookRequest,
  fetchRequestBook,
  searchRequestCatalogue,
} from '../api/requests'
import { useAuth } from '../auth/useAuth'
import { BookCover } from '../components/BookCover'
import { Button } from '../components/ui/Button'
import { Modal } from '../components/ui/Modal'
import { PageHeader } from '../components/ui/PageHeader'
import { SectionMark } from '../components/ui/SectionMark'
import { descriptionText } from '../lib/descriptionText'

type SearchState = {
  key: string
  items: RequestableBook[]
  loading: boolean
  error: string | null
}

export function ChildDiscover() {
  const { user } = useAuth()
  const [params, setParams] = useSearchParams()
  const [query, setQuery] = useState(params.get('q') ?? '')
  const [search, setSearch] = useState<SearchState>({ key: '', items: [], loading: false, error: null })
  const [detailState, setDetailState] = useState<{ key: string; book: RequestBookDetail | null; error: string | null }>({ key: '', book: null, error: null })
  const [asking, setAsking] = useState(false)
  const [requested, setRequested] = useState<Record<string, boolean>>({})
  const [askError, setAskError] = useState<string | null>(null)
  const provider = params.get('provider')
  const providerKey = params.get('providerKey')
  const selectedKey = `${provider ?? ''}|${providerKey ?? ''}`
  const detail = detailState.key === selectedKey ? detailState.book : null
  const detailError = detailState.key === selectedKey ? detailState.error : null
  const trimmed = query.trim()
  const visibleResults = search.key === trimmed ? search.items : []
  const searchLoading = trimmed.length >= 2 && (search.key !== trimmed || search.loading)

  useEffect(() => {
    if (trimmed.length < 2) return
    let cancelled = false
    const timer = setTimeout(() => {
      searchRequestCatalogue(trimmed)
        .then((response) => {
          if (!cancelled) setSearch({ key: trimmed, items: response.items, loading: false, error: null })
        })
        .catch((error: unknown) => {
          if (!cancelled) setSearch({ key: trimmed, items: [], loading: false, error: error instanceof ApiError ? error.message : 'Search failed' })
        })
    }, 250)
    return () => { cancelled = true; clearTimeout(timer) }
  }, [trimmed])

  useEffect(() => {
    if (!provider || !providerKey) {
      return
    }
    let cancelled = false
    fetchRequestBook(provider, providerKey)
      .then((book) => { if (!cancelled) setDetailState({ key: selectedKey, book, error: null }) })
      .catch((error: unknown) => {
        if (!cancelled) setDetailState({ key: selectedKey, book: null, error: error instanceof ApiError ? error.message : 'Could not load this book' })
      })
    return () => { cancelled = true }
  }, [provider, providerKey, selectedKey])

  function openBook(bookProvider: string, key: string) {
    setAskError(null)
    setParams({ ...(trimmed ? { q: trimmed } : {}), provider: bookProvider, providerKey: key })
  }

  function closeBook() {
    setParams(trimmed ? { q: trimmed } : {})
  }

  async function ask() {
    if (!detail || asking || requested[selectedKey]) return
    setAsking(true)
    setAskError(null)
    try {
      await createBookRequest(detail.provider, detail.providerKey)
      setRequested((current) => ({ ...current, [selectedKey]: true }))
    } catch (error) {
      setAskError(error instanceof ApiError ? error.message : 'Could not ask for this book')
    } finally {
      setAsking(false)
    }
  }

  return (
    <section>
      <PageHeader
        eyebrow="Discover"
        title="Find your next book"
        description={user?.canRequest
          ? 'Explore books from the public catalogue. Ask an adult to send one to your reader.'
          : 'Explore books from the public catalogue. An adult can choose books for your reader.'}
      />
      <div className="mt-8 flex items-center gap-3 border-b border-line pb-2 focus-within:border-accent">
        <Search size={18} className="shrink-0 text-ink-faint" aria-hidden />
        <input
          value={query}
          onChange={(event) => {
            setQuery(event.target.value)
            setSearch({ key: '', items: [], loading: false, error: null })
          }}
          placeholder="Search by title, author or ISBN"
          aria-label="Search Discover"
          className="h-10 min-w-0 flex-1 bg-transparent font-display text-lg text-ink outline-none placeholder:text-ink-faint"
        />
      </div>

      {trimmed.length >= 2 ? (
        <section className="mt-10">
          <SectionMark title="Search results" />
          {searchLoading && <p role="status" className="mt-6 text-sm text-ink-muted">Searching…</p>}
          {search.error && <p role="alert" className="mt-6 text-sm text-danger">{search.error}</p>}
          {!searchLoading && !search.error && visibleResults.length === 0 && (
            <p className="mt-6 text-sm text-ink-muted">No books found for that search.</p>
          )}
          {visibleResults.length > 0 && (
            <ul className="mt-5 grid gap-x-6 gap-y-5 sm:grid-cols-2 lg:grid-cols-3">
              {visibleResults.map((book) => (
                <li key={`${book.provider}|${book.providerKey}`}>
                  <button type="button" onClick={() => openBook(book.provider, book.providerKey)} className="flex w-full items-start gap-4 border-b border-line pb-5 text-left hover:text-accent">
                    <span className="w-16 shrink-0 overflow-hidden rounded-[3px] bg-surface-2">
                      {book.coverId ? <BookCover src={discoverCoverUrl(book.coverId, book.title, book.provider)} className="aspect-[2/3] w-full" /> : <span className="block aspect-[2/3]" />}
                    </span>
                    <span className="min-w-0 pt-1">
                      <span className="line-clamp-2 font-display text-lg text-ink">{book.title}</span>
                      <span className="mt-1 block line-clamp-1 text-xs text-ink-muted">{book.authors.join(', ') || 'Unknown author'}</span>
                      {book.year && <span className="mt-2 block text-xs text-ink-faint">{book.year}</span>}
                      {requested[`${book.provider}|${book.providerKey}`] && <span className="mt-2 block text-xs font-medium text-accent">Requested · waiting for an adult</span>}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>
      ) : <p className="mt-10 text-sm text-ink-muted">Search for a book or author to start exploring.</p>}

      <p className="mt-10 text-sm text-ink-muted">
        {user?.canRequest
          ? <>Already asked for a book? <Link to="/requests" className="text-accent hover:text-accent-strong">See your requests</Link>.</>
          : 'An adult can turn on book requests for this profile.'}
      </p>

      {provider && providerKey && (
        <Modal title={detail?.title ?? 'Book details'} onClose={closeBook} footer={
          <>
            <Button variant="ghost" onClick={closeBook}>Close</Button>
            {user?.canRequest && <Button variant={requested[selectedKey] ? 'secondary' : 'primary'} disabled={!detail || asking || requested[selectedKey]} onClick={() => void ask()}>{requested[selectedKey] ? 'Requested' : asking ? 'Asking…' : 'Ask an adult'}</Button>}
          </>
        }>
          {detailError && <p role="alert" className="text-sm text-danger">{detailError}</p>}
          {!detail && !detailError && <p role="status" className="text-sm text-ink-muted">Loading book…</p>}
          {requested[selectedKey] && <p role="status" className="mb-5 border-l-2 border-accent pl-3 text-sm text-ink-soft">Waiting for an adult to approve. <Link to="/requests" className="text-accent hover:text-accent-strong">See your requests</Link>.</p>}
          {askError && <p role="alert" className="mb-5 border-l-2 border-danger pl-3 text-sm text-danger">{askError}</p>}
          {detail && (
            <div className="flex items-start gap-5">
              <div className="w-24 shrink-0 overflow-hidden rounded-[3px] bg-surface-2 sm:w-28">
                {detail.coverId ? <BookCover src={discoverCoverUrl(detail.coverId, detail.title, detail.provider)} className="aspect-[2/3] w-full" /> : <span className="block aspect-[2/3]" />}
              </div>
              <div className="min-w-0 flex-1">
                <p className="text-sm text-ink-soft">{detail.authors.join(', ') || 'Unknown author'}</p>
                <p className="mt-2 text-xs text-ink-muted">{[detail.year, detail.series ? `${detail.series}${detail.seriesNumber ? ` · ${detail.seriesNumber}` : ''}` : null, detail.language?.toUpperCase()].filter(Boolean).join(' · ')}</p>
                {detail.description && <p className="mt-5 whitespace-pre-line text-sm leading-relaxed text-ink-soft">{descriptionText(detail.description)}</p>}
                {!user?.canRequest && <p className="mt-5 text-sm text-ink-muted">Ask an adult to add this book to your shelf.</p>}
              </div>
            </div>
          )}
        </Modal>
      )}
    </section>
  )
}

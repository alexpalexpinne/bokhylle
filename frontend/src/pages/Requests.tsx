import { useEffect, useRef, useState } from 'react'
import { Search } from 'lucide-react'
import { ApiError } from '../api/client'
import { discoverCoverUrl } from '../api/discover'
import {
  type BookRequest,
  type RequestableBook,
  approveBookRequest,
  createBookRequest,
  declineBookRequest,
  fetchBookRequests,
  searchRequestCatalogue,
} from '../api/requests'
import { useAuth } from '../auth/useAuth'
import { BookCover } from '../components/BookCover'
import { Button } from '../components/ui/Button'
import { MetaLine } from '../components/ui/MetaLine'
import { PageHeader } from '../components/ui/PageHeader'
import { SectionMark } from '../components/ui/SectionMark'

const PHASE_COPY: Record<BookRequest['phase'], string> = {
  requested: 'Waiting for an administrator to approve.',
  looking: 'Approved — Bokhylle is looking for a copy.',
  getting: 'Approved — getting the book.',
  ready: 'Ready — it is on your shelf.',
  declined: 'Declined.',
  unavailable: 'Approved — no suitable copy yet.',
}

function authorList(authors: string[]): string {
  return authors.length > 0 ? authors.join(', ') : 'Unknown author'
}

function requestPhaseCopy(request: BookRequest, isChild: boolean): string {
  if (request.phase !== 'ready') return PHASE_COPY[request.phase]
  if (request.deliveryStatus === 'SENT') return 'Sent to your reader — also on your shelf.'
  if (request.deliveryStatus === 'FAILED') return 'Reader delivery failed — ask an adult to retry it.'
  return isChild
    ? 'Ready in the library — ask an adult to check reader delivery.'
    : PHASE_COPY.ready
}

export function Requests() {
  const { user } = useAuth()
  const isChild = user?.profileType === 'child'
  const isAdmin = user?.role === 'admin'
  const canSearch = !isChild || user?.canRequest !== false
  const canAsk = !isAdmin && canSearch
  const [query, setQuery] = useState('')
  // Search state is keyed by the query it belongs to, so a failure can never
  // leave the previous query's results actionable under the new one.
  const [search, setSearch] = useState<{
    queryKey: string
    items: RequestableBook[]
    loading: boolean
    error: string | null
  }>({ queryKey: '', items: [], loading: false, error: null })
  const [requestsState, setRequestsState] = useState<{
    items: BookRequest[]
    loading: boolean
    error: string | null
  }>({ items: [], loading: true, error: null })
  const [adding, setAdding] = useState<string | null>(null)
  const [requested, setRequested] = useState<Record<string, boolean>>({})
  const [busy, setBusy] = useState<number | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const searchSeq = useRef(0)
  const loadSeq = useRef(0)

  function load() {
    const generation = ++loadSeq.current
    setRequestsState((current) => ({ ...current, loading: true, error: null }))
    fetchBookRequests()
      .then((data) => {
        if (generation === loadSeq.current) {
          setRequestsState({ items: data.items, loading: false, error: null })
        }
      })
      .catch((caught: unknown) => {
        if (generation === loadSeq.current) {
          setRequestsState({
            items: [],
            loading: false,
            error: caught instanceof ApiError ? caught.message : 'Could not load your requests',
          })
        }
      })
  }

  useEffect(() => {
    load()
  }, [])

  useEffect(() => {
    const trimmed = query.trim()
    if (trimmed.length < 2) {
      setSearch({ queryKey: trimmed, items: [], loading: false, error: null })
      return
    }
    const generation = ++searchSeq.current
    setSearch((current) => ({
      queryKey: trimmed,
      items: current.queryKey === trimmed ? current.items : [],
      loading: true,
      error: null,
    }))
    const timer = setTimeout(() => {
      searchRequestCatalogue(trimmed)
        .then((data) => {
          if (generation === searchSeq.current) {
            setSearch({ queryKey: trimmed, items: data.items, loading: false, error: null })
          }
        })
        .catch((caught: unknown) => {
          if (generation === searchSeq.current) {
            setSearch({
              queryKey: trimmed,
              items: [],
              loading: false,
              error: caught instanceof ApiError ? caught.message : 'Search failed',
            })
          }
        })
    }, 250)
    return () => clearTimeout(timer)
  }, [query])

  async function ask(book: RequestableBook) {
    const key = `${book.provider}|${book.providerKey}`
    if (requested[key]) return
    setAdding(key)
    setError(null)
    setNotice(null)
    try {
      const created = await createBookRequest(book.provider, book.providerKey)
      setNotice(
        created.duplicate
          ? `You already asked for “${book.title}”.`
          : `Asked for “${book.title}” — an administrator can approve it.`,
      )
      setRequested((current) => ({ ...current, [key]: true }))
      load()
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not ask for this book')
    } finally {
      setAdding(null)
    }
  }

  async function decide(request: BookRequest, approve: boolean) {
    setBusy(request.id)
    setError(null)
    try {
      if (approve) {
        const result = await approveBookRequest(request.id)
        setNotice(result.request.deliveryStatus === 'FAILED'
          ? `Approved “${request.title}”, but reader delivery failed. Check Delivery history in Settings.`
          : result.request.deliveryStatus === 'SENT'
            ? `Approved “${request.title}” and sent it to the reader.`
            : `Approved “${request.title}”.`)
      } else {
        await declineBookRequest(request.id)
        setNotice(`Declined “${request.title}”.`)
      }
      load()
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not update the request')
    } finally {
      setBusy(null)
    }
  }

  const currentKey = query.trim()
  const searchItems = search.queryKey === currentKey ? search.items : []
  const pending = requestsState.items.filter((request) => request.status === 'requested')
  const others = requestsState.items.filter((request) => request.status !== 'requested')

  return (
    <section>
      <PageHeader
        eyebrow="Requests"
        title={canAsk ? 'Ask for a book' : 'Requests'}
        description={
          isChild
            ? canSearch
              ? 'Search the catalogue and ask an administrator to send a book to your reader.'
              : 'Books an administrator approves for your reader will appear here.'
            : isAdmin
              ? 'Requests waiting for your decision.'
              : 'Ask an administrator to add a book to the shared library.'
        }
      />

      {error && <p className="mt-6 border-l-2 border-danger pl-4 text-sm text-danger">{error}</p>}
      {notice && (
        <p role="status" className="mt-6 rounded-card bg-surface px-4 py-3 text-sm text-ink-soft">
          {notice}
        </p>
      )}

      {canAsk && (
        <>
          <div className="mt-8 flex items-center gap-3 border-b border-line pb-2 transition-colors focus-within:border-accent">
            <Search size={16} className="shrink-0 text-ink-faint" aria-hidden />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search books by title, author or ISBN"
              aria-label="Search the request catalogue"
              className="h-9 min-w-0 flex-1 bg-transparent font-display text-base text-ink outline-none placeholder:text-ink-faint focus-visible:outline-none"
            />
          </div>

          {search.error && (
            <p className="mt-4 border-l-2 border-danger pl-4 text-sm text-danger">
              {search.error}
            </p>
          )}

          {search.loading && searchItems.length === 0 && (
            <p className="mt-6 text-sm text-ink-muted">Searching…</p>
          )}

          {searchItems.length > 0 && (
            <ul className="mt-6 divide-y divide-line">
              {searchItems.map((book) => (
                <li key={`${book.provider}|${book.providerKey}`} className="flex items-center gap-4 py-3">
                  <div className="w-10 shrink-0 overflow-hidden rounded-[3px] bg-surface-2">
                    {book.coverId ? (
                      <BookCover
                        src={discoverCoverUrl(book.coverId, book.title, book.provider)}
                        className="aspect-[2/3] w-full"
                      />
                    ) : (
                      <span className="block aspect-[2/3] w-full" />
                    )}
                  </div>
                  <div className="min-w-0 flex-1">
                    <p className="line-clamp-2 font-display text-base text-ink">{book.title}</p>
                    <p className="truncate text-xs text-ink-soft">{authorList(book.authors)}</p>
                    <MetaLine
                      className="mt-0.5"
                      items={[book.year ? String(book.year) : null, book.language?.toUpperCase()]}
                    />
                  </div>
                  <Button
                    variant="secondary"
                    size="sm"
                    disabled={adding === `${book.provider}|${book.providerKey}` || requested[`${book.provider}|${book.providerKey}`]}
                    onClick={() => void ask(book)}
                  >
                    {requested[`${book.provider}|${book.providerKey}`] ? 'Requested' : adding === `${book.provider}|${book.providerKey}` ? 'Asking…' : 'Ask an adult'}
                  </Button>
                </li>
              ))}
            </ul>
          )}

          {!search.loading && !search.error && currentKey.length >= 2 && searchItems.length === 0 && (
            <p className="mt-6 text-sm text-ink-muted">No books found for that search.</p>
          )}
        </>
      )}

      <div className="mt-12">
        <SectionMark title={isAdmin ? 'Waiting for approval' : 'Your requests'} />
        {requestsState.loading ? (
          <p className="mt-5 text-sm text-ink-muted">Loading requests…</p>
        ) : requestsState.error ? (
          <div className="mt-5 flex flex-col items-start gap-3 border-l-2 border-danger pl-4">
            <p className="text-sm text-danger">{requestsState.error}</p>
            <Button variant="ghost" size="sm" onClick={() => load()}>
              Try again
            </Button>
          </div>
        ) : pending.length === 0 ? (
          <p className="mt-5 border-l-2 border-line pl-4 text-sm text-ink-muted">
            {isChild
              ? canSearch
                ? 'You have not asked for anything yet.'
                : 'An administrator can approve books for your reader.'
              : 'You have not asked for anything yet.'}
          </p>
        ) : (
          <ul className="mt-2 divide-y divide-line">
            {pending.map((request) => (
              <li
                key={request.id}
                className="flex flex-wrap items-center justify-between gap-3 py-3.5"
              >
                <div className="min-w-0">
                  <p className="truncate font-display text-base text-ink">{request.title}</p>
                  <p className="truncate text-xs text-ink-soft">{authorList(request.authors)}</p>
                  <MetaLine
                    className="mt-1"
                    items={[isAdmin ? `requested by ${request.requester}` : null, requestPhaseCopy(request, isChild)]}
                  />
                </div>
                {isAdmin && (
                  <div className="flex shrink-0 items-center gap-2">
                    <Button
                      variant="primary"
                      size="sm"
                      disabled={busy === request.id}
                      onClick={() => void decide(request, true)}
                    >
                      Approve
                    </Button>
                    <Button
                      variant="secondary"
                      size="sm"
                      disabled={busy === request.id}
                      onClick={() => void decide(request, false)}
                    >
                      Decline
                    </Button>
                  </div>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>

      {others.length > 0 && (
        <div className="mt-12">
          <SectionMark title="Decided" />
          <ul className="mt-2 divide-y divide-line">
            {others.map((request) => (
              <li key={request.id} className="py-3">
                <p className="truncate text-sm text-ink-muted">{request.title}</p>
                <MetaLine
                  className="mt-0.5"
                  items={[isAdmin ? request.requester : null, requestPhaseCopy(request, isChild)]}
                />
              </li>
            ))}
          </ul>
        </div>
      )}
    </section>
  )
}

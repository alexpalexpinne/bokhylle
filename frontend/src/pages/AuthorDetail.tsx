import { useEffect, useState } from 'react'
import { ArrowLeft } from 'lucide-react'
import { Link, useLocation, useParams } from 'react-router-dom'
import { ApiError } from '../api/client'
import {
  type AuthorDetail as AuthorDetailData,
  type AuthorProfile,
  type CatalogueWork,
  fetchAuthorCatalogue,
  fetchAuthor,
  fetchAuthorProfile,
  fetchAuthorFollow,
  setAuthorAutomation,
  setAuthorFollow,
} from '../api/library'
import { type DeliveryTarget, fetchTargets } from '../api/delivery'
import { AuthorAvatar } from '../components/AuthorAvatar'
import { BookGrid } from '../components/BookGrid'
import { Button } from '../components/ui/Button'
import { Notice } from '../components/ui/Notice'
import { useAuth } from '../auth/useAuth'

export function AuthorDetail({ authorId }: { authorId: string }) {
  const id = Number(authorId)
  const location = useLocation()
  const candidate = (location.state as { authorReturnTo?: unknown } | null)?.authorReturnTo
  const returnTo = typeof candidate === 'string' && (
    candidate === '/' ||
    /^\/discover(?:[/?#]|$)/.test(candidate) ||
    /^\/library(?:[/?#]|$)/.test(candidate)
  ) ? candidate : '/library'
  const returnLabel = returnTo.startsWith('/discover')
    ? 'Discover'
    : returnTo === '/'
      ? 'Home'
      : /^\/library\/\d+/.test(returnTo)
        ? 'book'
        : 'library'
  const { user } = useAuth()
  const [allLanguages, setAllLanguages] = useState(false)
  const [author, setAuthor] = useState<AuthorDetailData | null>(null)
  const [profile, setProfile] = useState<AuthorProfile | null>(null)
  const [following, setFollowing] = useState<boolean | null>(null)
  const [autoAcquire, setAutoAcquire] = useState(false)
  const [deliveryTargetId, setDeliveryTargetId] = useState<number | null>(null)
  const [targets, setTargets] = useState<DeliveryTarget[]>([])
  const [catalogue, setCatalogue] = useState<CatalogueWork[]>([])
  const [catalogueNext, setCatalogueNext] = useState<string | null>(null)
  const [catalogueSort, setCatalogueSort] = useState<'newest' | 'title'>('newest')
  const [cataloguePage, setCataloguePage] = useState(1)
  const [loadingCatalogue, setLoadingCatalogue] = useState(false)
  const [cataloguePending, setCataloguePending] = useState(true)
  const [catalogueError, setCatalogueError] = useState(false)
  const [savingFollow, setSavingFollow] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false

    fetchAuthor(id)
      .then((data) => {
        if (!cancelled) {
          setAuthor(data)
        }
      })
      .catch((caught: unknown) => {
        if (!cancelled) {
          setError(caught instanceof ApiError ? caught.message : 'Failed to load author')
        }
      })

    fetchAuthorProfile(id)
      .then((data) => {
        if (!cancelled) setProfile(data)
      })
      .catch((caught: unknown) => console.warn('author.profile_failed', caught))

    fetchAuthorCatalogue(id, 1, 'newest')
      .then((data) => {
        if (!cancelled) {
          setCatalogue(data.items)
          setCatalogueNext(data.next)
          setCatalogueError(false)
        }
      })
      .catch((caught: unknown) => {
        console.warn('author.catalogue_failed', caught)
        if (!cancelled) setCatalogueError(true)
      })
      .finally(() => {
        if (!cancelled) setCataloguePending(false)
      })
    fetchAuthorFollow(id)
      .then((data) => {
        if (!cancelled) {
          setFollowing(data.following)
          setAutoAcquire(data.autoAcquire)
          setDeliveryTargetId(data.deliveryTargetId)
        }
      })
      .catch((caught: unknown) => console.warn('author.follow_state_failed', caught))
    fetchTargets()
      .then((items) => {
        if (!cancelled) {
          setTargets(items)
        }
      })
      .catch((caught: unknown) => console.warn('author.targets_failed', caught))

    return () => {
      cancelled = true
    }
  }, [id])

  if (error) {
    return (
      <div className="space-y-4">
        <Link to={returnTo} className="text-sm text-accent transition-colors hover:text-accent-strong">
          Back to {returnLabel}
        </Link>
        <Notice variant="danger">{error}</Notice>
      </div>
    )
  }

  async function toggleFollow() {
    if (following === null || savingFollow) {
      return
    }
    const next = !following
    setFollowing(next)
    setSavingFollow(true)
    try {
      await setAuthorFollow(id, next)
    } catch {
      setFollowing(!next)
    } finally {
      setSavingFollow(false)
    }
  }

  async function saveAutomation(nextAuto: boolean, nextTarget: number | null) {
    setAutoAcquire(nextAuto)
    setDeliveryTargetId(nextTarget)
    try {
      await setAuthorAutomation(id, nextAuto, nextTarget)
    } catch {
      setAutoAcquire(!nextAuto)
    }
  }

  if (!author) {
    return <p role="status" className="text-sm text-ink-muted">Loading author…</p>
  }

  // The reader's preferred languages are the browsing lens here, exactly like
  // Discover: a household can own several translations of the same book, and
  // "All languages" stays one click away.
  const preferredLanguages = user?.preferredLanguages?.length
    ? user.preferredLanguages
    : user?.preferredLanguage
      ? [user.preferredLanguage]
      : user?.defaultLanguage
        ? [user.defaultLanguage]
        : []
  const languageActive = !allLanguages && preferredLanguages.length > 0
  const visibleBooks = languageActive
    ? author.books.filter(
        (book) => book.language !== null && preferredLanguages.includes(book.language),
      )
    : author.books

  return (
    <section>
      <Link
        to={returnTo}
        className="inline-flex items-center gap-1.5 text-xs font-medium text-ink-muted transition-colors hover:text-ink"
      >
        <ArrowLeft size={14} aria-hidden />
        Back to {returnLabel}
      </Link>
      <div className="mt-4 flex flex-wrap items-center gap-4">
        <AuthorAvatar
          authorId={Number(authorId)}
          name={author.name}
          className="h-16 w-16 text-2xl"
        />
        <h1 className="font-display text-display text-ink">{author.name}</h1>
        {following !== null && (
          <Button
            variant={following ? 'secondary' : 'primary'}
            size="sm"
            className="ml-auto"
            disabled={savingFollow}
            onClick={() => void toggleFollow()}
          >
            {following ? 'Following' : 'Follow'}
          </Button>
        )}
      </div>
      {author.books.length > 0 && (
        <p className="mt-2 text-sm text-ink-muted">
          {author.books.length} {author.books.length === 1 ? 'book' : 'books'} in the household library
          {following ? ' · you will see more from this author on Home' : ''}
        </p>
      )}
      {profile && (profile.bio || profile.birthDate || profile.deathDate) && (
        <section aria-label={`About ${author.name}`} className="mt-6 max-w-2xl border-l-2 border-line pl-5">
          <h2 className="font-display text-xl text-ink">About the author</h2>
          {(profile.birthDate || profile.deathDate) && (
            <p className="mt-2 text-xs text-ink-muted">
              {[profile.birthDate && `Born ${profile.birthDate}`, profile.deathDate && `Died ${profile.deathDate}`]
                .filter(Boolean)
                .join(' · ')}
            </p>
          )}
          {profile.bio && <p className="mt-3 text-sm leading-relaxed text-ink-soft">{profile.bio}</p>}
          <a href={profile.sourceUrl} target="_blank" rel="noreferrer" className="mt-3 inline-block text-xs text-accent hover:text-accent-strong">
            Author information from Open Library ↗
          </a>
        </section>
      )}
      {following && (
        <div className="mt-4 max-w-md rounded-panel bg-surface p-4">
          <label className="flex items-start gap-2.5">
            <input
              type="checkbox"
              checked={autoAcquire}
              onChange={(event) => void saveAutomation(event.target.checked, deliveryTargetId)}
              className="mt-0.5 h-4 w-4 accent-[var(--color-accent)]"
            />
            <span>
              <span className="block text-sm text-ink">Automatically get new books</span>
              <span className="mt-0.5 block text-xs text-ink-faint">
                Existing books found when you enable this are ignored; Bokhylle will
                automatically get newly discovered recent releases from then on.
              </span>
            </span>
          </label>
          {autoAcquire && (
            <label className="mt-3 block">
              <span className="mb-1.5 block text-xs text-ink-muted">
                Send to reader when ready
              </span>
              <select
                value={deliveryTargetId === null ? '' : String(deliveryTargetId)}
                onChange={(event) =>
                  void saveAutomation(
                    true,
                    event.target.value === '' ? null : Number(event.target.value),
                  )
                }
                className="w-full rounded-card bg-surface-2 px-3.5 py-2.5 text-sm text-ink outline-none focus-visible:outline-2 focus-visible:outline-focus"
              >
                <option value="">Keep it in the library</option>
                {targets
                  .filter((target) => target.enabled)
                  .map((target) => (
                    <option key={target.id} value={target.id}>
                      {target.name}
                    </option>
                  ))}
              </select>
            </label>
          )}
        </div>
      )}
      <div className="mt-6">
        {author.books.length > 0 ? (
          <>
            <div className="mb-5 flex flex-wrap items-center justify-between gap-3">
              <p className="font-sans text-[11px] uppercase tracking-[0.18em] text-ink-faint">
                In your household library
              </p>
              {preferredLanguages.length > 0 && (
                <div className="flex items-center gap-3">
                  <span className="font-sans text-[11px] uppercase tracking-[0.16em] text-ink-faint">
                    {allLanguages
                      ? 'All languages'
                      : `Preferred · ${preferredLanguages
                          .map((value) => value.toUpperCase())
                          .join('/')}`}
                  </span>
                  <button
                    type="button"
                    aria-pressed={allLanguages}
                    onClick={() => setAllLanguages((current) => !current)}
                    className="font-sans text-[11px] font-medium uppercase tracking-[0.16em] text-ink-muted transition-colors hover:text-ink"
                  >
                    {allLanguages ? 'Preferred only' : 'Show all languages'}
                  </button>
                </div>
              )}
            </div>
            {visibleBooks.length > 0 ? (
              <BookGrid books={visibleBooks} />
            ) : (
              <div className="flex flex-col items-start gap-3 border-l-2 border-line pl-5">
                <p className="text-sm text-ink-soft">
                  Nothing in {preferredLanguages.map((value) => value.toUpperCase()).join('/')}{' '}
                  by this author in the library.
                </p>
                <Button variant="ghost" size="sm" onClick={() => setAllLanguages(true)}>
                  Show all languages
                </Button>
              </div>
            )}
          </>
        ) : (
          <p className="text-sm text-ink-muted">
            No books by this author are in your household library yet.
            {following === true && ' New releases will appear on Home.'}
            {following === false && ' Follow this author to see new releases on Home.'}
          </p>
        )}
      </div>
      {cataloguePending && <p role="status" className="mt-8 text-sm text-ink-muted">Looking for more books by this author…</p>}
      {catalogueError && <p role="status" className="mt-8 text-sm text-ink-muted">More books could not be loaded right now.</p>}
      {catalogue.length > 0 && (
        <div className="mt-10">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <p className="font-sans text-[11px] uppercase tracking-[0.18em] text-ink-faint">
              Not in your library yet
            </p>
            <div className="flex items-center gap-1.5">
              {(
                [
                  ['newest', 'Newest'],
                  ['title', 'Title A–Z'],
                ] as const
              ).map(([value, label]) => (
                <button
                  key={value}
                  type="button"
                  aria-pressed={catalogueSort === value}
                  disabled={cataloguePending}
                  onClick={() => {
                    if (catalogueSort === value) return
                    setCatalogueSort(value)
                    setCataloguePage(1)
                    setCatalogueNext(null)
                    setCataloguePending(true)
                    fetchAuthorCatalogue(id, 1, value)
                      .then((data) => {
                        setCatalogue(data.items)
                        setCatalogueNext(data.next)
                        setCatalogueError(false)
                      })
                      .catch((caught: unknown) => {
                        console.warn('author.catalogue_sort_failed', caught)
                        setCatalogueError(true)
                      })
                      .finally(() => setCataloguePending(false))
                  }}
                  className={`rounded-[3px] px-2.5 py-1 font-sans text-[11px] font-medium uppercase tracking-[0.14em] transition-colors ${
                    catalogueSort === value
                      ? 'bg-accent text-accent-ink'
                      : 'bg-surface-2 text-ink-muted hover:bg-surface-3 hover:text-ink'
                  }`}
                >
                  {label}
                </button>
              ))}
            </div>
          </div>
          <ul className={`mt-3 divide-y divide-line ${cataloguePending ? 'opacity-60' : ''}`}>
            {catalogue.map((work) => (
              <li key={`${work.provider}-${work.providerKey}`}>
                <Link
                  to={`/discover?provider=${encodeURIComponent(work.provider)}&providerKey=${encodeURIComponent(work.providerKey)}`}
                  className="flex items-center justify-between gap-4 py-3 transition-colors hover:text-accent"
                >
                  <span className="min-w-0 truncate text-sm text-ink-soft">
                    {work.title}
                    {work.year ? ` · ${work.year}` : ''}
                  </span>
                  <span className="shrink-0 font-sans text-[11px] uppercase tracking-[0.14em] text-ink-faint">
                    Check availability
                  </span>
                </Link>
              </li>
            ))}
          </ul>
          {catalogueNext && (
            <div className="mt-4 flex justify-center">
              <Button
                variant="secondary"
                size="sm"
                disabled={loadingCatalogue || cataloguePending}
                onClick={() => {
                  const nextPage = cataloguePage + 1
                  setLoadingCatalogue(true)
                  fetchAuthorCatalogue(id, nextPage, catalogueSort)
                    .then((data) => {
                      setCatalogue((current) => [...current, ...data.items])
                      setCatalogueNext(data.next)
                      setCataloguePage(nextPage)
                    })
                    .catch((caught: unknown) => {
                      console.warn('author.catalogue_more_failed', caught)
                      setCatalogueError(true)
                    })
                    .finally(() => setLoadingCatalogue(false))
                }}
              >
                {loadingCatalogue ? 'Loading…' : 'Load more'}
              </Button>
            </div>
          )}
        </div>
      )}

    </section>
  )
}

export function AuthorDetailRoute() {
  const { authorId } = useParams()
  return <AuthorDetail key={authorId} authorId={authorId ?? ''} />
}

import { useEffect, useState } from 'react'
import { Link, useLocation, useNavigate } from 'react-router-dom'
import { ArrowRight } from 'lucide-react'
import { heroBlurb } from '../lib/blurb'
import { EmptyState } from '../components/ui/EmptyState'
import { fetchOnboarding } from '../api/profile'
import { ApiError } from '../api/client'
import {
  type AuthorSummary,
  type BookSummary,
  type ContinueReadingItem,
  type HomeRail,
  coverUrl,
  fetchAuthors,
  fetchBooks,
  fetchContinueReading,
  fetchHighlights,
  fetchHomeRails,
  fetchRecent,
  setSubjectHidden,
} from '../api/library'
import { BookCard } from '../components/BookCard'
import { discoverCoverUrl } from '../api/discover'
import { AuthorAvatar } from '../components/AuthorAvatar'
import {
  type SpotlightItem,
  type Updates,
  fetchSpotlight,
  fetchUpdates,
} from '../api/library'
import { Spotlight, SpotlightSkeleton } from '../components/Spotlight'
import { ShelfBook, ShelfBookSkeleton, ShelfRail } from '../components/ShelfRail'
import { MetaLine } from '../components/ui/MetaLine'
import { SectionMark } from '../components/ui/SectionMark'
import { type CollectionDetail, fetchCollection, fetchCollections } from '../api/collections'
import { BookCover } from '../components/BookCover'
import { BookRail } from '../components/BookRail'
import { ButtonLink } from '../components/ui/Button'
import { useAuth } from '../auth/useAuth'
import { BrandMark } from '../components/BrandMark'

type HomeViewState = {
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

const HOME_TTL_MS = 60_000
const homeCache = new Map<string, HomeViewState>()

function baseView(): HomeViewState {
  return {
    spotlight: [],
    recommendations: [],
    updates: null,
    recent: [],
    continueReading: [],
    highlights: [],
    authors: [],
    shelves: [],
    rails: [],
    householdBooks: 0,
    loadedAt: Date.now(),
  }
}

function ReadingStatus({ item }: { item: ContinueReadingItem }) {
  const browserPosition = item.browserFileId != null && item.browserPercentage != null
  const reader = browserPosition ? 'Bokhylle' : 'KOReader'
  // The card opens Bokhylle when an EPUB is available. Show the position it
  // will actually resume, rather than the most recent device's position.
  const percentage = Math.min(1, Math.max(0, browserPosition ? item.browserPercentage! : item.percentage))
  const rounded = Math.round(percentage * 100)
  const canOpenHere = item.browserFileId != null || item.epubFileId != null
  const differentKoReaderPosition = browserPosition && item.source === 'koreader' &&
    Math.round(item.percentage * 100) !== rounded

  return (
    <span className="block">
      <span className="flex items-baseline justify-between gap-1 text-[11px] tabular-nums">
        {(!browserPosition || differentKoReaderPosition) && <span>{reader}</span>}
        <span className="ml-auto">{rounded}%</span>
      </span>
      <span
        className="mt-1 block h-1 overflow-hidden rounded-full bg-line"
        role="progressbar"
        aria-label={`${reader} reading progress`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={rounded}
      >
        <span className="block h-full bg-accent" style={{ width: `${percentage * 100}%` }} />
      </span>
      {differentKoReaderPosition && (
        <span className="mt-1 block text-[11px] tabular-nums text-ink-muted">
          KOReader last · {Math.round(item.percentage * 100)}%
        </span>
      )}
      {!browserPosition && canOpenHere && (
        <span className="mt-1 block text-[11px] text-ink-muted">Starts here at beginning</span>
      )}
    </span>
  )
}

export function Home() {
  const { user } = useAuth()
  const location = useLocation()
  const fromOnboarding = (location.state as { fromOnboarding?: boolean } | null)?.fromOnboarding === true
  const isChild = user?.profileType === 'child'
  const navigate = useNavigate()
  const preferredLanguages = user?.preferredLanguages?.length
    ? user.preferredLanguages
    : user?.preferredLanguage
      ? [user.preferredLanguage]
      : user?.defaultLanguage
        ? [user.defaultLanguage]
        : []
  const cacheKey = `${user?.id ?? 0}|${isChild ? 'child' : 'adult'}|${user?.canDiscover ? 'discover' : 'shelf'}|${preferredLanguages.join(',')}`
  const [view, setView] = useState<HomeViewState | null>(
    () => fromOnboarding ? null : homeCache.get(cacheKey) ?? null,
  )
  const [error, setError] = useState<string | null>(null)
  const [hiddenNotice, setHiddenNotice] = useState<{ subject: string; title: string } | null>(null)
  const [heroPending, setHeroPending] = useState(false)

  const store = (next: HomeViewState) => {
    homeCache.set(cacheKey, next)
    return next
  }

  // New members land in the setup wizard once, which is always skippable.
  useEffect(() => {
    let cancelled = false
    fetchOnboarding()
      .then((data) => {
        if (!cancelled && !data.onboarded) {
          navigate('/welcome')
        }
      })
      .catch((caught: unknown) => console.warn('home.onboarding_gate_failed', caught))
    return () => {
      cancelled = true
    }
  }, [navigate])

  useEffect(() => {
    const cached = fromOnboarding ? undefined : homeCache.get(cacheKey)
    if (cached && Date.now() - cached.loadedAt < HOME_TTL_MS) {
      setView(cached)
      setHeroPending(false)
      return
    }

    let cancelled = false
    // Sections publish as they settle: core/local content first, the hero
    // when Spotlight arrives, into its reserved slot. The cache is written
    // only once both halves have settled.
    let merged = cached ?? (fromOnboarding ? null : view) ?? baseView()
    let coreDone = false
    let spotlightDone = false
    setHeroPending(merged.spotlight.length === 0)

    const publish = () => {
      if (cancelled || !coreDone) {
        return
      }
      const next = { ...merged, loadedAt: Date.now() }
      setView(next)
      if (coreDone && spotlightDone) {
        homeCache.set(cacheKey, next)
      }
    }

    const shelvesPromise = isChild
      ? Promise.resolve([] as CollectionDetail[])
      : fetchCollections()
          .then((collections) =>
            Promise.all(
              collections
                .filter((collection) => collection.bookCount > 0)
                .slice(0, 4)
                .map((collection) => fetchCollection(collection.id)),
            ),
          )
          .catch(() => [] as CollectionDetail[])

    Promise.allSettled([
      fetchRecent(12),
      fetchHighlights(12),
      fetchContinueReading(),
      // Children cannot browse authors; the call would be refused and blank
      // the shelf they are allowed to see.
      isChild ? Promise.resolve([] as AuthorSummary[]) : fetchAuthors(),
      isChild
        ? Promise.resolve(0)
        : fetchBooks('recent', 1, 1, { mine: false })
            .then((page) => page.total)
            .catch(() => 0),
      fetchHomeRails(),
      shelvesPromise,
      isChild ? Promise.resolve(null) : fetchUpdates(),
    ]).then((results) => {
      if (cancelled) {
        return
      }
      const value = <T,>(index: number, fallback: T): T => {
        const result = results[index]
        return result && result.status === 'fulfilled' ? (result.value as T) : fallback
      }
      merged = {
        ...merged,
        recent: value(0, merged.recent),
        highlights: value(1, merged.highlights),
        continueReading: value(2, merged.continueReading),
        authors: value(3, merged.authors),
        householdBooks: value(4, merged.householdBooks),
        rails: value(5, merged.rails),
        shelves: value(6, merged.shelves),
        updates: value(7, merged.updates),
      }
      coreDone = true
      publish()
      const core = results[0]
      if (core && core.status === 'rejected') {
        const caught = core.reason
        setError(caught instanceof ApiError ? caught.message : 'Could not load your library')
      }
    })

    fetchSpotlight()
      .then((data) => {
        if (cancelled) {
          return
        }
        merged = {
          ...merged,
          spotlight: data.items
            .filter((item) => (isChild ? item.source === 'shelf' || (user?.canDiscover && item.source === 'discover') : true) && item.blurb && heroBlurb(item.blurb) !== null)
            .slice(0, 5),
          recommendations: isChild && !user?.canDiscover ? [] : data.recommendations ?? [],
        }
        spotlightDone = true
        publish()
      })
      .catch(() => {
        spotlightDone = true
        publish()
      })
      .finally(() => {
        if (!cancelled) {
          setHeroPending(false)
        }
      })

    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cacheKey, fromOnboarding, isChild, user?.canDiscover])

  useEffect(() => {
    if (!hiddenNotice) {
      return
    }
    const timer = setTimeout(() => setHiddenNotice(null), 6000)
    return () => clearTimeout(timer)
  }, [hiddenNotice])

  async function hideRail(rail: HomeRail) {
    if (!rail.subject) {
      return
    }
    try {
      await setSubjectHidden(rail.subject, true)
      const current = view ?? baseView()
      setView(
        store({ ...current, rails: current.rails.filter((item) => item.key !== rail.key) }),
      )
      setHiddenNotice({ subject: rail.subject, title: rail.title })
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not hide the category')
    }
  }

  async function undoHide() {
    if (!hiddenNotice) {
      return
    }
    try {
      await setSubjectHidden(hiddenNotice.subject, false)
      setHiddenNotice(null)
      const items = await fetchHomeRails()
      setView(store({ ...(view ?? baseView()), rails: items }))
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not restore the category')
    }
  }

  const spotlight = view?.spotlight ?? []
  const recommendations = view?.recommendations ?? []
  const updates = view?.updates ?? null
  const recent = view?.recent ?? []
  // The endpoint merges browser and KOReader activity; never manufacture progress.
  const continueReading = (view?.continueReading ?? []).filter((item) => Number.isFinite(item.percentage) && item.percentage > 0 && item.percentage < 0.995)
  const highlights = view?.highlights ?? []
  const authors = view?.authors ?? []
  const shelves = view?.shelves ?? []
  const rails = view?.rails ?? []
  const householdBooks = view?.householdBooks ?? 0
  const loading = view === null
  const recentBooks = recent.filter(
    (book) => !spotlight.some((item) => item.bookId === book.id),
  )
  const rediscoveredBooks = highlights.filter(
    (book) => !spotlight.some((item) => item.bookId === book.id),
  )

  // A shelf can be empty while the household, follows, or the wizard's
  // taste signals still have something useful to show.
  const hasContent =
    recent.length > 0 ||
    continueReading.length > 0 ||
    highlights.length > 0 ||
    spotlight.length > 0 ||
    recommendations.length > 0 ||
    rails.length > 0 ||
    shelves.length > 0 ||
    authors.length > 0 ||
    (updates?.discoveries.length ?? 0) > 0

  if (loading) {
    return (
      <>
        <h1 className="sr-only">Your library</h1>
        <div role="status" aria-live="polite" className="mb-8 flex items-center gap-4 border-b border-line pb-6">
          <BrandMark className="h-7 w-7 shrink-0 animate-pulse text-ink-faint" />
          <div>
            <p className="font-display text-xl text-ink">{fromOnboarding ? 'Preparing your library' : 'Loading your library'}</p>
            <p className="mt-1 text-sm text-ink-muted">{fromOnboarding ? 'Your choices are saved. Gathering your books and suggestions…' : 'Gathering your books and suggestions…'}</p>
          </div>
        </div>
        <HomeSkeleton />
      </>
    )
  }

  if (!hasContent) {
    const householdHasBooks = !isChild && householdBooks > 0
    return (
      <>
        <h1 className="sr-only">Your library</h1>
        <EmptyState
          className="mt-10"
          title={
            isChild
              ? 'Your shelf is empty'
              : householdHasBooks
                ? 'Your shelf is empty'
                : 'Your library is empty'
          }
          message={
            isChild
              ? user?.canRequest
                ? 'Ask an adult to add books to your shelf, or search for a book and ask for it. Books appear here after approval.'
                : 'Ask an adult to add some books to your shelf.'
              : householdHasBooks
                ? `The household library has ${householdBooks} ${
                    householdBooks === 1 ? 'book' : 'books'
                  }. Choose books for your shelf, or choose reading interests for personal suggestions.`
                : 'Find a book and Bokhylle will fetch it for you, file it neatly, and send it to your reader when you are ready.'
          }
          action={
            isChild ? user?.canDiscover ? (
              <ButtonLink to="/discover" variant="primary" size="md">
                Discover books
                <ArrowRight size={16} />
              </ButtonLink>
            ) : user?.canRequest ? (
              <ButtonLink to="/requests" variant="primary" size="md">Search and ask<ArrowRight size={16} /></ButtonLink>
            ) : undefined : householdHasBooks ? (
              <div className="flex flex-wrap items-center justify-center gap-3">
                <ButtonLink to="/welcome" variant="primary" size="md">
                  Choose reading interests
                </ButtonLink>
                <ButtonLink to="/library?scope=household" variant="secondary" size="md">
                  Browse household library
                </ButtonLink>
              </div>
            ) : (
              <ButtonLink to="/discover" variant="primary" size="md">
                Discover books
                <ArrowRight size={16} />
              </ButtonLink>
            )
          }
        />
        {error && <p className="mt-4 text-sm text-danger">{error}</p>}
      </>
    )
  }


  return (
    <div className="space-y-10 sm:space-y-12">
      <h1 className="sr-only">Your library</h1>
      {error && <p className="border-l-2 border-danger pl-4 text-sm text-danger">{error}</p>}

      {spotlight.length > 0 ? (
        <Spotlight items={spotlight} preferredLanguages={preferredLanguages} />
      ) : heroPending ? <SpotlightSkeleton /> : null}

      {recommendations.length > 0 && (
        <section className="home-shelf-section">
          <div className="home-shelf-heading">
            <SectionMark
              rule={false}
              title="Picked for you"
              action={<Link to="/discover" className="text-xs font-medium text-accent hover:text-accent-strong">Explore more</Link>}
            />
          </div>
          <ShelfRail label="Picked for you books">
            {recommendations.map((item) => (
              <Link
                key={`${item.provider}-${item.providerKey}`}
                to={`/discover?provider=${encodeURIComponent(item.provider ?? 'openlibrary')}&providerKey=${encodeURIComponent(item.providerKey ?? '')}`}
                state={isChild ? undefined : { backgroundLocation: location }}
                className="shelf-book group block"
              >
                <ShelfBook
                  title={item.title}
                  authors={item.authors}
                  cover={item.coverId ? discoverCoverUrl(item.coverId, item.title, item.provider ?? undefined) : null}
                />
              </Link>
            ))}
          </ShelfRail>
        </section>
      )}

      {updates && updates.discoveries.length > 0 && (
        <section className="home-shelf-section">
          <div className="home-shelf-heading">
            <SectionMark
              rule={false}
              title="From authors you follow"
              action={
                <Link
                  to="/library?mode=authors&following=1"
                  className="text-xs font-medium text-accent transition-colors hover:text-accent-strong"
                >
                  See all
                </Link>
              }
            />
          </div>
          <ShelfRail label="Books from authors you follow">
            {updates.discoveries.map((item) => (
              <Link
                key={item.providerKey}
                to={`/discover?provider=${encodeURIComponent(item.provider)}&providerKey=${encodeURIComponent(item.providerKey)}`}
                state={{ backgroundLocation: location }}
                className="shelf-book group block"
              >
                <ShelfBook
                  title={item.title}
                  authors={item.authors}
                  cover={item.coverId ? discoverCoverUrl(item.coverId, item.title, item.provider) : null}
                  context={item.year ? String(item.year) : undefined}
                />
              </Link>
            ))}
          </ShelfRail>
        </section>
      )}

      {continueReading.length > 0 && (
        <section className="home-shelf-section">
          <div className="home-shelf-heading">
            <SectionMark
              rule={false}
              title="Continue reading"
              action={
                <Link
                  to="/library"
                  className="text-xs font-medium text-accent transition-colors hover:text-accent-strong"
                >
                  See all
                </Link>
              }
            />
          </div>
          <ShelfRail label="Continue reading books">
            {continueReading.map((item) => (
              <BookCard
                key={item.book.id}
                book={item.book}
                appearance="shelf"
                to={item.browserFileId || item.epubFileId
                  ? `/read/${item.book.id}/${item.browserFileId ?? item.epubFileId}`
                  : `/library/${item.book.id}`}
                status={<ReadingStatus item={item} />}
              />
            ))}
          </ShelfRail>
        </section>
      )}

      {recentBooks.length > 0 && (
        <BookRail title="Recently Added" books={recentBooks} seeAllHref="/library" appearance="shelf" />
      )}

      {rediscoveredBooks.length > 0 && (
        <BookRail
          title="Rediscover your library"
          subtitle="A few books already on your shelves, worth another look."
          books={rediscoveredBooks.slice(0, 3)}
          seeAllHref="/library"
          appearance="shelf"
        />
      )}

      {rails.map((rail) => (
        <BookRail
          key={rail.key}
          appearance="shelf"
          title={rail.title}
          subtitle={rail.subtitle}
          books={rail.books.filter(
            (book) => !spotlight.some((item) => item.bookId === book.id),
          )}
          seeAllHref={
            rail.subject ? `/library?subject=${encodeURIComponent(rail.subject)}` : '/library'
          }
          onHide={rail.subject ? () => void hideRail(rail) : undefined}
        />
      ))}

      {hiddenNotice && (
        <p
          role="status"
          className="flex flex-wrap items-center justify-between gap-2 border-l-2 border-accent pl-4 text-sm text-ink-soft"
        >
          <span>Hidden &ldquo;{hiddenNotice.title}&rdquo; for you.</span>
          <button
            type="button"
            onClick={() => void undoHide()}
            className="text-xs font-medium text-accent transition-colors hover:text-accent-strong"
          >
            Undo
          </button>
        </p>
      )}

      {shelves.length > 0 && (
        <section>
          <SectionMark
            rule={false}
            title="Collections"
            action={
              <Link
                to="/library"
                className="text-xs font-medium text-accent transition-colors hover:text-accent-strong"
              >
                Browse all
              </Link>
            }
          />
          <div className="mt-7 grid gap-8 sm:grid-cols-2 lg:grid-cols-4">
            {shelves.map((shelf) => (
              <Link
                key={shelf.id}
                to={`/library?collection=${shelf.id}`}
                className="group block"
              >
                <div className="relative h-24">
                  {shelf.books.slice(0, 3).map((book, index) => (
                    <BookCover
                      key={book.id}
                      src={coverUrl(book.id)}
                      className="absolute top-0 aspect-[2/3] w-16 rounded-[3px] shadow-card transition-transform duration-200 ease-smooth group-hover:-translate-y-0.5"
                      style={{ left: `${index * 20}px`, zIndex: 3 - index }}
                    />
                  ))}
                </div>
                <p className="mt-4 font-display text-lg text-ink transition-colors group-hover:text-accent">
                  {shelf.name}
                </p>
                <MetaLine
                  className="mt-1"
                  items={[`${shelf.bookCount} ${shelf.bookCount === 1 ? 'book' : 'books'}`]}
                />
              </Link>
            ))}
          </div>
        </section>
      )}

      {authors.length > 0 && (
        <section>
          <SectionMark
            rule={false}
            title="Authors"
            action={
              <Link
                to="/library?mode=authors"
                className="text-xs font-medium text-accent transition-colors hover:text-accent-strong"
              >
                Browse all
              </Link>
            }
          />
          <div className="rail-scroll -mx-4 mt-7 flex gap-6 overflow-x-auto px-4 pb-2 sm:mx-0 sm:px-0">
            {authors.slice(0, 16).map((author) => (
              <Link
                key={author.id}
                to={`/authors/${author.id}`}
                state={{ authorReturnTo: `${location.pathname}${location.search}${location.hash}` }}
                className="group flex w-32 shrink-0 flex-col items-center text-center"
              >
                <AuthorAvatar
                  authorId={author.id}
                  name={author.name}
                  className="h-14 w-14 text-lg"
                />
                <span className="mt-3 line-clamp-2 text-sm font-medium text-ink transition-colors group-hover:text-accent">
                  {author.name}
                </span>
                <MetaLine
                  className="mt-1"
                  items={[`${author.bookCount} ${author.bookCount === 1 ? 'book' : 'books'}`]}
                />
              </Link>
            ))}
          </div>
        </section>
      )}
    </div>
  )
}

function HomeSkeleton() {
  return (
    <div className="space-y-10 sm:space-y-12" aria-hidden>
      <SpotlightSkeleton />
      {[0, 1].map((section) => (
        <div key={section}>
          <div className="h-7 w-44 animate-pulse rounded bg-surface-2" />
          <ShelfRail label="Loading books" loading>
            {Array.from({ length: 6 }, (_, index) => <ShelfBookSkeleton key={index} />)}
          </ShelfRail>
        </div>
      ))}
    </div>
  )
}

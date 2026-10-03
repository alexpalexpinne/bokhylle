import { useEffect, useState } from 'react'
import { Link, useLocation, useNavigate } from 'react-router-dom'
import { ArrowRight } from 'lucide-react'
import { EmptyState } from '../components/ui/EmptyState'
import { fetchOnboarding } from '../api/profile'
import { ApiError } from '../api/client'
import {
  type ContinueReadingItem,
  type HomeRail,
  coverUrl,
  fetchHomeRails,
  setSubjectHidden,
} from '../api/library'
import { BookCard } from '../components/BookCard'
import { discoverCoverUrl, localDiscoveryBookId } from '../api/discover'
import { AuthorAvatar } from '../components/AuthorAvatar'
import { Spotlight, SpotlightSkeleton } from '../components/Spotlight'
import { ShelfBook, ShelfBookSkeleton, ShelfRail } from '../components/ShelfRail'
import { MetaLine } from '../components/ui/MetaLine'
import { SectionMark } from '../components/ui/SectionMark'
import { BookCover } from '../components/BookCover'
import { BookRail } from '../components/BookRail'
import { ButtonLink } from '../components/ui/Button'
import { useAuth } from '../auth/useAuth'
import { type HomeSection, useHomeSnapshot } from './useHomeSnapshot'

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
  const preferredLanguages = user?.preferredLanguages?.length
    ? user.preferredLanguages
    : user?.preferredLanguage
      ? [user.preferredLanguage]
      : user?.defaultLanguage
        ? [user.defaultLanguage]
        : []
  const cacheKey = JSON.stringify([user?.id, user?.username, user?.role, user?.profileType,
    user?.canDiscover, user?.canRequest, preferredLanguages])
  return <HomePage key={`${cacheKey}|${fromOnboarding}`} cacheKey={cacheKey}
    fromOnboarding={fromOnboarding} preferredLanguages={preferredLanguages} />
}

function HomePage({ cacheKey, fromOnboarding, preferredLanguages }: {
  cacheKey: string
  fromOnboarding: boolean
  preferredLanguages: string[]
}) {
  const { user } = useAuth()
  const location = useLocation()
  const navigate = useNavigate()
  const isChild = user?.profileType === 'child'
  const [hiddenNotice, setHiddenNotice] = useState<{ subject: string; title: string } | null>(null)
  const { view, setView, pending, refreshPending, error, setError, store, visible, nextSection } =
    useHomeSnapshot({ cacheKey, fromOnboarding, isChild, canDiscover: !!user?.canDiscover })

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
      const current = view
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
      setView(store({ ...view, rails: items }))
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not restore the category')
    }
  }

  const spotlight = view.spotlight
  const recommendations = view.recommendations
  const updates = view.updates
  // Spotlight features one book at a time. Its candidates still belong on
  // their shelves, especially when a small shelf fits entirely in Spotlight.
  const recent = view.recent
  // The endpoint merges browser and KOReader activity; never manufacture progress.
  const continueReading = view.continueReading.filter((item) => Number.isFinite(item.percentage) && item.percentage > 0 && item.percentage < 0.995)
  const highlights = view.highlights
  const authors = view.authors
  const shelves = view.shelves
  const rails = view.rails
  const householdBooks = view.householdBooks
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
  const loading = Object.values(pending).some(Boolean) || (!hasContent && refreshPending)

  if (!hasContent && !loading) {
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

      {visible('spotlight') && spotlight.length > 0 ? (
        <Spotlight items={spotlight} preferredLanguages={preferredLanguages} />
      ) : pending.spotlight || (!hasContent && refreshPending) ? <SpotlightSkeleton /> : null}

      {visible('spotlight') && recommendations.length > 0 && (
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
                key={item.bookId ?? `${item.provider}-${item.providerKey}`}
                to={item.bookId ? `/library/${item.bookId}` : `/discover?provider=${encodeURIComponent(item.provider ?? 'openlibrary')}&providerKey=${encodeURIComponent(item.providerKey ?? '')}`}
                state={item.bookId || isChild ? undefined : { backgroundLocation: location }}
                className="shelf-book group block"
              >
                <ShelfBook
                  title={item.title}
                  authors={item.authors}
                  cover={item.bookId ? coverUrl(item.bookId) : item.coverId ? discoverCoverUrl(item.coverId, item.title, item.provider ?? undefined) : null}
                />
              </Link>
            ))}
          </ShelfRail>
        </section>
      )}

      {visible('updates') && updates && updates.discoveries.length > 0 && (
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
            {updates.discoveries.map((item) => {
              const bookId = localDiscoveryBookId(item.provider, item.providerKey)
              return (
                <Link
                  key={`${item.provider}-${item.providerKey}`}
                  to={bookId ? `/library/${bookId}` : `/discover?provider=${encodeURIComponent(item.provider)}&providerKey=${encodeURIComponent(item.providerKey)}`}
                  state={bookId ? undefined : { backgroundLocation: location }}
                  className="shelf-book group block"
                >
                  <ShelfBook
                    title={item.title}
                    authors={item.authors}
                    cover={bookId ? coverUrl(bookId) : item.coverId ? discoverCoverUrl(item.coverId, item.title, item.provider) : null}
                    context={item.year ? String(item.year) : undefined}
                  />
                </Link>
              )
            })}
          </ShelfRail>
        </section>
      )}

      {visible('continueReading') && continueReading.length > 0 && (
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

      {visible('recent') && recent.length > 0 && (
        <BookRail title="Recently Added" books={recent} seeAllHref="/library" appearance="shelf" />
      )}

      {visible('highlights') && highlights.length > 0 && (
        <BookRail
          title="Rediscover your library"
          subtitle="A few books already on your shelves, worth another look."
          books={highlights.slice(0, 3)}
          seeAllHref="/library"
          appearance="shelf"
        />
      )}

      {visible('rails') && rails.map((rail) => (
        <BookRail
          key={rail.key}
          appearance="shelf"
          title={rail.title}
          subtitle={rail.subtitle}
          books={rail.books}
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

      {visible('shelves') && shelves.length > 0 && (
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

      {visible('authors') && authors.length > 0 && (
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
      {nextSection && nextSection !== 'spotlight' && <HomeSectionSkeleton section={nextSection} />}
      {loading && <p role="status" className="sr-only">{fromOnboarding ? 'Preparing your library' : 'Loading your library'}</p>}
    </div>
  )
}

function HomeSectionSkeleton({ section }: { section: HomeSection }) {
  if (section === 'authors' || section === 'shelves') {
    return (
      <section aria-hidden>
        <div className="h-7 w-44 animate-pulse rounded bg-surface-2" />
        <div className={section === 'authors'
          ? 'rail-scroll mt-7 flex gap-6 overflow-hidden'
          : 'mt-7 grid gap-8 sm:grid-cols-2 lg:grid-cols-4'}>
          {Array.from({ length: 4 }, (_, index) => (
            <div key={index} className={section === 'authors'
              ? 'flex w-32 shrink-0 animate-pulse flex-col items-center'
              : 'animate-pulse'}>
              <div className={section === 'authors'
                ? 'h-14 w-14 rounded-full bg-surface-2'
                : 'h-24 w-28 rounded-[3px] bg-surface-2'} />
              <div className="mt-4 h-4 w-24 rounded bg-surface-2" />
              <div className="mt-2 h-3 w-16 rounded bg-surface-2" />
            </div>
          ))}
        </div>
      </section>
    )
  }
  return (
    <section aria-hidden className={section === 'updates' || section === 'continueReading' ? 'home-shelf-section' : undefined}>
      <div className="home-shelf-heading"><div className="h-7 w-44 animate-pulse rounded bg-surface-2" /></div>
      <ShelfRail label="Loading books" loading>
        {Array.from({ length: 6 }, (_, index) => <ShelfBookSkeleton key={index} />)}
      </ShelfRail>
    </section>
  )
}

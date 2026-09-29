import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { X } from 'lucide-react'
import { useAuth } from '../auth/useAuth'
import { ChildOnboarding } from './ChildWelcome'
import { ApiError } from '../api/client'
import { createTarget, fetchDefaultReader } from '../api/delivery'
import {
  type AuthorHit,
  type BookSummary,
  claimShelf,
  coverUrl,
  fetchBooks,
  followDiscoverAuthor,
  searchBooks,
  searchDiscoverAuthors,
  setAuthorFollow,
  setBookPreference,
} from '../api/library'
import { completeOnboarding, saveInterests, updateProfile } from '../api/profile'
import { type DiscoveryResult, discoverCoverUrl, likeExternalBook, searchDiscover } from '../api/discover'
import { AuthorAvatar } from '../components/AuthorAvatar'
import { BookCover } from '../components/BookCover'
import { BrandMark } from '../components/BrandMark'
import { Button } from '../components/ui/Button'
import { Input } from '../components/ui/Field'
import { SearchField } from '../components/ui/SearchField'
import { LANGUAGES, languageLabel } from '../lib/languages'
import { GENRES } from '../lib/genres'

const STEP_TITLES = [
  'What do you like to read?',
  'What languages do you read in?',
  'Pick a few authors you like',
  'Pick a few books you loved',
  'Where should books go?',
  'How automatic should Bokhylle be?',
]

function AdultOnboarding() {
  const navigate = useNavigate()
  const { user } = useAuth()
  const [step, setStep] = useState(0)
  const [languages, setLanguages] = useState<string[]>(() => {
    if (user?.preferredLanguages?.length) {
      return user.preferredLanguages
    }
    return user?.defaultLanguage ? [user.defaultLanguage] : []
  })
  const [subjects, setSubjects] = useState<string[]>([])
  const [authorQuery, setAuthorQuery] = useState('')
  const [authorHits, setAuthorHits] = useState<AuthorHit[]>([])
  const [bookQuery, setBookQuery] = useState('')
  const [bookHits, setBookHits] = useState<BookSummary[]>([])
  const [discoverHits, setDiscoverHits] = useState<DiscoveryResult[]>([])
  const [liked, setLiked] = useState<number[]>([])
  const [likedKeys, setLikedKeys] = useState<string[]>([])
  const [readerEmail, setReaderEmail] = useState('')
  const [readerType, setReaderType] = useState<'kindle' | 'pocketbook' | 'other' | 'later'>('kindle')
  const [readerSaved, setReaderSaved] = useState(false)
  const [mode, setMode] = useState<'automatic' | 'ask'>('automatic')
  const [amazonUrl, setAmazonUrl] = useState<string | null>(null)
  const [householdBooks, setHouseholdBooks] = useState(0)
  const [startWithHousehold, setStartWithHousehold] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const authorSearchSeq = useRef(0)
  const bookSearchSeq = useRef(0)

  useEffect(() => {
    fetchBooks('recent', 1, 1, { mine: false })
      .then((page) => setHouseholdBooks(page.total))
      .catch((caught: unknown) => console.warn('onboarding.household_count_failed', caught))
  }, [])

  useEffect(() => {
    fetchDefaultReader()
      .then((reader) => setAmazonUrl(reader.amazonUrl))
      .catch((caught: unknown) => console.warn('onboarding.default_reader_failed', caught))
  }, [])

  useEffect(() => {
    const generation = ++authorSearchSeq.current
    const trimmed = authorQuery.trim()
    if (trimmed.length < 3) {
      setAuthorHits([])
      return
    }
    const timer = setTimeout(() => {
      searchDiscoverAuthors(trimmed)
        .then((data) => {
          if (generation === authorSearchSeq.current) {
            setAuthorHits([...data.local, ...data.external])
          }
        })
        .catch((caught: unknown) => console.warn('onboarding.author_search_failed', caught))
    }, 225)
    return () => {
      clearTimeout(timer)
      authorSearchSeq.current += 1
    }
  }, [authorQuery])

  useEffect(() => {
    const generation = ++bookSearchSeq.current
    const trimmed = bookQuery.trim()
    if (trimmed.length < 3) {
      setBookHits([])
      setDiscoverHits([])
      return
    }
    const timer = setTimeout(() => {
      // Household hits render as soon as they arrive; provider hits fill in
      // underneath so taste is captured even on an empty installation.
      searchBooks(trimmed, { mine: false })
        .then((items) => {
          if (generation === bookSearchSeq.current) {
            setBookHits(items)
          }
        })
        .catch((caught: unknown) => console.warn('onboarding.book_search_failed', caught))
      searchDiscover(trimmed, 'any')
        .then((items) => {
          if (generation === bookSearchSeq.current) {
            setDiscoverHits(items)
          }
        })
        .catch((caught: unknown) => console.warn('onboarding.discover_search_failed', caught))
    }, 125)
    return () => {
      clearTimeout(timer)
      bookSearchSeq.current += 1
    }
  }, [bookQuery])

  function clearSessionSearch() {
    try {
      sessionStorage.removeItem('bokhylle.discover.last')
    } catch {
      // Best-effort only.
    }
  }

  async function skip() {
    try {
      await completeOnboarding()
    } catch {
      // Skipping must never trap the user.
    }
    clearSessionSearch()
    navigate('/', { replace: true, state: { fromOnboarding: true } })
  }

  async function next() {
    setError(null)
    setBusy(true)
    try {
      if (step === 0 && subjects.length > 0) {
        await saveInterests(subjects)
      }
      if (step === 1) {
        await updateProfile({
          preferredLanguages: languages,
          preferredLanguage: languages[0] ?? null,
        })
      }
      if (step === 4 && readerType !== 'later' && readerEmail.trim() && !readerSaved) {
        await createTarget(readerEmail.trim(), readerType)
        setReaderSaved(true)
      }
      setStep((current) => current + 1)
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not save that yet')
    } finally {
      setBusy(false)
    }
  }

  async function finish() {
    setError(null)
    setBusy(true)
    try {
      await updateProfile({ acquisitionMode: mode })
      if (startWithHousehold && householdBooks > 0) {
        await claimShelf()
      }
      await completeOnboarding()
      clearSessionSearch()
      navigate('/', { replace: true, state: { fromOnboarding: true } })
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not finish setup')
    } finally {
      setBusy(false)
    }
  }

  return (
    <main className="mx-auto flex min-h-screen max-w-xl flex-col px-5 py-10">
      <div className="flex items-baseline justify-between">
        <p className="font-sans text-[11px] uppercase tracking-[0.18em] text-ink-faint">
          Welcome to Bokhylle · Step {step + 1} of {STEP_TITLES.length}
        </p>
        <button
          type="button"
          onClick={() => void skip()}
          className="font-sans text-[11px] font-medium uppercase tracking-[0.16em] text-ink-muted transition-colors hover:text-ink"
        >
          Skip
        </button>
      </div>

      <h1 className="mt-6 font-display text-display text-ink">{STEP_TITLES[step]}</h1>
      <p className="mt-2 text-sm text-ink-muted">
        {step === 3
          ? "This helps Bokhylle understand your taste. They don't need to be in your library."
          : 'Everything here is optional, and you can change it later in Profile.'}
      </p>

      {error && <p className="mt-4 text-sm text-danger">{error}</p>}

      <div className="mt-8 flex-1">
        {step === 0 && (
          <div className="flex flex-wrap gap-2">
            {GENRES.map((genre) => {
              const selected = subjects.includes(genre)
              return (
                <button
                  key={genre}
                  type="button"
                  aria-pressed={selected}
                  onClick={() =>
                    setSubjects((current) =>
                      selected ? current.filter((item) => item !== genre) : [...current, genre],
                    )
                  }
                  className={`rounded-[3px] px-3.5 py-2 text-sm transition-colors ${
                    selected
                      ? 'bg-accent text-accent-ink'
                      : 'bg-surface-2 text-ink-soft hover:bg-surface-3'
                  }`}
                >
                  {genre}
                </button>
              )
            })}
          </div>
        )}

        {step === 1 && (
          <div>
            <p className="text-sm text-ink-muted">
              We use this for search results and for choosing which releases to fetch.
            </p>
            {languages.length > 0 ? (
              <ul className="mt-4 flex flex-wrap gap-2">
                {languages.map((language, index) => (
                  <li key={language}>
                    <button
                      type="button"
                      aria-label={`Remove ${languageLabel(language)}`}
                      onClick={() =>
                        setLanguages((current) =>
                          current.filter((item) => item !== language),
                        )
                      }
                      className="inline-flex items-center gap-2 rounded-[3px] bg-surface-2 px-3.5 py-2 text-sm text-ink-soft transition-colors hover:bg-surface-3"
                    >
                      {languageLabel(language)}
                      {index === 0 && (
                        <span className="font-sans text-[10px] uppercase tracking-[0.14em] text-ink-faint">
                          Preferred
                        </span>
                      )}
                      <X size={13} aria-hidden />
                    </button>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="mt-4 text-sm text-ink-faint">No languages chosen yet.</p>
            )}
            <label className="mt-5 block max-w-xs">
              <span className="mb-1.5 block text-xs text-ink-muted">Add another language</span>
              <select
                value=""
                onChange={(event) => {
                  const value = event.target.value
                  if (value) {
                    setLanguages((current) => [...current, value])
                  }
                }}
                className="w-full rounded-card bg-surface-2 px-3 py-2 text-sm text-ink outline-none focus-visible:outline-2 focus-visible:outline-focus"
              >
                <option value="">Choose a language…</option>
                {LANGUAGES.filter(([code]) => !languages.includes(code)).map(([code, label]) => (
                  <option key={code} value={code}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
          </div>
        )}

        {step === 2 && (
          <div>
            <SearchField
              variant="compact"
              value={authorQuery}
              onChange={setAuthorQuery}
              placeholder="Search authors…"
              ariaLabel="Search authors"
            />
            <div className="mt-6 flex flex-wrap justify-center gap-x-6 gap-y-8 sm:justify-start">
              {authorHits.map((hit) => (
                <div
                  key={`${hit.name}|${hit.providerKey ?? ''}`}
                  className="flex w-28 flex-col items-center text-center"
                >
                  <AuthorAvatar
                    authorId={hit.authorId}
                    provider={hit.provider}
                    providerKey={hit.providerKey}
                    name={hit.name}
                    className="h-16 w-16 text-xl"
                  />
                  <span className="mt-3 line-clamp-2 text-sm font-medium text-ink">{hit.name}</span>
                  <div className="mt-auto pt-2">
                    <Button
                      variant={hit.following ? 'secondary' : 'ghost'}
                      size="sm"
                      onClick={() => {
                        const following = !hit.following
                        const matches = (item: AuthorHit) =>
                          item.name === hit.name && item.providerKey === hit.providerKey
                        const patch = (update: Partial<AuthorHit>) =>
                          setAuthorHits((current) =>
                            current.map((item) =>
                              matches(item) ? { ...item, ...update } : item,
                            ),
                          )
                        patch({ following })
                        const request =
                          hit.authorId !== null
                            ? setAuthorFollow(hit.authorId, following)
                            : following
                              ? followDiscoverAuthor({
                                  name: hit.name,
                                  provider: hit.provider,
                                  providerKey: hit.providerKey,
                                }).then((created) => {
                                  patch({ authorId: created.authorId })
                                })
                              : Promise.resolve()
                        void request.catch(() => {
                          patch({ following: hit.following })
                          setError('Could not update that follow. Try again.')
                        })
                      }}
                    >
                      {hit.following ? 'Following' : 'Follow'}
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}

        {step === 3 && (
          <div>
            <SearchField
              variant="compact"
              value={bookQuery}
              onChange={setBookQuery}
              placeholder="Search books…"
              ariaLabel="Search books"
            />
            {bookHits.length > 0 && (
              <>
                <p className="mt-5 font-sans text-[11px] font-medium uppercase tracking-[0.16em] text-ink-faint">
                  In your library
                </p>
                <ul className="mt-1 divide-y divide-line">
                  {bookHits.slice(0, 8).map((book) => (
                    <li key={book.id} className="flex items-center justify-between gap-3 py-3">
                      <span className="flex min-w-0 items-center gap-3">
                        <BookCover src={coverUrl(book.id)} className="h-16 w-11 shrink-0 rounded-[2px] bg-surface-2" />
                        <span className="min-w-0">
                          <span className="block truncate text-sm text-ink">{book.title}</span>
                          <span className="block truncate text-xs text-ink-faint">
                            {book.authors.join(', ')}
                          </span>
                        </span>
                      </span>
                      <Button
                        variant={liked.includes(book.id) ? 'primary' : 'ghost'}
                        size="sm"
                        className="shrink-0"
                        onClick={() => {
                          const wasLiked = liked.includes(book.id)
                          setLiked((current) =>
                            wasLiked ? current.filter((id) => id !== book.id) : [...current, book.id],
                          )
                          void setBookPreference(book.id, wasLiked ? null : 'liked').catch(() => {
                            setLiked((current) =>
                              wasLiked
                                ? current.includes(book.id)
                                  ? current
                                  : [...current, book.id]
                                : current.filter((id) => id !== book.id),
                            )
                            setError('Could not save that pick. Try again.')
                          })
                        }}
                      >
                        {liked.includes(book.id) ? 'Liked' : 'Love it'}
                      </Button>
                    </li>
                  ))}
                </ul>
              </>
            )}

            {discoverHits.filter(
              (hit) => !bookHits.some((book) => book.id === hit.ownedBookId),
            ).length > 0 && (
              <>
                <p className="mt-5 font-sans text-[11px] font-medium uppercase tracking-[0.16em] text-ink-faint">
                  Other books
                </p>
                <ul className="mt-1 divide-y divide-line">
                  {discoverHits
                    .filter((hit) => !bookHits.some((book) => book.id === hit.ownedBookId))
                    .slice(0, 8)
                    .map((hit) => (
                      <li
                        key={hit.providerKey}
                        className="flex items-center justify-between gap-3 py-3"
                      >
                        <span className="flex min-w-0 items-center gap-3">
                          {hit.coverId ? (
                            <BookCover
                              src={discoverCoverUrl(hit.coverId, hit.title, hit.provider)}
                              className="h-16 w-11 shrink-0 rounded-[2px] bg-surface-2"
                            />
                          ) : (
                            <span className="flex h-16 w-11 shrink-0 items-center justify-center rounded-[2px] bg-surface-2">
                              <BrandMark className="h-6 w-6 text-ink-faint" />
                            </span>
                          )}
                          <span className="min-w-0">
                            <span className="block truncate text-sm text-ink">{hit.title}</span>
                            <span className="block truncate text-xs text-ink-faint">
                              {hit.authors.join(', ')}
                              {hit.year ? ` · ${hit.year}` : ''}
                            </span>
                          </span>
                        </span>
                        <Button
                          variant={likedKeys.includes(hit.providerKey) ? 'primary' : 'ghost'}
                          size="sm"
                          className="shrink-0"
                          onClick={() => {
                            if (likedKeys.includes(hit.providerKey)) {
                              return
                            }
                            setLikedKeys((current) => [...current, hit.providerKey])
                            void likeExternalBook(hit.providerKey, hit.provider).catch(() => {
                              setLikedKeys((current) =>
                                current.filter((key) => key !== hit.providerKey),
                              )
                              setError('Could not save that pick. Try again.')
                            })
                          }}
                        >
                          {likedKeys.includes(hit.providerKey) ? 'Liked' : 'Love it'}
                        </Button>
                      </li>
                    ))}
                </ul>
              </>
            )}
          </div>
        )}

        {step === 4 && (
          <div className="space-y-4">
            <div>
              <span className="mb-1.5 block text-xs text-ink-muted">What do you read on?</span>
              <div className="flex flex-wrap gap-2">
                {(
                  [
                    ['kindle', 'Kindle'],
                    ['pocketbook', 'PocketBook'],
                    ['other', 'Other'],
                    ['later', 'Set up later'],
                  ] as const
                ).map(([value, label]) => (
                  <button
                    key={value}
                    type="button"
                    aria-pressed={readerType === value}
                    onClick={() => setReaderType(value)}
                    className={`rounded-[3px] px-3.5 py-2 text-sm transition-colors ${
                      readerType === value
                        ? 'bg-accent text-accent-ink'
                        : 'bg-surface-2 text-ink-soft hover:bg-surface-3'
                    }`}
                  >
                    {label}
                  </button>
                ))}
              </div>
            </div>

            {readerType !== 'later' && (
              <div>
                <span className="mb-1.5 block text-xs text-ink-muted">Reader address</span>
                <Input
                  value={readerEmail}
                  onChange={(event) => setReaderEmail(event.target.value)}
                  placeholder={
                    readerType === 'kindle'
                      ? 'name@kindle.com'
                      : readerType === 'pocketbook'
                        ? 'name@pbsync.com'
                        : 'reader@example.com'
                  }
                />
                {readerType === 'kindle' && (
                  <p className="mt-2 text-xs text-ink-faint">
                    Add Bokhylle as an approved sender in Amazon, or Kindle silently rejects the
                    file.
                  </p>
                )}
                {readerType === 'pocketbook' && (
                  <p className="mt-2 text-xs text-ink-faint">
                    Find your Send-to-PocketBook address in the PocketBook app; no sender approval
                    is needed.
                  </p>
                )}
              </div>
            )}

            {readerType === 'kindle' && amazonUrl && (
              <a
                href={amazonUrl}
                target="_blank"
                rel="noreferrer"
                className="inline-block text-xs text-accent transition-colors hover:text-accent-strong"
              >
                Open Amazon personal document settings
              </a>
            )}
          </div>
        )}

        {step === 5 && (
          <div className="space-y-3">
            {(
              [
                ['automatic', 'Choose automatically, so I get the best version Bokhylle finds.'],
                ['ask', 'Let me choose the version before it downloads.'],
              ] as const
            ).map(([value, label]) => (
              <label key={value} className="flex items-start gap-2.5">
                <input
                  type="radio"
                  checked={mode === value}
                  onChange={() => setMode(value)}
                  className="mt-0.5 h-4 w-4 accent-[var(--color-accent)]"
                />
                <span className="text-sm text-ink-soft">{label}</span>
              </label>
            ))}
            {householdBooks > 0 && (
              <label className="mt-6 flex items-start gap-2.5 border-t border-line pt-4">
                <input
                  type="checkbox"
                  checked={startWithHousehold}
                  onChange={(event) => setStartWithHousehold(event.target.checked)}
                  className="mt-0.5 h-4 w-4 accent-[var(--color-accent)]"
                />
                <span className="text-sm text-ink-soft">
                  Add all {householdBooks} household books to my shelf.
                </span>
              </label>
            )}
          </div>
        )}
      </div>

      <div className="mt-8 flex items-center justify-between">
        <Button
          variant="ghost"
          size="sm"
          disabled={step === 0}
          onClick={() => setStep((current) => Math.max(0, current - 1))}
        >
          Back
        </Button>
        {step < STEP_TITLES.length - 1 ? (
          <Button variant="primary" disabled={busy} onClick={() => void next()}>
            {busy ? 'Saving…' : 'Next'}
          </Button>
        ) : (
          <Button variant="primary" disabled={busy} onClick={() => void finish()}>
            {busy ? 'Saving…' : 'Finish'}
          </Button>
        )}
      </div>
    </main>
  )
}

/// The wizard forks by profile type: adults get discovery, authors and
/// readers; children only get their own shelf and interests.
export function Onboarding() {
  const { user } = useAuth()
  if (user?.profileType === 'child') {
    return <ChildOnboarding name={user.displayName ?? user.username} />
  }
  return <AdultOnboarding />
}

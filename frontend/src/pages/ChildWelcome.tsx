import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { ApiError } from '../api/client'
import { completeOnboarding, saveInterests } from '../api/profile'
import { type BookSummary, fetchBooks, setBookPreference } from '../api/library'
import { Button } from '../components/ui/Button'
import { GENRES } from '../lib/genres'

const STEP_TITLES = ['Welcome', 'Pick books you like', 'What do you like reading?']

/// Children are curated by a parent: this flow only ever shows the child's
/// own shelf and never offers discovery, household browsing or reader setup.
export function ChildOnboarding({ name }: { name: string }) {
  const navigate = useNavigate()
  const [step, setStep] = useState(0)
  const [books, setBooks] = useState<BookSummary[]>([])
  const [liked, setLiked] = useState<number[]>([])
  const [subjects, setSubjects] = useState<string[]>([])
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    fetchBooks('recent', 1, 50, { mine: true })
      .then((page) => setBooks(page.items))
      .catch((caught: unknown) => console.warn('child_welcome.recent_failed', caught))
  }, [])

  async function skip() {
    try {
      await completeOnboarding()
    } catch {
      // Skipping must never trap the user.
    }
    navigate('/', { replace: true, state: { fromOnboarding: true } })
  }

  async function finish() {
    setBusy(true)
    setError(null)
    try {
      if (subjects.length > 0) {
        await saveInterests(subjects)
      }
      await completeOnboarding()
      navigate('/', { replace: true, state: { fromOnboarding: true } })
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not finish setup')
    } finally {
      setBusy(false)
    }
  }

  function toggleLike(book: BookSummary) {
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

      {step === 0 && (
        <>
          <h1 className="mt-6 font-display text-display text-ink">Hi, {name}</h1>
          <p className="mt-2 text-sm text-ink-muted">
            This is your shelf. An adult adds the books you can read; you pick what you like.
          </p>
        </>
      )}

      {step === 1 && (
        <>
          <h1 className="mt-6 font-display text-display text-ink">Pick books you like</h1>
          <p className="mt-2 text-sm text-ink-muted">Only books on your shelf show up here.</p>
          {books.length === 0 ? (
            <p className="mt-8 rounded-panel bg-surface p-5 text-sm text-ink-muted">
              No books here yet. Ask an adult to add some books to your shelf.
            </p>
          ) : (
            <ul className="mt-6 divide-y divide-line">
              {books.map((book) => (
                <li key={book.id} className="flex items-center justify-between gap-3 py-3">
                  <span className="min-w-0">
                    <span className="block truncate text-sm text-ink">{book.title}</span>
                    <span className="block truncate text-xs text-ink-faint">
                      {book.authors.join(', ')}
                    </span>
                  </span>
                  <Button
                    variant={liked.includes(book.id) ? 'primary' : 'ghost'}
                    size="sm"
                    className="shrink-0"
                    onClick={() => toggleLike(book)}
                  >
                    {liked.includes(book.id) ? 'Liked' : 'Love it'}
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </>
      )}

      {step === 2 && (
        <>
          <h1 className="mt-6 font-display text-display text-ink">
            What do you like reading?
          </h1>
          <p className="mt-2 text-sm text-ink-muted">
            Pick what you enjoy and we&apos;ll show more of it first.
          </p>
          <div className="mt-6 flex flex-wrap gap-2">
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
        </>
      )}

      {error && <p className="mt-4 text-sm text-danger">{error}</p>}

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
          <Button variant="primary" disabled={busy} onClick={() => setStep((current) => current + 1)}>
            Next
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

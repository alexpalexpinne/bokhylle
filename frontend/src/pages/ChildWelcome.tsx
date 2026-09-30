import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { ApiError } from '../api/client'
import { completeOnboarding, fetchLikedBooks, fetchOnboarding, saveInterests } from '../api/profile'
import { type BookSummary, fetchBooks, setBookPreference } from '../api/library'
import { useAuth } from '../auth/useAuth'
import { Button } from '../components/ui/Button'
import { ReadingInterests } from '../components/ReadingInterests'
import { childBookAccess } from '../lib/childBookAccess'

type Step = 'welcome' | 'books' | 'interests'

// Book choices stay on the assigned shelf. Public catalogue access is
// explained from the child's permissions, and opened only after setup.
export function ChildOnboarding({ name }: { name: string }) {
  const navigate = useNavigate()
  const { user } = useAuth()
  const access = childBookAccess(user ?? {})
  const [step, setStep] = useState<Step>('welcome')
  const [books, setBooks] = useState<BookSummary[] | null>(null)
  const [liked, setLiked] = useState<number[]>([])
  const [subjects, setSubjects] = useState<string[]>([])
  const [savedSubjects, setSavedSubjects] = useState<string[]>([])
  const [attempt, setAttempt] = useState(0)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [pendingLikes, setPendingLikes] = useState<number[]>([])
  const [error, setError] = useState<string | null>(null)
  const steps: Step[] = ['welcome', ...(books?.length ? ['books' as const] : []), 'interests']
  const index = steps.indexOf(step)
  const findRoute = user?.canDiscover ? '/discover' : user?.canRequest ? '/requests' : null
  const findLabel = user?.canDiscover ? 'Explore books' : 'Search and ask'

  useEffect(() => {
    let cancelled = false
    Promise.all([fetchBooks('recent', 1, 50, { mine: true }), fetchOnboarding(), fetchLikedBooks()])
      .then(([page, profile, preferences]) => {
        if (cancelled) return
        setBooks(page.items)
        setSubjects(profile.interests)
        setSavedSubjects(profile.interests)
        setLiked(preferences.items.map((book) => book.bookId))
      })
      .catch((caught: unknown) => {
        if (!cancelled) setLoadError(caught instanceof ApiError ? caught.message : 'Could not load your setup. Try again.')
      })
    return () => { cancelled = true }
  }, [attempt])

  async function finish(destination = '/') {
    if (busy || pendingLikes.length > 0) return
    setBusy(true)
    setError(null)
    try {
      if (JSON.stringify(subjects) !== JSON.stringify(savedSubjects)) {
        await saveInterests(subjects)
        setSavedSubjects(subjects)
      }
      await completeOnboarding()
      navigate(destination, { replace: true, state: { fromOnboarding: true } })
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not finish setup')
    } finally {
      setBusy(false)
    }
  }

  async function skip() {
    if (busy || pendingLikes.length > 0) return
    setBusy(true)
    setError(null)
    try {
      // Skipping never replaces the stored interest selection.
      await completeOnboarding()
      navigate('/', { replace: true, state: { fromOnboarding: true } })
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not skip setup. Try again.')
    } finally {
      setBusy(false)
    }
  }

  async function toggleLike(book: BookSummary) {
    if (pendingLikes.includes(book.id)) return
    const wasLiked = liked.includes(book.id)
    setPendingLikes((ids) => [...ids, book.id])
    setLiked((ids) => wasLiked ? ids.filter((id) => id !== book.id) : [...ids, book.id])
    setError(null)
    try {
      await setBookPreference(book.id, wasLiked ? null : 'liked')
    } catch {
      setLiked((ids) => wasLiked ? [...ids, book.id] : ids.filter((id) => id !== book.id))
      setError('Could not save that pick. Try again.')
    } finally {
      setPendingLikes((ids) => ids.filter((id) => id !== book.id))
    }
  }

  return (
    <main className="mx-auto flex min-h-screen max-w-xl flex-col px-5 py-10">
      <div className="flex items-baseline justify-between gap-3">
        <p className="font-sans text-[11px] uppercase tracking-[0.18em] text-ink-muted">
          Welcome to Bokhylle · Step {index + 1}{books !== null ? ' of ' + steps.length : ''}
        </p>
        <button type="button" disabled={busy || pendingLikes.length > 0} onClick={() => void skip()} className="font-sans text-[11px] font-medium uppercase tracking-[0.16em] text-ink-muted transition-colors hover:text-ink disabled:opacity-50">Skip</button>
      </div>

      {step === 'welcome' && (
        <>
          <h1 className="mt-6 font-display text-display text-ink">Hi, {name}</h1>
          <p className="mt-2 text-sm text-ink-muted">This is your shelf. An adult adds the books you can read; you pick what you like.</p>
          <div className="mt-6 border-t border-line pt-5">
            <h2 className="font-display text-lg text-ink">Finding your next book</h2>
            <p className="mt-2 text-sm text-ink-muted">{access === 'explore'
              ? 'Explore Discover and suggestions, open book details, and tap Ask an adult when you find something you want.'
              : access === 'search'
                ? 'Heard about a book? Open Requests, search by title, author or ISBN, and tap Ask an adult.'
                : access === 'browse-only'
                  ? 'Explore Discover and suggestions. If you find a book you want, ask an adult to add it to your shelf.'
                  : 'An adult chooses books for your shelf. Tell them what you would like to read next.'}</p>
            {user?.canRequest && <p className="mt-2 text-sm text-ink-muted">Asking does not add a book immediately. An administrator must approve it before it appears on your shelf.</p>}
          </div>
          {books?.length === 0 && <p className="mt-6 text-sm text-ink-muted">Your shelf is waiting for its first books. Ask an adult to add some so you can start reading.</p>}
          {books === null && !loadError && <p role="status" className="mt-6 text-sm text-ink-muted">Loading your shelf and interests…</p>}
          {loadError && <div role="alert" className="mt-6">
            <p className="text-sm text-danger">{loadError}</p>
            <Button variant="ghost" size="sm" onClick={() => { setLoadError(null); setAttempt((value) => value + 1) }}>Try again</Button>
          </div>}
        </>
      )}

      {step === 'books' && (
        <>
          <h1 className="mt-6 font-display text-display text-ink">Pick books you like</h1>
          <p className="mt-2 text-sm text-ink-muted">Only books on your shelf show up here.</p>
          <ul className="mt-6 divide-y divide-line">
            {books?.map((book) => (
              <li key={book.id} className="flex items-center justify-between gap-3 py-3">
                <span className="min-w-0">
                  <span className="block truncate text-sm text-ink">{book.title}</span>
                  <span className="block truncate text-xs text-ink-muted">{book.authors.join(', ')}</span>
                </span>
                <Button variant={liked.includes(book.id) ? 'primary' : 'ghost'} size="sm" className="shrink-0" disabled={pendingLikes.includes(book.id)} onClick={() => void toggleLike(book)}>{liked.includes(book.id) ? 'Liked' : 'Love it'}</Button>
              </li>
            ))}
          </ul>
        </>
      )}

      {step === 'interests' && (
        <>
          <h1 className="mt-6 font-display text-display text-ink">What do you like reading?</h1>
          <p className="mt-2 text-sm text-ink-muted">{user?.canDiscover ? 'Pick what you enjoy to help us suggest books.' : 'Pick what you enjoy to help us arrange your shelf as it grows.'} Choose your favourites first.</p>
          <div className="mt-6"><ReadingInterests selected={subjects} onChange={setSubjects} disabled={busy} /></div>
        </>
      )}

      {error && <p role="alert" className="mt-4 text-sm text-danger">{error}</p>}
      <div className="mt-8 flex flex-wrap items-center justify-between gap-3">
        <Button variant="ghost" size="sm" disabled={index === 0 || busy || pendingLikes.length > 0} onClick={() => setStep(steps[index - 1])}>Back</Button>
        {step !== 'interests' ? (
          <Button variant="primary" disabled={books === null || busy || pendingLikes.length > 0} onClick={() => setStep(steps[index + 1])}>Next</Button>
        ) : (
          <div className="flex flex-wrap gap-2">
            {findRoute && <Button variant="secondary" disabled={busy} onClick={() => void finish(findRoute)}>{findLabel}</Button>}
            {!findRoute && <Button variant="secondary" disabled={busy} onClick={() => void finish('/library')}>Open my shelf</Button>}
            <Button variant="primary" disabled={busy} onClick={() => void finish()}>{busy ? 'Saving…' : 'Finish'}</Button>
          </div>
        )}
      </div>
    </main>
  )
}

import { useEffect, useState } from 'react'
import { ApiError } from '../../api/client'
import { type BookSummary, coverUrl, fetchBooks, searchBooks } from '../../api/library'
import { BookCover } from '../../components/BookCover'
import { Button } from '../../components/ui/Button'
import { Input } from '../../components/ui/Field'

type Results = { key: string; items: BookSummary[]; total: number; error: string | null }

export function ChildStartingBooks({ selected, onChange, disabled = false }: {
  selected: BookSummary[]
  onChange: (books: BookSummary[]) => void
  disabled?: boolean
}) {
  const [query, setQuery] = useState('')
  const [page, setPage] = useState(1)
  const [attempt, setAttempt] = useState(0)
  const [results, setResults] = useState<Results | null>(null)
  const trimmed = query.trim()
  const key = `${trimmed}|${page}|${attempt}`
  const current = results?.key === key ? results : null

  useEffect(() => {
    let cancelled = false
    const timer = setTimeout(() => {
      const request = trimmed
        ? searchBooks(trimmed, { mine: false }).then((items) => ({ items, total: items.length }))
        : fetchBooks('title', page, 24, { mine: false })
      request.then(({ items, total }) => {
        if (!cancelled) setResults({ key, items, total, error: null })
      }).catch((error: unknown) => {
        if (!cancelled) setResults({ key, items: [], total: 0, error: error instanceof ApiError ? error.message : 'Could not load household books.' })
      })
    }, trimmed ? 200 : 0)
    return () => { cancelled = true; clearTimeout(timer) }
  }, [trimmed, page, key])

  function toggle(book: BookSummary) {
    onChange(selected.some((item) => item.id === book.id)
      ? selected.filter((item) => item.id !== book.id)
      : [...selected, book])
  }

  return (
    <div className="space-y-4">
      <label className="block">
        <span className="mb-1.5 block text-xs text-ink-muted">Search household books</span>
        <Input value={query} disabled={disabled} onChange={(event) => { setQuery(event.target.value); setPage(1) }} placeholder="Title, author or ISBN" />
      </label>
      <p role="status" className="text-xs text-ink-muted">{selected.length} {selected.length === 1 ? 'book selected' : 'books selected'}</p>
      {selected.length > 0 && (
        <ul className="space-y-1 border-b border-line pb-3" aria-label="Selected starting books">
          {selected.map((book) => (
            <li key={book.id} className="flex items-center justify-between gap-3 text-sm">
              <span className="min-w-0 break-words text-ink">{book.title}</span>
              <Button size="sm" variant="ghost" disabled={disabled} aria-label={`Remove ${book.title}`} onClick={() => toggle(book)}>Remove</Button>
            </li>
          ))}
        </ul>
      )}
      {!current ? <p role="status" className="py-5 text-sm text-ink-muted">Loading books…</p> : current.error ? (
        <div role="alert">
          <p className="text-sm text-danger">{current.error}</p>
          <Button variant="ghost" size="sm" disabled={disabled} onClick={() => setAttempt((value) => value + 1)}>Try again</Button>
        </div>
      ) : current.items.length === 0 ? (
        <p className="py-5 text-sm text-ink-muted">{trimmed
          ? 'No household books match that search.'
          : 'The household library is empty. Import some books first, or set up this shelf later.'}</p>
      ) : (
        <ul className="divide-y divide-line" aria-label="Available household books">
          {current.items.map((book) => (
            <li key={book.id}>
              <label className="flex cursor-pointer items-center gap-3 py-3">
                <input type="checkbox" disabled={disabled} checked={selected.some((item) => item.id === book.id)} onChange={() => toggle(book)} className="h-4 w-4 shrink-0 accent-accent" />
                <BookCover src={coverUrl(book.id)} className="h-14 w-10 shrink-0" />
                <span className="min-w-0">
                  <span className="block break-words text-sm text-ink">{book.title}</span>
                  <span className="block text-xs text-ink-muted">{book.authors.join(', ')}</span>
                </span>
              </label>
            </li>
          ))}
        </ul>
      )}
      {!trimmed && current && current.total > 24 && (
        <div className="flex items-center justify-between">
          <Button variant="ghost" size="sm" disabled={disabled || page === 1} onClick={() => setPage((value) => value - 1)}>Previous</Button>
          <span className="text-xs text-ink-muted">Page {page} of {Math.ceil(current.total / 24)}</span>
          <Button variant="ghost" size="sm" disabled={disabled || page * 24 >= current.total} onClick={() => setPage((value) => value + 1)}>Next page</Button>
        </div>
      )}
    </div>
  )
}

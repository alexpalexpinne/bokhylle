import { useState } from 'react'
import { createPortal } from 'react-dom'
import { ApiError } from '../../api/client'
import {
  type CollectionSummary,
  addBookToCollection,
  createCollection,
  fetchBookCollections,
  fetchCollections,
  removeBookFromCollection,
} from '../../api/collections'
import { Button } from '../../components/ui/Button'
import { Modal } from '../../components/ui/Modal'

export function CollectionsManager({ bookId, onError }: {
  bookId: number
  onError: (message: string) => void
}) {
  const [open, setOpen] = useState(false)
  const [collections, setCollections] = useState<CollectionSummary[]>([])
  const [memberships, setMemberships] = useState<Set<number>>(new Set())
  const [name, setName] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function openCollections() {
    setError(null)
    setName('')
    try {
      const [all, current] = await Promise.all([fetchCollections(), fetchBookCollections(bookId)])
      setCollections(all)
      setMemberships(new Set(current.map((collection) => collection.id)))
      setOpen(true)
    } catch (caught) {
      onError(caught instanceof ApiError ? caught.message : 'Could not load collections')
    }
  }

  async function save() {
    setBusy(true)
    setError(null)
    try {
      const original = await fetchBookCollections(bookId)
      const originalIds = new Set(original.map((collection) => collection.id))
      for (const collection of collections) {
        if (memberships.has(collection.id) && !originalIds.has(collection.id)) {
          await addBookToCollection(collection.id, bookId)
        }
      }
      for (const collection of original) {
        if (!memberships.has(collection.id)) {
          await removeBookFromCollection(collection.id, bookId)
        }
      }
      setOpen(false)
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not update collections')
    } finally {
      setBusy(false)
    }
  }

  async function create() {
    if (!name.trim()) return
    setBusy(true)
    setError(null)
    try {
      const collection = await createCollection(name.trim())
      setCollections((current) => [...current, collection].sort((a, b) => a.name.localeCompare(b.name)))
      setMemberships((current) => new Set(current).add(collection.id))
      setName('')
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not create the shelf')
    } finally {
      setBusy(false)
    }
  }

  return (
    <>
      <Button variant="ghost" size="sm" onClick={() => void openCollections()}>
        Manage collections
      </Button>
      {open && createPortal(
        <Modal
          title="Collections"
          description="Group this book with others. Collections are shared across the household."
          onClose={() => setOpen(false)}
          footer={
            <>
              <Button variant="ghost" onClick={() => setOpen(false)}>Cancel</Button>
              <Button variant="primary" disabled={busy} onClick={() => void save()}>
                {busy ? 'Saving…' : 'Save'}
              </Button>
            </>
          }
        >
          {error && <p className="mb-4 rounded-card bg-surface-2 px-3.5 py-2.5 text-sm text-danger">{error}</p>}
          <div className="space-y-1">
            {collections.map((collection) => (
              <label
                key={collection.id}
                className="flex cursor-pointer items-center gap-3 rounded-card bg-surface-2 px-4 py-2.5"
              >
                <input
                  type="checkbox"
                  checked={memberships.has(collection.id)}
                  onChange={() => setMemberships((current) => {
                    const next = new Set(current)
                    if (next.has(collection.id)) {
                      next.delete(collection.id)
                    } else {
                      next.add(collection.id)
                    }
                    return next
                  })}
                  className="h-4 w-4 accent-[var(--color-accent)]"
                />
                <span className="text-sm text-ink">{collection.name}</span>
                <span className="ml-auto text-xs text-ink-faint">
                  {collection.bookCount} {collection.bookCount === 1 ? 'book' : 'books'}
                </span>
              </label>
            ))}
            {collections.length === 0 && <p className="text-sm text-ink-muted">No collections yet — create one below.</p>}
          </div>
          <div className="mt-4 flex gap-2">
            <input
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder="New collection name"
              className="min-w-0 flex-1 rounded-card bg-surface-2 px-3.5 py-2.5 text-sm text-ink outline-none placeholder:text-ink-faint focus-visible:outline-2 focus-visible:outline-focus"
            />
            <Button variant="secondary" disabled={busy || !name.trim()} onClick={() => void create()}>Add</Button>
          </div>
        </Modal>,
        document.body,
      )}
    </>
  )
}

import { useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import { Users } from 'lucide-react'
import { type ShelfUser, fetchBookShelfUsers, setBookShelf } from '../../api/users'
import { Button } from '../../components/ui/Button'
import { Modal } from '../../components/ui/Modal'
import { useMutation } from '../../lib/useMutation'

export function HouseholdAccess({ bookId, menu = false }: { bookId: number; menu?: boolean }) {
  const [open, setOpen] = useState(false)
  const [users, setUsers] = useState<ShelfUser[] | null>(null)
  const mutation = useMutation()

  useEffect(() => {
    let cancelled = false
    fetchBookShelfUsers(bookId)
      .then((shelf) => {
        if (!cancelled) setUsers(shelf.users)
      })
      .catch((caught: unknown) => console.warn('book_detail.read_failed', caught))
    return () => {
      cancelled = true
    }
  }, [bookId])

  return (
    <>
      {menu ? (
        <button
          type="button"
          onClick={() => setOpen(true)}
          className="flex w-full items-center gap-2 rounded-[3px] px-3 py-2 text-left text-sm text-ink-soft transition-colors hover:bg-surface-3 hover:text-ink"
        >
          <Users size={14} aria-hidden />
          Manage access…
        </button>
      ) : (
        <Button variant="ghost" size="sm" onClick={() => setOpen(true)}>
          <Users size={15} aria-hidden />
          Manage access
        </Button>
      )}
      {open && createPortal(
        <Modal
          title="Available to"
          description="Choose who sees this book on their shelf. Children only see what is assigned to them."
          onClose={() => setOpen(false)}
          footer={<Button variant="ghost" onClick={() => setOpen(false)}>Done</Button>}
        >
          {users && users.length > 0 ? (
            <div className="space-y-1">
              {users.map((person) => (
                <button
                  key={person.userId}
                  type="button"
                  aria-pressed={person.onShelf}
                  disabled={mutation.busyKey === `person-${person.userId}`}
                  onClick={() => void mutation.run(
                    `person-${person.userId}`,
                    () => setBookShelf(person.userId, bookId, !person.onShelf),
                    'Could not update that shelf',
                    () => setUsers((current) => current?.map((entry) =>
                      entry.userId === person.userId ? { ...entry, onShelf: !entry.onShelf } : entry,
                    ) ?? null),
                  )}
                  className="flex w-full items-center gap-3 rounded-card px-3 py-2.5 text-left transition-colors hover:bg-surface-2"
                >
                  <span
                    className={`h-1.5 w-1.5 shrink-0 rounded-full ${person.onShelf ? 'bg-accent' : 'border border-line'}`}
                    aria-hidden
                  />
                  <span className="flex-1 text-sm text-ink">{person.displayName}</span>
                  <span className={`font-sans text-[10px] uppercase tracking-[0.14em] ${person.onShelf ? 'text-accent' : 'text-ink-faint'}`}>
                    {person.onShelf ? 'On their shelf' : 'Not assigned'}
                  </span>
                </button>
              ))}
            </div>
          ) : <p className="text-sm text-ink-muted">No child profiles yet.</p>}
          {mutation.error && <p className="mt-3 border-l-2 border-danger pl-3 text-sm text-danger">{mutation.error}</p>}
        </Modal>,
        document.body,
      )}
    </>
  )
}

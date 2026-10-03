import { useState } from 'react'
import { createPortal } from 'react-dom'
import { LockKeyhole, UsersRound } from 'lucide-react'
import { setBookSharing, type BookDetail } from '../../api/library'
import { BookSharingChoice, type BookSharing } from '../../components/BookSharingChoice'
import { BookSharingMarker } from '../../components/BookSharingMarker'
import { Button } from '../../components/ui/Button'
import { Modal } from '../../components/ui/Modal'
import { useMutation } from '../../lib/useMutation'

export function BookSharingSettings({ book, onUpdated }: { book: BookDetail; onUpdated: () => void }) {
  const [open, setOpen] = useState(false)
  const [draft, setDraft] = useState<BookSharing>('shared')
  const mutation = useMutation()
  const label = book.sharedInHousehold ? 'Shared with household' : 'Private'
  const VisibilityIcon = book.sharedInHousehold ? UsersRound : LockKeyhole
  if (!book.sharing) return <BookSharingMarker value={book.sharedInHousehold ? 'shared' : 'private'} label={`Book sharing: ${label}`} />
  return (
    <>
      <button type="button" aria-label={`Book sharing: ${label}. Change your sharing`} aria-haspopup="dialog" aria-expanded={open}
        title={`${label} · Change your sharing`}
        className="inline-flex h-11 w-11 shrink-0 items-center justify-center rounded-[3px] text-ink-soft transition-colors hover:bg-surface-2 hover:text-ink focus-visible:outline-2 focus-visible:outline-focus"
        onClick={() => { mutation.clearError(); setDraft(book.sharing ?? 'shared'); setOpen(true) }}>
        <VisibilityIcon size={18} aria-hidden />
      </button>
      {open && createPortal(<Modal title="Book sharing" description="Choose whether you share this book with the household. Your personal shelf stays private." onClose={() => { if (!mutation.busyKey) setOpen(false) }} footer={<>
        <Button variant="ghost" disabled={!!mutation.busyKey} onClick={() => setOpen(false)}>Cancel</Button>
        <Button variant="primary" disabled={!!mutation.busyKey || draft === book.sharing} onClick={() => void mutation.run('sharing', () => setBookSharing(book.id, draft), 'Could not update sharing', () => { setOpen(false); onUpdated() })}>{mutation.busyKey ? 'Saving…' : 'Save sharing'}</Button>
      </>}>
        <BookSharingChoice value={draft} disabled={!!mutation.busyKey} onChange={setDraft} />
        {draft === 'private' && book.sharing !== 'private' && <p className="mt-4 text-sm text-ink-muted">Your choice changes only your sharing. Other owners and explicitly assigned children keep access. If another owner shares this book, it stays shared with the household.</p>}
        {book.sharing === 'private' && book.sharedInHousehold && <p className="mt-4 text-sm text-ink-muted">Your addition is private. Another owner shares this book, so it is still available in the household collection.</p>}
        {mutation.error && <p role="alert" className="mt-4 text-sm text-danger">{mutation.error}</p>}
      </Modal>, document.body)}
    </>
  )
}

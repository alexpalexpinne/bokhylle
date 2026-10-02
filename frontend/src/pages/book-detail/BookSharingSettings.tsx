import { useState } from 'react'
import { Lock, Users } from 'lucide-react'
import { setBookSharing, type BookDetail } from '../../api/library'
import { BookSharingChoice, type BookSharing } from '../../components/BookSharingChoice'
import { Button } from '../../components/ui/Button'
import { Modal } from '../../components/ui/Modal'
import { useMutation } from '../../lib/useMutation'

export function BookSharingSettings({ book, onUpdated }: { book: BookDetail; onUpdated: () => void }) {
  const [open, setOpen] = useState(false)
  const [draft, setDraft] = useState<BookSharing>('shared')
  const mutation = useMutation()
  if (!book.sharing) return null
  return (
    <>
      <Button variant="ghost" size="sm" onClick={() => { setDraft(book.sharing ?? 'shared'); setOpen(true) }}>
        {book.sharing === 'private' ? <Lock size={14} aria-hidden /> : <Users size={14} aria-hidden />}
        {book.sharing === 'private' ? 'Private' : 'Shared'} · Change
      </Button>
      {open && <Modal title="Book sharing" description="Choose whether you share this book with the household. Your personal shelf stays private." onClose={() => { if (!mutation.busyKey) setOpen(false) }} footer={<>
        <Button variant="ghost" disabled={!!mutation.busyKey} onClick={() => setOpen(false)}>Cancel</Button>
        <Button variant="primary" disabled={!!mutation.busyKey || draft === book.sharing} onClick={() => void mutation.run('sharing', () => setBookSharing(book.id, draft), 'Could not update sharing', () => { setOpen(false); onUpdated() })}>{mutation.busyKey ? 'Saving…' : 'Save sharing'}</Button>
      </>}>
        <BookSharingChoice value={draft} disabled={!!mutation.busyKey} onChange={setDraft} />
        {book.sharing === 'private' && book.sharedInHousehold && <p className="mt-4 text-sm text-ink-muted">Another reader shares this book, so it is still available in the household collection.</p>}
        {mutation.error && <p role="alert" className="mt-4 text-sm text-danger">{mutation.error}</p>}
      </Modal>}
    </>
  )
}

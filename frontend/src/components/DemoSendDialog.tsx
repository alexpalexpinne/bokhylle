import { useState } from 'react'
import { createPortal } from 'react-dom'
import { useNavigate } from 'react-router-dom'
import { Send } from 'lucide-react'
import { ApiError } from '../api/client'
import { sendDemoBook } from '../api/demo'
import { coverUrl } from '../api/library'
import { BookCover } from './BookCover'
import { Button } from './ui/Button'
import { Modal } from './ui/Modal'

export function DemoSendDialog({ bookId, title, onClose }: { bookId: number; title: string; onClose: () => void }) {
  const navigate = useNavigate()
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function send() {
    if (busy) return
    setBusy(true)
    setError(null)
    try {
      await sendDemoBook(bookId)
      navigate('/activity')
      onClose()
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not start the demo delivery. Try again.')
      setBusy(false)
    }
  }

  return createPortal(
    <Modal
      title="Send to Demo Kindle"
      description="Try sending a book to a Kindle. This is a simulation."
      onClose={onClose}
      footer={<>
        <Button variant="ghost" onClick={onClose}>Cancel</Button>
        <Button variant="primary" disabled={busy} onClick={() => void send()}>
          <Send size={14} aria-hidden /> {busy ? 'Preparing…' : 'Simulate send'}
        </Button>
      </>}
    >
      <div className="flex items-start gap-5">
        <BookCover src={coverUrl(bookId)} loading="eager" className="h-32 w-[85px] shrink-0 rounded-[3px] shadow-card" />
        <div className="min-w-0">
          <p className="font-display text-xl text-ink">{title}</p>
          <p className="mt-2 text-xs uppercase tracking-[0.14em] text-ink-muted">EPUB → Demo Kindle</p>
          <p className="mt-4 text-sm leading-relaxed text-ink-soft">Follow the book from preparation to delivery in Activity. No file or email is sent to a real device.</p>
        </div>
      </div>
      {error && <p role="alert" className="mt-5 border-l-2 border-danger pl-3 text-sm text-danger">{error}</p>}
    </Modal>,
    document.body,
  )
}

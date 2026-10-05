import { useEffect, useRef, useState } from 'react'
import { X } from 'lucide-react'
import { ApiError } from '../../api/client'
import { Button } from './Button'

/** Keep confirmation and undo visible even when a suggestion leaves its rail. */
export function ActionNotice({ message, onUndo, onDismiss }: { message: string; onUndo: () => Promise<void>; onDismiss: () => void }) {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const undoRef = useRef<HTMLButtonElement>(null)
  useEffect(() => {
    const frame = requestAnimationFrame(() => {
      if (document.activeElement === document.body) undoRef.current?.focus({ preventScroll: true })
    })
    return () => cancelAnimationFrame(frame)
  }, [])
  async function undo() {
    setBusy(true)
    setError(null)
    try { await onUndo(); onDismiss() }
    catch (caught) { setError(caught instanceof ApiError ? caught.message : 'Could not undo this change. Please try again.') }
    finally { setBusy(false) }
  }
  return <aside aria-label="Change confirmation" className="fixed inset-x-4 bottom-[calc(5rem+env(safe-area-inset-bottom))] z-40 mx-auto max-w-xl rounded-[3px] border border-line bg-surface p-4 shadow-modal md:bottom-6">
    <div className="flex items-start gap-3">
      <div className="min-w-0 flex-1"><p role="status" className="text-sm text-ink">{message}</p>{error && <p role="alert" className="mt-2 text-sm text-danger">{error}</p>}</div>
      <Button ref={undoRef} variant="ghost" size="sm" className="min-h-12" disabled={busy} onClick={() => void undo()}>{busy ? 'Undoing…' : 'Undo'}</Button>
      <Button variant="ghost" size="sm" className="min-h-12 min-w-12 px-2" aria-label="Dismiss confirmation" disabled={busy} onClick={onDismiss}><X size={16} aria-hidden /></Button>
    </div>
  </aside>
}

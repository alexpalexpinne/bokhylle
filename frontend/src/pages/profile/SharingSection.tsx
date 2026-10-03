import { useState } from 'react'
import { ApiError } from '../../api/client'
import { updateProfile } from '../../api/profile'
import { useAuth } from '../../auth/useAuth'
import { BookSharingChoice, type BookSharing } from '../../components/BookSharingChoice'
import { Button } from '../../components/ui/Button'

export function SharingSection() {
  const { user, refresh } = useAuth()
  const [draft, setDraft] = useState<BookSharing | null>(null)
  const [saving, setSaving] = useState(false)
  const [message, setMessage] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const saved = user?.defaultBookSharing ?? 'shared'
  const value = draft ?? saved

  async function save() {
    setSaving(true)
    setError(null)
    setMessage(null)
    try {
      await updateProfile({ defaultBookSharing: value })
      await refresh()
      setDraft(null)
      setMessage('Default saved. Existing books keep their sharing settings.')
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not save your sharing default')
    } finally { setSaving(false) }
  }

  return (
    <section className="mt-10 max-w-2xl border-t border-line pt-6" aria-labelledby="sharing-default-heading">
      <h2 id="sharing-default-heading" className="font-display text-title text-ink">New book sharing</h2>
      <p className="mt-2 text-sm text-ink-muted">Choose how books you get will start. Open the Private or Shared marker on a book you acquired to change its sharing afterward. Adding someone else’s shared book to your shelf does not give you control over its sharing.</p>
      <div className="mt-4 max-w-md"><BookSharingChoice value={value} label="Default for new books" disabled={saving} onChange={(next) => { setDraft(next); setMessage(null) }} /></div>
      <Button className="mt-4" variant="primary" disabled={saving || value === saved} onClick={() => void save()}>{saving ? 'Saving…' : 'Save sharing default'}</Button>
      {message && <p role="status" className="mt-3 text-sm text-ink-soft">{message}</p>}
      {error && <p role="alert" className="mt-3 text-sm text-danger">{error}</p>}
    </section>
  )
}

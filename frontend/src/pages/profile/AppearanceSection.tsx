import { useCallback, useState } from 'react'
import { useBeforeUnload, useBlocker } from 'react-router-dom'
import { ApiError } from '../../api/client'
import { updateProfile } from '../../api/profile'
import type { User } from '../../auth/context'
import { useAuth } from '../../auth/useAuth'
import { BrandMark } from '../../components/BrandMark'
import { ShelfDecoration, ShelfSurface } from '../../components/ShelfStructure'
import { shelfFinish, type ShelfFinish } from '../../lib/appearance'
import { Button } from '../../components/ui/Button'
import { Modal } from '../../components/ui/Modal'

type Appearance = { shelfFinish: ShelfFinish; shelfDecorations: boolean; spotlightRotation: boolean }

const FINISHES = [
  { value: 'oak', label: 'Light oak', description: 'Warm wood with a quiet grain.' },
  { value: 'black', label: 'Faded black', description: 'Soft charcoal with a matte edge.' },
  { value: 'metal', label: 'Muted metal', description: 'Neutral grey with a fine highlight.' },
] as const

export function AppearanceSection({ user }: { user: User }) {
  const { refresh } = useAuth()
  const [edits, setEdits] = useState<Partial<Appearance>>({})
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const saved: Appearance = {
    shelfFinish: shelfFinish(user.shelfFinish),
    shelfDecorations: user.shelfDecorations ?? true,
    spotlightRotation: user.spotlightRotation ?? true,
  }
  const draft = { ...saved, ...edits }
  const changed = draft.shelfFinish !== saved.shelfFinish || draft.shelfDecorations !== saved.shelfDecorations || draft.spotlightRotation !== saved.spotlightRotation
  const blocker = useBlocker(changed && !saving)
  useBeforeUnload(useCallback((event) => {
    if (changed && !saving) { event.preventDefault(); event.returnValue = '' }
  }, [changed, saving]))

  function edit(update: Partial<Appearance>) {
    setEdits((current) => ({ ...current, ...update }))
    setNotice(null)
  }

  async function save() {
    setSaving(true)
    setError(null)
    try {
      await updateProfile(draft)
      await refresh()
      setEdits({})
      setNotice('Appearance saved. These choices follow your profile on every device.')
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not save your appearance')
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="max-w-2xl">
      <fieldset disabled={saving}>
        <legend className="font-display text-title text-ink">Shelf finish</legend>
        <p className="mt-2 text-sm text-ink-muted">The same material on your featured shelf and book rails.</p>
        <div className="mt-5 grid gap-3 sm:grid-cols-3">
          {FINISHES.map(({ value, label, description }) => (
            <label key={value} data-shelf-finish={value} className={`cursor-pointer rounded-[4px] border p-4 ${draft.shelfFinish === value ? 'border-accent' : 'border-line'}`}>
              <span className="relative mb-4 block h-2"><ShelfSurface /></span>
              <span className="flex items-center gap-2 text-sm font-medium text-ink">
                <input type="radio" name="shelf-finish" value={value} checked={draft.shelfFinish === value} onChange={() => edit({ shelfFinish: value })} className="accent-[var(--color-accent)]" />
                {label}
              </span>
              <span className="mt-2 block text-xs leading-5 text-ink-muted">{description}</span>
            </label>
          ))}
        </div>
        <div data-shelf-finish={draft.shelfFinish} className="shelf-preview mt-6 border-b border-line pb-8">
          <p className="mb-4 text-xs text-ink-muted">Shelf preview</p>
          <div className="spotlight-display" data-decorated={draft.shelfDecorations}>
            <div className="spotlight-cover"><span className="flex items-center justify-center bg-surface-2"><BrandMark className="h-12 w-12 text-ink-faint" /></span></div>
            <span className="spotlight-upright"><ShelfSurface upright /></span>
            {draft.shelfDecorations && <ShelfDecoration />}
            <ShelfSurface />
          </div>
        </div>
        <label className="mt-5 flex min-h-11 cursor-pointer items-start gap-3 py-2">
          <input type="checkbox" checked={draft.shelfDecorations} onChange={(event) => edit({ shelfDecorations: event.target.checked })} className="mt-1 h-4 w-4 shrink-0 accent-[var(--color-accent)]" />
          <span><span className="block text-sm font-medium text-ink">Decorate the featured shelf</span><span className="mt-1 block text-xs leading-5 text-ink-muted">A small vase and branch, adapted to your finish and screen size.</span></span>
        </label>
        <label className="mt-2 flex min-h-11 cursor-pointer items-start gap-3 py-2">
          <input type="checkbox" checked={draft.spotlightRotation} onChange={(event) => edit({ spotlightRotation: event.target.checked })} className="mt-1 h-4 w-4 shrink-0 accent-[var(--color-accent)]" />
          <span><span className="block text-sm font-medium text-ink">Automatically rotate Spotlight</span><span className="mt-1 block text-xs leading-5 text-ink-muted">Change books every ten seconds. Rotation waits while you interact and stays off when your device requests reduced motion.</span></span>
        </label>
      </fieldset>
      {error && <p role="alert" className="mt-4 text-sm text-danger">{error}</p>}
      {notice && <p role="status" className="mt-4 text-sm text-ink-soft">{notice}</p>}
      <div className="mt-6 flex items-center gap-4">
        <Button variant="primary" disabled={!changed || saving} onClick={() => void save()}>{saving ? 'Saving…' : 'Save appearance'}</Button>
        {changed && <span className="text-xs text-ink-muted">Unsaved changes</span>}
      </div>
      {blocker.state === 'blocked' && (
        <Modal title="Discard unsaved appearance?" onClose={() => blocker.reset()}>
          <p className="text-sm text-ink-soft">Save your appearance before leaving, or discard these changes.</p>
          <div className="mt-6 flex justify-end gap-2">
            <Button onClick={() => blocker.reset()}>Keep editing</Button>
            <Button variant="danger" onClick={() => blocker.proceed()}>Discard changes</Button>
          </div>
        </Modal>
      )}
    </div>
  )
}

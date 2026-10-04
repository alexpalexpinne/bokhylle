import { useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import { ApiError } from '../api/client'
import { fetchReleases, type ReleasePreview } from '../api/discover'
import { useAuth } from '../auth/useAuth'
import { Button, ButtonLink } from './ui/Button'
import { Modal } from './ui/Modal'
import { ReleaseVersionDetails } from './ReleaseVersionDetails'

export type VersionSelection = { context: string; key: string }
type SearchError = { kind: 'setup' | 'retry' | 'blocked'; message: string }

/** Browsing and choosing a version are read-only. Get starts acquisition. */
export function ReleasePreviewChoices({ provider, providerKey, format, context, selected, disabled, onChange }: {
  provider: string
  providerKey: string
  format: string
  context: string
  selected: string | null
  disabled: boolean
  onChange: (selection: VersionSelection | null) => void
}) {
  const [releases, setReleases] = useState<ReleasePreview[] | null>(null)
  const { user } = useAuth()
  const [error, setError] = useState<SearchError | null>(null)
  const [retry, setRetry] = useState(0)
  const [pickerOpen, setPickerOpen] = useState(false)
  const [draft, setDraft] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    fetchReleases(providerKey, format, provider)
      .then((response) => {
        if (cancelled) return
        const choices = response.releases.filter((release) => !release.rejected && !!release.selectionKey)
        setReleases(choices)
        const recommended = choices.find((release) => release.recommended && !release.needsReview)
        onChange(recommended?.selectionKey ? { context, key: recommended.selectionKey } : null)
      })
      .catch((caught) => {
        if (cancelled) return
        if (caught instanceof ApiError && caught.code === 'indexer_not_configured') {
          setError({ kind: 'setup', message: user?.role === 'admin'
            ? 'Book downloading isn’t set up yet.'
            : 'An administrator needs to enable book downloading.' })
        } else if (caught instanceof ApiError && [400, 401, 403, 404].includes(caught.status)) {
          setError({ kind: 'blocked', message: caught.message })
        } else {
          setError({ kind: 'retry', message: 'Couldn’t search for versions.' })
        }
      })
    return () => { cancelled = true }
  }, [provider, providerKey, format, context, retry, user?.role, onChange])

  const chosen = releases?.find((release) => release.selectionKey === selected)
  const suitable = releases?.filter((release) => !release.needsReview) ?? []
  const possible = releases?.filter((release) => release.needsReview) ?? []
  function searchAgain() {
    onChange(null)
    setError(null)
    setReleases(null)
    setRetry((value) => value + 1)
  }
  function openPicker() {
    setDraft(selected)
    setPickerOpen(true)
  }

  return <section aria-label="Download version" className="mt-6 min-w-0">
    <h3 className="font-sans text-[11px] uppercase tracking-[0.16em] text-ink-muted">Version</h3>
    {error ? <div role={error.kind === 'setup' ? 'status' : 'alert'} className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1">
      <p className={`text-sm ${error.kind === 'setup' ? 'text-ink-muted' : 'text-danger'}`}>{error.message}</p>
      {error.kind === 'setup' && user?.role === 'admin' && <ButtonLink variant="ghost" size="sm" to="/settings/getting-books">Set up downloading</ButtonLink>}
      {error.kind === 'retry' && <Button variant="ghost" size="sm" disabled={disabled} onClick={searchAgain}>Try again</Button>}
    </div> : !releases ? <p role="status" className="mt-2 text-sm text-ink-muted">Finding available versions…</p>
      : releases.length === 0 ? <p role="status" className="mt-2 text-sm text-ink-muted">No matching version found.</p>
        : <div className="mt-2 space-y-2">
        {chosen ? <>
          <ReleaseVersionDetails release={chosen} compact />
          {chosen.needsReview && <p className="text-xs text-ink-muted">Book identity is uncertain. Check the title and author before downloading.</p>}
        </> : <p role="status" className="text-sm text-ink-muted">No confident match found. Review the possible matches before downloading.</p>}
        {(!chosen || releases.length > 1 || chosen.needsReview) && <Button variant="ghost" size="sm" className="min-h-11 -ml-3.5" disabled={disabled} onClick={openPicker}>
          {chosen ? releases.length > 1 ? 'Change version' : 'Review version' : 'Review possible matches'}
          {releases.length > 1 && <span className="text-ink-muted">({chosen ? releases.length - 1 : releases.length})</span>}
        </Button>}
      </div>}
    {pickerOpen && releases && createPortal(<Modal title="Available versions" description="Choose a file, then use Get or Get & send on the book."
      onClose={() => setPickerOpen(false)}
      footer={<>
        <Button variant="ghost" onClick={() => setPickerOpen(false)}>Cancel</Button>
        <Button variant="primary" disabled={!draft || disabled} onClick={() => {
          if (draft) onChange({ context, key: draft })
          setPickerOpen(false)
        }}>Use this version</Button>
      </>}>
      <fieldset disabled={disabled} className="min-w-0 space-y-5">
        <legend className="sr-only">Download version</legend>
        {[{ title: 'Matching versions', choices: suitable }, { title: 'Possible matches', choices: possible }].filter((group) => group.choices.length > 0).map((group) =>
          <div key={group.title}>
            {group.title === 'Possible matches' && <div className="mb-2 space-y-1">
              <h3 className="text-sm font-medium text-ink">Possible matches</h3>
              <p className="text-xs text-ink-muted">We couldn’t confidently identify these as this book. Check the title and author.</p>
            </div>}
            <div className="divide-y divide-line">
              {group.choices.map((release) => <label key={release.selectionKey!} className="flex min-h-12 cursor-pointer items-start gap-3 py-3">
                <input type="radio" name={`version-${context}`} value={release.selectionKey!}
                  checked={draft === release.selectionKey} onChange={() => setDraft(release.selectionKey!)}
                  className="mt-1 h-4 w-4 shrink-0 accent-accent focus-visible:outline-2 focus-visible:outline-focus" />
                <ReleaseVersionDetails release={release} />
              </label>)}
            </div>
          </div>)}
      </fieldset>
    </Modal>, document.body)}
  </section>
}

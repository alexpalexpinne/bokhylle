import { useEffect, useState } from 'react'
import { ApiError } from '../api/client'
import { fetchReleases, type ReleasePreview } from '../api/discover'
import { availabilityLabel, formatBytes } from '../api/acquisitions'
import { useAuth } from '../auth/useAuth'
import { Button, ButtonLink } from './ui/Button'

export type VersionSelection = { context: string; key: string }
type SearchError = { kind: 'setup' | 'retry' | 'blocked'; message: string }

/** Browsing is read-only; the parent starts acquisition only after Get. */
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
  const [expanded, setExpanded] = useState(false)
  useEffect(() => {
    let cancelled = false
    fetchReleases(providerKey, format, provider)
      .then((response) => { if (!cancelled) setReleases(response.releases) })
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
  }, [provider, providerKey, format, context, retry, user?.role])

  const shown = expanded ? releases : releases?.slice(0, 6)
  return (
    <fieldset className="mt-6 min-w-0" disabled={disabled}>
      <legend className="font-sans text-[11px] uppercase tracking-[0.16em] text-ink-muted">Available versions</legend>
      {error ? <div role={error.kind === 'setup' ? 'status' : 'alert'} className="mt-3 flex flex-wrap items-center gap-x-3 gap-y-1">
        <p className={`text-sm ${error.kind === 'setup' ? 'text-ink-muted' : 'text-danger'}`}>{error.message}</p>
        {error.kind === 'setup' && user?.role === 'admin' && <ButtonLink variant="ghost" size="sm" to="/settings/getting-books">Set up downloading</ButtonLink>}
        {error.kind === 'retry' && <Button variant="ghost" size="sm" onClick={() => { onChange(null); setError(null); setReleases(null); setRetry((value) => value + 1) }}>Try again</Button>}
      </div> : !releases ? <p role="status" className="mt-3 text-sm text-ink-muted">Finding available versions…</p>
        : releases.length === 0 ? <p role="status" className="mt-3 text-sm text-ink-muted">No versions found for this book.</p>
          : <>
            <p className="mt-2 text-xs text-ink-muted">Select a version, then Get for your shelf or send it to your reader.</p>
            <div className="mt-2 divide-y divide-line">
              {shown?.map((release, index) => {
                const availability = availabilityLabel(release.seeders, release.method)
                const selectable = !!release.selectionKey && !release.rejected
                return <label key={release.selectionKey ?? `${release.releaseName}-${index}`}
                  className={`flex min-h-12 items-start gap-3 py-3 ${selectable ? 'cursor-pointer' : 'text-ink-muted'} ${disabled ? 'opacity-60' : ''}`}>
                  <input type="radio" name={`version-${context}`} value={release.selectionKey ?? ''}
                    checked={!!release.selectionKey && selected === release.selectionKey}
                    disabled={!selectable || disabled}
                    onChange={() => { if (release.selectionKey) onChange({ context, key: release.selectionKey }) }}
                    className="mt-1 h-4 w-4 shrink-0 accent-accent focus-visible:outline-2 focus-visible:outline-focus" />
                  <span className="min-w-0 flex-1">
                    <span className="block text-sm font-medium [overflow-wrap:anywhere]">{release.releaseName || 'Unnamed release'}</span>
                    <span className="mt-1 block text-xs text-ink-muted">
                      {release.recommended && <span className="mr-2 font-medium text-accent-strong">Recommended</span>}
                      {(release.format ?? 'unknown format').toUpperCase()}{release.language ? ` · ${release.language.toUpperCase()}` : ' · Language unknown'} · {formatBytes(release.sizeBytes)}
                      {release.isCollection ? ' · complete collection' : ''}
                    </span>
                    <span className={`mt-1 block text-xs ${availability.className}`}>
                      {release.method === 'nzb' ? 'Usenet' : release.method === 'http' ? 'Direct download' : 'Torrent'} · {availability.label}
                      {release.method === 'torrent' && typeof release.seeders === 'number' ? ` · ${release.seeders} seeders` : ''}
                      {release.indexer ? ` · ${release.indexer}` : ''}
                    </span>
                    {release.unavailableReason && <span className="mt-1 block text-xs text-ink-muted">{release.unavailableReason}</span>}
                  </span>
                </label>
              })}
            </div>
            {!expanded && releases.length > 6 && <Button variant="ghost" size="sm" onClick={() => setExpanded(true)}>Show {releases.length - 6} more versions</Button>}
          </>}
    </fieldset>
  )
}

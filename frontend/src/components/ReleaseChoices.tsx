import { useEffect, useState } from 'react'
import { Link } from 'react-router-dom'
import { ApiError } from '../api/client'
import {
  type Acquisition, type Candidate, availabilityLabel, fetchAcquisition,
  fetchCandidates, formatBytes, selectCandidate, statusLabel,
} from '../api/acquisitions'
import { useAuth } from '../auth/useAuth'
import { Button } from './ui/Button'

/** The same selectable list in Discover, book details and Activity. */
export function ReleaseChoices({ acquisitionId, onSelected, onBusyChange }: {
  acquisitionId: string
  onSelected: () => void
  onBusyChange?: (busy: boolean) => void
}) {
  const { user } = useAuth()
  const [acquisition, setAcquisition] = useState<Acquisition | null>(null)
  const [candidates, setCandidates] = useState<Candidate[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [selecting, setSelecting] = useState<number | null>(null)
  const [retry, setRetry] = useState(0)

  useEffect(() => {
    let cancelled = false
    let timer: ReturnType<typeof setTimeout> | undefined
    async function load() {
      try {
        const current = await fetchAcquisition(acquisitionId)
        if (cancelled) return
        setAcquisition(current)
        setError(null)
        const canChoose = user?.role === 'admin' || current.requestedByUserId === user?.id
        if (current.status === 'NEEDS_SELECTION' && canChoose) {
          const choices = await fetchCandidates(acquisitionId)
          if (!cancelled) setCandidates(choices)
        } else if (['REQUESTED', 'SEARCHING', 'EVALUATING'].includes(current.status)) {
          timer = setTimeout(() => void load(), 1000)
        }
      } catch (caught) {
        if (!cancelled) setError(caught instanceof ApiError ? caught.message : 'Could not load versions')
      }
    }
    void load()
    return () => { cancelled = true; clearTimeout(timer) }
  }, [acquisitionId, retry, user?.id, user?.role])

  async function choose(index: number) {
    setSelecting(index)
    setError(null)
    onBusyChange?.(true)
    try {
      await selectCandidate(acquisitionId, index)
      onSelected()
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not select this version')
    } finally {
      setSelecting(null)
      onBusyChange?.(false)
    }
  }

  const searching = !acquisition || ['REQUESTED', 'SEARCHING', 'EVALUATING'].includes(acquisition.status)
  const canChoose = user?.role === 'admin' || acquisition?.requestedByUserId === user?.id
  return (
    <section aria-label="Available versions" className="space-y-4">
      {error && <div role="alert" className="space-y-2 text-sm text-danger">
        <p>{error}</p>
        {!candidates && <Button variant="ghost" size="sm" onClick={() => setRetry((value) => value + 1)}>Try again</Button>}
      </div>}
      {!error && searching && <p role="status" className="text-sm text-ink-muted">
        {!acquisition ? 'Checking download status…' : !canChoose
          ? 'Finding versions for an existing shared request. Its original requester keeps control of the selection.'
          : acquisition.askBeforeDownload
            ? 'Finding available versions… You will choose before anything downloads.'
            : 'Finding the best suitable file for the existing download request…'}
      </p>}
      {acquisition && !searching && (acquisition.status !== 'NEEDS_SELECTION' || !canChoose) && <div role="status" className="space-y-2 text-sm text-ink-muted">
        <p>{!canChoose && ['NEEDS_SELECTION', 'QUEUED', 'DOWNLOADING'].includes(acquisition.status)
          ? 'This book already has a shared download in progress. Its original requester keeps control of the selection.'
          : acquisition.errorMessage || statusLabel(acquisition)}</p>
        <Link to="/activity" className="text-accent hover:text-accent-strong">Follow in Activity</Link>
      </div>}
      {acquisition?.status === 'NEEDS_SELECTION' && canChoose && <>
        <p className="text-sm text-ink-muted">Pick the torrent or file to download. Existing library files will be kept.</p>
        {!candidates && !error && <p role="status" className="text-sm text-ink-muted">Loading versions…</p>}
        {candidates?.length === 0 && <p className="text-sm text-ink-muted">No versions are available.</p>}
        <div className="divide-y divide-line">
          {candidates?.map((candidate, index) => {
            const availability = availabilityLabel(candidate.seeders, candidate.method)
            return <div key={candidate.index} className="flex items-start gap-3 py-4">
              <div className="min-w-0 flex-1">
                <p className="text-sm font-medium text-ink [overflow-wrap:anywhere]">{candidate.releaseName || 'Unnamed release'}</p>
                <p className="mt-1.5 text-xs text-ink-muted">
                  {index === 0 && !candidate.rejected && <span className="mr-2 font-medium text-accent-strong">Recommended</span>}
                  {(candidate.format ?? 'unknown').toUpperCase()}{candidate.language ? ` · ${candidate.language.toUpperCase()}` : ''} · {formatBytes(candidate.sizeBytes)}
                  {candidate.isCollection ? ' · complete collection' : ''}
                </p>
                <p className={`mt-1 text-xs ${availability.className}`}>{availability.label}</p>
                <p className="mt-1 text-xs text-ink-muted [overflow-wrap:anywhere]">
                  {candidate.method === 'nzb' ? 'Usenet' : candidate.method === 'http' ? 'Direct download' : 'Torrent'}
                  {candidate.method === 'torrent' && typeof candidate.seeders === 'number' ? ` · ${candidate.seeders} seeders` : ''}
                  {candidate.indexer ? ` · ${candidate.indexer}` : ''}
                </p>
                {candidate.rejected && <p className="mt-1 text-xs text-ink-muted">Unavailable for this request.</p>}
              </div>
              <Button size="sm" className="min-h-11 shrink-0" variant="primary" disabled={candidate.rejected || selecting !== null} onClick={() => void choose(candidate.index)}>
                {selecting === candidate.index ? 'Choosing…' : 'Get this version'}
              </Button>
            </div>
          })}
        </div>
      </>}
    </section>
  )
}

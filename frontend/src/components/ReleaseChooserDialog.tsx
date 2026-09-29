import { useEffect, useState } from 'react'
import { ApiError } from '../api/client'
import {
  type Candidate,
  availabilityLabel,
  fetchCandidates,
  formatBytes,
  selectCandidate,
} from '../api/acquisitions'
import { Button } from './ui/Button'
import { Modal } from './ui/Modal'

type ReleaseChooserDialogProps = {
  acquisitionId: string
  onClose: () => void
  onSelected: () => void
}

export function ReleaseChooserDialog({
  acquisitionId,
  onClose,
  onSelected,
}: ReleaseChooserDialogProps) {
  const [candidates, setCandidates] = useState<Candidate[]>([])
  const [error, setError] = useState<string | null>(null)
  const [selecting, setSelecting] = useState<number | null>(null)
  const [loaded, setLoaded] = useState(false)

  useEffect(() => {
    let cancelled = false

    fetchCandidates(acquisitionId)
      .then((items) => {
        if (!cancelled) {
          setCandidates(items)
        }
      })
      .catch((caught: unknown) => {
        if (!cancelled) {
          setError(caught instanceof ApiError ? caught.message : 'Could not load versions')
        }
      })
      .finally(() => {
        if (!cancelled) {
          setLoaded(true)
        }
      })

    return () => {
      cancelled = true
    }
  }, [acquisitionId])

  async function choose(index: number) {
    setSelecting(index)
    setError(null)
    try {
      await selectCandidate(acquisitionId, index)
      onSelected()
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not select this version')
      setSelecting(null)
    }
  }

  return (
    <Modal
      title="Choose a version"
      description="We found several versions. Pick the one you want."
      onClose={onClose}
      footer={
        <Button variant="ghost" onClick={onClose}>
          Close
        </Button>
      }
    >
      {error && (
        <p className="mb-4 rounded-card bg-surface-2 px-3.5 py-2.5 text-sm text-danger">{error}</p>
      )}

      <div className="space-y-2">
        {!loaded && <p className="text-sm text-ink-muted">Loading versions…</p>}
        {loaded && candidates.length === 0 && !error && (
          <p className="text-sm text-ink-muted">No versions are available.</p>
        )}
        {candidates.map((candidate, index) => {
          const availability = availabilityLabel(candidate.seeders, candidate.method)
          const recommended = index === 0 && !candidate.rejected
          return (
            <div
              key={candidate.index}
              className="flex items-start justify-between gap-3 rounded-card bg-surface-2 px-4 py-3"
            >
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  {recommended && (
                    <span className="rounded-[3px] bg-accent/15 px-2 py-0.5 font-sans text-[10px] font-medium uppercase tracking-[0.14em] text-accent">
                      Recommended
                    </span>
                  )}
                  <p className="min-w-0 truncate text-sm font-medium text-ink">
                    {(candidate.format ?? 'unknown').toUpperCase()}
                    {candidate.language ? ` · ${candidate.language.toUpperCase()}` : ''} ·{' '}
                    {formatBytes(candidate.sizeBytes)}
                    {candidate.isCollection ? ' · complete collection' : ''}
                  </p>
                </div>
                <p className="mt-1 text-xs">
                  <span className={availability.className}>{availability.label}</span>
                </p>
                <details className="mt-1.5 text-xs text-ink-faint">
                  <summary className="cursor-pointer list-none underline-offset-2 transition-colors hover:text-ink [&::-webkit-details-marker]:hidden">
                    Source details
                  </summary>
                  <p className="mt-1">
                    {candidate.releaseName ?? 'Release'}
                    {typeof candidate.seeders === 'number' ? ` · ${candidate.seeders} seeders` : ''}
                    {candidate.leechers ? ` · ${candidate.leechers} leechers` : ''}
                    {candidate.indexer ? ` · ${candidate.indexer}` : ''}
                  </p>
                </details>
              </div>
              <Button
                size="sm"
                variant="primary"
                disabled={candidate.rejected || selecting !== null}
                onClick={() => void choose(candidate.index)}
              >
                {selecting === candidate.index ? 'Choosing…' : 'Choose'}
              </Button>
            </div>
          )
        })}
      </div>
    </Modal>
  )
}

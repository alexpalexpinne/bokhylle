import { useEffect, useState } from 'react'
import { type ReviewCandidate, type ReviewDetail, basename, errorMessage, fetchReview, resolveReview } from '../api/review'
import { formatBytes } from '../api/acquisitions'
import { Button } from './ui/Button'
import { Modal } from './ui/Modal'

type ReviewDialogProps = {
  acquisitionId: string
  onClose: () => void
  onResolved: () => void
}

export function ReviewDialog({ acquisitionId, onClose, onResolved }: ReviewDialogProps) {
  const [detail, setDetail] = useState<ReviewDetail | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [loaded, setLoaded] = useState(false)

  useEffect(() => {
    let cancelled = false

    fetchReview(acquisitionId)
      .then((value) => {
        if (!cancelled) {
          setDetail(value)
        }
      })
      .catch((caught: unknown) => {
        if (!cancelled) {
          setError(errorMessage(caught, 'Could not load review items'))
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

  async function act(action: 'choose' | 'retry' | 'ignore', path?: string) {
    setBusy(true)
    setError(null)
    try {
      await resolveReview(acquisitionId, action, path)
      onResolved()
    } catch (caught) {
      setError(errorMessage(caught, 'Could not resolve this review'))
      setBusy(false)
    }
  }

  const candidates: ReviewCandidate[] =
    detail && 'candidates' in detail && Array.isArray(detail.candidates)
      ? detail.candidates.filter((candidate): candidate is ReviewCandidate =>
          candidate !== null &&
          typeof candidate === 'object' &&
          typeof candidate.path === 'string' &&
          typeof candidate.format === 'string' &&
          typeof candidate.size === 'number' &&
          typeof candidate.confidence === 'number' &&
          Array.isArray(candidate.reasons),
        )
      : []

  return (
    <Modal
      title="Review download"
      description="We could not confidently identify this download. Import a file, retry identification, or ignore it. Nothing is deleted while under review."
      onClose={onClose}
      wide
      footer={
        <>
          <Button variant="secondary" disabled={busy} onClick={() => void act('retry')}>
            Retry identification
          </Button>
          <Button variant="danger" disabled={busy} onClick={() => void act('ignore')}>
            Ignore (keep files)
          </Button>
        </>
      }
    >
      {error && (
        <p className="mb-4 rounded-card bg-surface-2 px-3.5 py-2.5 text-sm text-danger">{error}</p>
      )}

      {!loaded && <p className="text-sm text-ink-muted">Loading files…</p>}

      {loaded && candidates.length === 0 && !error && (
        <p className="text-sm text-ink-muted">
          No candidate files were recorded for this download.
        </p>
      )}

      <div className="space-y-2">
        {candidates.map((candidate) => (
          <div
            key={candidate.path}
            className="flex items-center justify-between gap-3 rounded-card bg-surface-2 px-4 py-3"
          >
            <div className="min-w-0">
              <p className="truncate text-sm font-medium text-ink">{basename(candidate.path)}</p>
              <p className="truncate text-xs text-ink-faint">
                {candidate.format.toUpperCase()} · {formatBytes(candidate.size)}
                {candidate.title ? ` · ${candidate.title}` : ''}
              </p>
              <p className="text-xs text-ink-faint">
                confidence {candidate.confidence.toFixed(2)}
                {candidate.reasons.length > 0 ? ` · ${candidate.reasons.join(', ')}` : ''}
              </p>
            </div>
            <Button
              size="sm"
              variant="primary"
              disabled={busy}
              onClick={() => void act('choose', candidate.path)}
            >
              Import this file
            </Button>
          </div>
        ))}
      </div>
    </Modal>
  )
}

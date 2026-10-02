import { type DirectActivity, fetchDirectActivity } from '../api/acquisitions'
import { retryDelivery } from '../api/delivery'
import { type ReactNode, useCallback, useEffect, useState } from 'react'
import { useSearchParams } from 'react-router-dom'
import { AlertCircle, Check, Loader2, RefreshCw, XCircle } from 'lucide-react'
import { ApiError } from '../api/client'
import { EmptyState } from '../components/ui/EmptyState'
import { ScopeTabs } from '../components/ui/ScopeTabs'
import {
  type Acquisition,
  type AcquisitionDiagnostics,
  type AttentionItem,
  type AcquisitionStatus,
  cancelAcquisition,
  fetchAcquisitions,
  inspectAcquisition,
  retryAcquisition,
  fetchAttention,
  fetchDiagnostics,
  formatBytes,
  setKeepLooking,
  isActiveStatus,
  statusLabel,
} from '../api/acquisitions'
import { coverUrl } from '../api/library'
import { BookCover } from '../components/BookCover'
import { ReleaseChooserDialog } from '../components/ReleaseChooserDialog'
import { ReviewDialog } from '../components/ReviewDialog'
import { Button, ButtonLink } from '../components/ui/Button'
import { PageHeader } from '../components/ui/PageHeader'
import { SectionMark } from '../components/ui/SectionMark'
import { useAuth } from '../auth/useAuth'
import { useMutation } from '../lib/useMutation'

type Group = 'attention' | 'working' | 'finished'

function groupOf(status: AcquisitionStatus): Group {
  switch (status) {
    case 'NEEDS_REVIEW':
    case 'NEEDS_SELECTION':
      return 'attention'
    case 'READY':
    case 'CANCELLED':
    case 'NO_RELEASE_FOUND':
    case 'DOWNLOAD_FAILED':
    case 'IMPORT_FAILED':
      return 'finished'
    default:
      return 'working'
  }
}

function statusPhrase(acquisition: Acquisition): string {
  if (acquisition.status === 'NEEDS_SELECTION') {
    return 'Several versions to choose from'
  }
  return statusLabel(acquisition)
}

const STAGES = ['FOUND', 'FETCHING', 'SHELVING', 'READY', 'DELIVERED'] as const

type StageInfo = {
  current: number
  failed: number | null
  details: (string | null)[]
}

/// Maps an acquisition onto the five narrative stages, so the page reads as
/// a journey (finding, fetching, shelving, ready, delivered) instead of a
/// progress bar with a status code.
function stageInfo(acquisition: Acquisition): StageInfo {
  const release =
    [acquisition.selectedReleaseFormat?.toUpperCase(), acquisition.selectedReleaseName]
      .filter(Boolean)
      .join(' · ') || null
  const percent = `${Math.round(acquisition.progress)}%`

  switch (acquisition.status) {
    case 'REQUESTED':
    case 'SEARCHING':
    case 'EVALUATING':
      return { current: 0, failed: null, details: ['searching available sources', null, null, null, null] }
    case 'NO_RELEASE_FOUND':
      // Searched, but nothing was found; stage 01 stays FINDING.
      return { current: 0, failed: 0, details: ['no copy found', null, null, null, null] }
    case 'NEEDS_SELECTION':
      return { current: 0, failed: null, details: ['choose a version', null, null, null, null] }
    case 'NEEDS_REVIEW':
      // Review happens after the download, while files are inspected.
      return {
        current: 2,
        failed: null,
        details: [release, null, 'needs your review', null, null],
      }
    case 'QUEUED':
    case 'DOWNLOADING':
      return { current: 1, failed: null, details: [release, percent, null, null, null] }
    case 'DOWNLOAD_FAILED':
      return {
        current: 1,
        failed: 1,
        details: [release, acquisition.errorMessage ?? 'download failed', null, null, null],
      }
    case 'DOWNLOADED':
    case 'INSPECTING':
    case 'IDENTIFIED':
    case 'IMPORTING':
      return { current: 2, failed: null, details: [release, null, 'verifying and filing', null, null] }
    case 'IMPORT_FAILED':
      return {
        current: 2,
        failed: 2,
        details: [release, null, acquisition.errorMessage ?? 'import failed', null, null],
      }
    case 'READY': {
      const shelf = [release, null, null, 'on your shelf', null] as (string | null)[]
      switch (acquisition.deliveryStatus) {
        case 'SENT':
          return { current: 4, failed: null, details: [...shelf.slice(0, 4), 'delivered to your reader'] }
        case 'FAILED':
          return {
            current: 4,
            failed: 4,
            details: [...shelf.slice(0, 4), acquisition.errorMessage ?? 'delivery failed'],
          }
        case 'PENDING':
          return { current: 3, failed: null, details: [...shelf.slice(0, 4), 'delivering…'] }
        default:
          return { current: 3, failed: null, details: shelf }
      }
    }
    case 'CANCELLED':
      return { current: -1, failed: null, details: [] }
  }
}

function requestCode(id: string): string {
  return id.replace(/-/g, '').slice(-6).toUpperCase()
}

function Stages({ acquisition }: { acquisition: Acquisition }) {
  const info = stageInfo(acquisition)
  if (info.current < 0) {
    return null
  }
  const searching =
    acquisition.status === 'REQUESTED' ||
    acquisition.status === 'SEARCHING' ||
    acquisition.status === 'EVALUATING' ||
    acquisition.status === 'NO_RELEASE_FOUND'

  const lastVisible = info.failed ?? info.current
  const remaining = STAGES.slice(lastVisible + 1)

  return (
    <ol className="mt-3 space-y-1.5">
      {STAGES.slice(0, lastVisible + 1).map((label, index) => {
        const failed = info.failed === index
        const done = info.current > index
        const active = info.current === index
        const labelState = failed
          ? 'text-danger'
          : active
            ? 'text-ink'
            : done
              ? 'text-ink-soft'
              : 'text-ink-faint'
        const shownLabel = index === 0 && searching && active ? 'FINDING' : label
        const detail = info.details[index]

        return (
          <li key={label} className="grid grid-cols-[1.6rem_6.5rem_1fr] items-baseline gap-x-3">
            <span className="font-sans text-[10px] tabular-nums tracking-[0.16em] text-ink-faint">
              {String(index + 1).padStart(2, '0')}
            </span>
            <span
              className={`font-sans text-[11px] uppercase tracking-[0.16em] ${labelState}`}
            >
              {shownLabel}
            </span>
            <span className="min-w-0 truncate text-xs text-ink-muted">
              {active && acquisition.status === 'DOWNLOADING' ? (
                <span className="flex items-center gap-3">
                  <span className="relative h-0.5 w-32 overflow-hidden bg-line">
                    <span
                      className="absolute inset-y-0 left-0 bg-accent"
                      style={{ width: `${Math.round(acquisition.progress)}%` }}
                    />
                  </span>
                  <span className="font-sans text-[11px] tabular-nums text-ink-soft">
                    {Math.round(acquisition.progress)}%
                  </span>
                  {acquisition.downloadSpeed !== null && acquisition.downloadSpeed > 0 && (
                    <span className="text-ink-faint">
                      {formatBytes(acquisition.downloadSpeed)}/s
                    </span>
                  )}
                </span>
              ) : (
                (detail ?? (done ? '' : active ? '' : 'waiting'))
              )}
            </span>
          </li>
        )
      })}
      {remaining.length > 0 && (
        <li className="pl-[2.6rem] font-sans text-[10px] uppercase tracking-[0.16em] text-ink-faint">
          then {remaining.join(' · ')}
        </li>
      )}
    </ol>
  )
}

function formatWhen(seconds: number): string {
  const date = new Date(seconds * 1000)
  const minutes = Math.round((Date.now() - date.getTime()) / 60000)
  if (minutes < 1) {
    return 'just now'
  }
  if (minutes < 60) {
    return `${minutes} min ago`
  }
  const hours = Math.round(minutes / 60)
  if (hours < 24) {
    return `${hours} h ago`
  }
  return date.toLocaleDateString(undefined, { day: 'numeric', month: 'short' })
}

export function Downloads() {
  const { user } = useAuth()
  const [searchParams, setSearchParams] = useSearchParams()
  const isAdmin = user?.role === 'admin'

  const [acquisitions, setAcquisitions] = useState<Acquisition[]>([])
  const [householdScope, setHouseholdScope] = useState(false)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [choosing, setChoosing] = useState<string | null>(null)
  const [reviewing, setReviewing] = useState<string | null>(null)
  const [direct, setDirect] = useState<DirectActivity[]>([])
  const [attentionItems, setAttentionItems] = useState<AttentionItem[]>([])
  const directMutation = useMutation()
  const [refreshToken, setRefreshToken] = useState(0)
  const [refreshing, setRefreshing] = useState(false)

  const refresh = useCallback(() => {
    setRefreshing(true)
    setRefreshToken((token) => token + 1)
  }, [])

  useEffect(() => {
    let cancelled = false

    fetchAcquisitions(50, householdScope)
      .then((items) => {
        if (!cancelled) {
          setAcquisitions(items)
          setError(null)
        }
      })
      .catch((caught: unknown) => {
        if (!cancelled) {
          setError(caught instanceof ApiError ? caught.message : 'Failed to load activity')
        }
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false)
          setRefreshing(false)
        }
      })

    return () => {
      cancelled = true
    }
  }, [refreshToken, householdScope])

  useEffect(() => {
    if (!isAdmin) {
      return
    }
    let cancelled = false
    fetchAttention()
      .then((data) => {
        if (!cancelled) {
          setAttentionItems(data.items)
        }
      })
      .catch((caught: unknown) => console.warn('activity.read_failed', caught))
    return () => {
      cancelled = true
    }
  }, [isAdmin, refreshToken])

  useEffect(() => {
    const hasActive = acquisitions.some((acquisition) => isActiveStatus(acquisition.status))
    if (!hasActive) {
      return
    }

    const timer = setInterval(refresh, 3000)
    return () => clearInterval(timer)
  }, [acquisitions, refresh])

  async function inspect(id: string) {
    setError(null)
    try {
      await inspectAcquisition(id)
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not inspect the download')
    } finally {
      refresh()
    }
  }

  async function retry(id: string) {
    setError(null)
    try {
      await retryAcquisition(id)
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not retry this request')
    } finally {
      refresh()
    }
  }

  async function keepLooking(id: string, enabled: boolean) {
    setError(null)
    try {
      await setKeepLooking(id, enabled)
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not update this request')
    } finally {
      refresh()
    }
  }

  async function cancel(id: string) {
    try {
      await cancelAcquisition(id)
      refresh()
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not cancel the download')
    }
  }

  const canManage = (acquisition: Acquisition) =>
    isAdmin || acquisition.requestedByUserId === user?.id || acquisition.managedByMe

  const requestedChoice = acquisitions.find((acquisition) => acquisition.id === searchParams.get('choose'))
  const choosingId = choosing ?? (requestedChoice?.status === 'NEEDS_SELECTION' && canManage(requestedChoice) ? requestedChoice.id : null)
  function closeChooser() {
    setChoosing(null)
    if (searchParams.has('choose')) {
      const next = new URLSearchParams(searchParams)
      next.delete('choose')
      setSearchParams(next, { replace: true })
    }
  }

  // Anything already represented by the higher-level inbox stays out of the
  // generic groups, so one problem never appears twice on the page.
  const attentionIds = new Set(attentionItems.map((item) => item.id))
  const remaining = acquisitions.filter((item) => !attentionIds.has(item.id))
  const attention = remaining.filter((item) => groupOf(item.status) === 'attention')
  const working = remaining.filter((item) => groupOf(item.status) === 'working')
  const finished = remaining.filter((item) => groupOf(item.status) === 'finished')

  function loadDirect() {
    fetchDirectActivity()
      .then((data) => setDirect(data.items))
      .catch((caught: unknown) => console.warn('activity.read_failed', caught))
  }

  useEffect(() => {
    loadDirect()
  }, [])

  const rowProps = {
    isAdmin,
    canManage,
    onCancel: (id: string) => void cancel(id),
    onRetry: (id: string) => void retry(id),
    onInspect: (id: string) => void inspect(id),
    onReview: (id: string) => setReviewing(id),
    onChoose: (id: string) => setChoosing(id),
    onKeepLooking: (id: string, enabled: boolean) => void keepLooking(id, enabled),
  }

  return (
    <section>
      <PageHeader
        eyebrow="Activity"
        title="Your activity"
        description="What Bokhylle is doing for you, and what it has finished recently."
        actions={
          <>
          {isAdmin && (
            <div className="pr-2">
              <ScopeTabs
                ariaLabel="Activity scope"
                value={householdScope ? 'household' : 'mine'}
                onChange={(value) => setHouseholdScope(value === 'household')}
                options={[
                  { value: 'mine', label: 'My activity' },
                  { value: 'household', label: 'Household' },
                ]}
              />
            </div>
          )}
          <Button variant="ghost" size="sm" onClick={refresh} disabled={refreshing}>
            <RefreshCw size={14} aria-hidden className={refreshing ? 'animate-spin' : ''} />
            {refreshing ? 'Refreshing…' : 'Refresh'}
          </Button>
          </>
        }
      />

      {error && <p className="mt-6 border-l-2 border-danger pl-4 text-sm text-danger">{error}</p>}

      {requestedChoice && requestedChoice.status !== 'NEEDS_SELECTION' && <p role="status" className="mt-6 border-l-2 border-line pl-4 text-sm text-ink-muted">
        {requestedChoice.askBeforeDownload && ['REQUESTED', 'SEARCHING', 'EVALUATING'].includes(requestedChoice.status)
          ? 'Finding versions. You will choose before anything downloads.'
          : isActiveStatus(requestedChoice.status)
            ? 'This book already has a shared download in progress. Its current selection is kept.'
            : 'See the result of this request below.'}
      </p>}

      {loading && acquisitions.length === 0 && (
        <div className="mt-10 space-y-4">
          {Array.from({ length: 3 }).map((_, index) => (
            <div key={index} className="h-28 animate-pulse border-b border-line bg-surface/40" />
          ))}
        </div>
      )}

      {isAdmin && attentionItems.length > 0 && (
        <Section title="Needs attention" count={attentionItems.length}>
          {attentionItems.map((item) => (
            <div
              key={item.id}
              className="flex flex-wrap items-start justify-between gap-x-4 gap-y-2 border-t border-line px-1 py-3"
            >
              <div className="min-w-0">
                <h3 className="truncate font-display text-base text-ink">{item.title}</h3>
                <p className="truncate text-xs text-ink-muted">
                  {item.authors.length > 0 ? item.authors.join(', ') : 'Unknown author'}
                </p>
                <p className="mt-1 text-xs text-ink-faint">
                  {item.kind === 'review'
                    ? 'Several files were found — choose the right one.'
                    : item.kind === 'cancel_failed'
                      ? 'Bokhylle could not remove this download from your download client.'
                      : item.kind === 'path'
                        ? (item.errorMessage ??
                          'Bokhylle cannot reach the imported file. Check the download mapping.')
                        : (item.errorMessage ?? 'The import failed.')}
                </p>
              </div>
              <div className="flex shrink-0 items-center gap-2">
                {item.kind === 'review' ? (
                  <Button size="sm" variant="primary" onClick={() => setReviewing(item.id)}>
                    Review files
                  </Button>
                ) : item.kind === 'path' ? (
                  <ButtonLink to="/settings" variant="secondary" size="sm">
                    Fix integration
                  </ButtonLink>
                ) : item.kind === 'cancel_failed' ? (
                  <Button size="sm" variant="secondary" onClick={() => void cancel(item.id)}>
                    Retry cleanup
                  </Button>
                ) : (
                  <Button size="sm" variant="secondary" onClick={() => void inspect(item.id)}>
                    Choose file
                  </Button>
                )}
              </div>
            </div>
          ))}
        </Section>
      )}

      {attention.length > 0 && (
        <Section title="Waiting on you" count={attention.length}>
          {attention.map((acquisition) => (
            <AcquisitionRow key={acquisition.id} acquisition={acquisition} {...rowProps} />
          ))}
        </Section>
      )}

      {working.length > 0 && (
        <Section title="On the way" count={working.length}>
          {working.map((acquisition) => (
            <AcquisitionRow key={acquisition.id} acquisition={acquisition} {...rowProps} />
          ))}
        </Section>
      )}

      {(finished.length > 0 || direct.length > 0) && (
        <Section title="Finished" count={finished.length + direct.length}>
          {finished.map((acquisition) => (
            <AcquisitionRow key={acquisition.id} acquisition={acquisition} quiet {...rowProps} />
          ))}
          {direct.map((item) => (
            <div
              key={`${item.authorId}-${item.title}-${item.outcome}`}
              className="flex flex-wrap items-start justify-between gap-x-4 gap-y-1 border-t border-line px-1 py-3"
            >
              <div className="min-w-0">
                <h3 className="truncate font-display text-base text-ink-muted">{item.title}</h3>
                <p className="truncate text-xs text-ink-muted">
                  {item.authors.length > 0 ? item.authors.join(', ') : 'Unknown author'}
                </p>
                <p className="mt-1 truncate font-sans text-[10px] uppercase tracking-[0.16em] text-accent">
                  Automatic · Because you follow {item.authorName}
                </p>
                <p
                  className={`mt-0.5 text-xs ${
                    item.outcome === 'delivery_failed' ? 'text-danger' : 'text-ink-faint'
                  }`}
                >
                  {item.detail}
                </p>
                {item.outcome === 'delivery_failed' && item.deliveryId !== null && (
                  <button
                    type="button"
                    disabled={directMutation.busyKey === `direct-${item.deliveryId}`}
                    onClick={() =>
                      void directMutation.run(
                        `direct-${item.deliveryId}`,
                        () => retryDelivery(item.deliveryId as number),
                        'Could not retry that delivery',
                        loadDirect,
                      )
                    }
                    className="mt-1.5 font-sans text-[11px] font-medium uppercase tracking-[0.16em] text-accent transition-colors hover:text-accent-strong disabled:opacity-50"
                  >
                    Retry
                  </button>
                )}
                {item.outcome === 'delivery_failed' && directMutation.error && (
                  <p className="mt-1 text-xs text-danger">{directMutation.error}</p>
                )}
              </div>
            </div>
          ))}
        </Section>
      )}

      {!loading && acquisitions.length === 0 && direct.length === 0 && (
        <EmptyState
          className="mt-10"
          title="Nothing here yet"
          message="When you get a book, Bokhylle will show its progress and what it delivered for you."
          action={
            <ButtonLink to="/discover" variant="primary" size="md">
              Discover books
            </ButtonLink>
          }
        />
      )}

      {choosingId && (
        <ReleaseChooserDialog
          acquisitionId={choosingId}
          onClose={closeChooser}
          onSelected={() => {
            closeChooser()
            refresh()
          }}
        />
      )}

      {reviewing && (
        <ReviewDialog
          acquisitionId={reviewing}
          onClose={() => setReviewing(null)}
          onResolved={() => {
            setReviewing(null)
            refresh()
          }}
        />
      )}
    </section>
  )
}

function Section({
  title,
  count,
  children,
}: {
  title: string
  count: number
  children: ReactNode
}) {
  return (
    <div className="mt-12">
      <SectionMark
        title={title}
        action={<span className="font-sans text-xs text-ink-faint">{count}</span>}
      />
      <div className="mt-2 divide-y divide-line">{children}</div>
    </div>
  )
}

function StatusIcon({ status }: { status: AcquisitionStatus }) {
  const group = groupOf(status)
  if (group === 'attention') {
    return <AlertCircle size={14} className="shrink-0 text-warning" aria-hidden />
  }
  if (status === 'READY') {
    return <Check size={14} className="shrink-0 text-success" aria-hidden />
  }
  if (group === 'finished') {
    return (
      <XCircle
        size={14}
        className={`shrink-0 ${status === 'CANCELLED' ? 'text-ink-faint' : 'text-danger'}`}
        aria-hidden
      />
    )
  }
  return <Loader2 size={14} className="shrink-0 animate-spin text-accent" aria-hidden />
}

function AcquisitionRow({
  acquisition,
  quiet = false,
  isAdmin,
  canManage,
  onCancel,
  onRetry,
  onInspect,
  onReview,
  onChoose,
  onKeepLooking,
}: {
  acquisition: Acquisition
  quiet?: boolean
  isAdmin: boolean
  canManage: (acquisition: Acquisition) => boolean
  onCancel: (id: string) => void
  onRetry: (id: string) => void
  onInspect: (id: string) => void
  onReview: (id: string) => void
  onChoose: (id: string) => void
  onKeepLooking: (id: string, enabled: boolean) => void
}) {
  const manageable = canManage(acquisition)

  return (
    <article className={`transition-colors ${quiet ? 'py-3' : 'py-4'}`}>
      <div className="flex gap-4">
        <BookCover
          src={coverUrl(acquisition.bookId)}
          className={`shrink-0 self-start rounded-[3px] bg-surface-2 ${
            quiet ? 'aspect-[2/3] w-12 opacity-70' : 'aspect-[2/3] w-16 sm:w-20'
          }`}
        />

        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-start justify-between gap-x-4 gap-y-1">
            <div className="min-w-0">
              <h3
                className={`truncate font-display text-base ${quiet ? 'text-ink-muted' : 'text-ink'}`}
              >
                {acquisition.bookTitle}
              </h3>
              <p className="truncate text-xs text-ink-muted">
                {acquisition.bookAuthors.length > 0
                  ? acquisition.bookAuthors.join(', ')
                  : 'Unknown author'}
              </p>
              {acquisition.source === 'author_automation' && (
                <p className="mt-1 truncate font-sans text-[10px] uppercase tracking-[0.16em] text-accent">
                  Automatic
                  {acquisition.sourceAuthor ? ` · Because you follow ${acquisition.sourceAuthor}` : ''}
                </p>
              )}
            </div>
            {quiet ? (
              <span className="shrink-0 text-xs text-ink-faint">
                {formatWhen(acquisition.updatedAt)}
              </span>
            ) : (
              <span className="shrink-0 text-right">
                <span className="block font-sans text-[10px] uppercase tracking-[0.16em] text-ink-faint">
                  Request {requestCode(acquisition.id)}
                </span>
                {acquisition.requestedBy && (
                  <span className="mt-0.5 block text-xs text-ink-faint">
                    by {acquisition.requestedBy}
                  </span>
                )}
              </span>
            )}
          </div>

          {quiet &&
          acquisition.status !== 'NO_RELEASE_FOUND' &&
          acquisition.status !== 'DOWNLOAD_FAILED' &&
          acquisition.status !== 'IMPORT_FAILED' &&
          !(acquisition.status === 'READY' && acquisition.deliveryStatus !== 'NONE') ? (
            <div className="mt-1.5 flex items-center gap-2">
              <StatusIcon status={acquisition.status} />
              <span className="text-xs text-ink-faint">
                {acquisition.status === 'READY' && acquisition.deliveryStatus === 'SENT'
                  ? 'Delivered to your reader'
                  : acquisition.status === 'READY' && acquisition.deliveryStatus === 'FAILED'
                    ? 'Delivery failed'
                    : statusPhrase(acquisition)}
              </span>
            </div>
          ) : (
            <>
              <Stages acquisition={acquisition} />
              {acquisition.keepLooking ? (
                <p className="mt-2 text-xs text-ink-faint">
                  We&apos;re looking for this book — Bokhylle will keep checking and let you know
                  when a copy appears.
                </p>
              ) : (
                acquisition.status === 'NO_RELEASE_FOUND' && (
                  <p className="mt-2 text-xs text-ink-faint">
                    Not looking for a copy right now. Availability changes — you can try again
                    later.
                  </p>
                )
              )}
              {(acquisition.status === 'DOWNLOAD_FAILED' ||
                acquisition.status === 'IMPORT_FAILED' ||
                acquisition.status === 'NO_RELEASE_FOUND') &&
                manageable && (
                  <div className="mt-3 flex flex-wrap items-center gap-2">
                    <Button
                      size="sm"
                      variant="primary"
                      onClick={() => onRetry(acquisition.id)}
                    >
                      Try again
                    </Button>
                    {acquisition.status === 'IMPORT_FAILED' && isAdmin && (
                      <Button
                        size="sm"
                        variant="secondary"
                        onClick={() => onInspect(acquisition.id)}
                      >
                        Choose file
                      </Button>
                    )}
                    {acquisition.keepLooking ? (
                      <Button
                        size="sm"
                        variant="secondary"
                        onClick={() => onKeepLooking(acquisition.id, false)}
                      >
                        Stop looking
                      </Button>
                    ) : (
                      acquisition.status === 'NO_RELEASE_FOUND' && (
                        <Button
                          size="sm"
                          variant="secondary"
                          onClick={() => onKeepLooking(acquisition.id, true)}
                        >
                          Keep looking
                        </Button>
                      )
                    )}
                  </div>
                )}
              {acquisition.status === 'IMPORT_FAILED' && !manageable && (
                <p className="mt-2 text-xs text-ink-faint">
                  Ask {acquisition.requestedBy ?? 'the requester'} to try this request again.
                </p>
              )}
            </>
          )}

          {!quiet && (
            <div className="mt-3 flex flex-wrap items-center gap-2">
              {acquisition.status === 'NEEDS_REVIEW' && isAdmin && (
                <Button size="sm" variant="primary" onClick={() => onReview(acquisition.id)}>
                  Review files
                </Button>
              )}
              {acquisition.status === 'NEEDS_REVIEW' && !isAdmin && (
                <p className="text-xs text-ink-muted">
                  An administrator needs to review this download.
                </p>
              )}
              {acquisition.status === 'NEEDS_SELECTION' && manageable && (
                <Button size="sm" variant="primary" onClick={() => onChoose(acquisition.id)}>
                  Choose a version
                </Button>
              )}
              {acquisition.status === 'NEEDS_SELECTION' && !manageable && (
                <p className="text-xs text-ink-muted">
                  Waiting for {acquisition.requestedBy ?? 'the requester'} to choose a version.
                </p>
              )}
              {isActiveStatus(acquisition.status) && manageable && (
                <Button size="sm" variant="secondary" onClick={() => onCancel(acquisition.id)}>
                  Cancel
                </Button>
              )}
              {isAdmin && <AdminDetails acquisitionId={acquisition.id} />}
            </div>
          )}

          {quiet && acquisition.status === 'READY' && (
            <div className="mt-1.5">
              <ButtonLink to={`/library/${acquisition.bookId}`} variant="ghost" size="sm">
                View in library
              </ButtonLink>
            </div>
          )}
        </div>
      </div>

      {quiet && isAdmin && (
        <div className="mt-1 pl-13">
          <AdminDetails acquisitionId={acquisition.id} />
        </div>
      )}
    </article>
  )
}

function AdminDetails({ acquisitionId }: { acquisitionId: string }) {
  const [open, setOpen] = useState(false)
  const [diagnostics, setDiagnostics] = useState<AcquisitionDiagnostics | null>(null)
  const [error, setError] = useState<string | null>(null)

  function toggle() {
    const next = !open
    setOpen(next)

    if (next && diagnostics === null) {
      fetchDiagnostics(acquisitionId)
        .then(setDiagnostics)
        .catch((caught: unknown) => {
          setError(caught instanceof ApiError ? caught.message : 'Could not load diagnostics')
        })
    }
  }

  return (
    <div className="w-full">
      <button
        type="button"
        onClick={toggle}
        className="text-xs text-ink-faint underline decoration-dotted underline-offset-2 transition-colors hover:text-ink-soft"
      >
        {open ? 'Hide technical details' : 'Technical details'}
      </button>

      {open && (
        <div className="mt-2 rounded-card bg-surface-2 p-3 text-xs text-ink-soft">
          {error && <p className="text-danger">{error}</p>}
          {!error && diagnostics === null && <p className="text-ink-faint">Loading…</p>}
          {diagnostics && (
            <dl className="grid gap-1 sm:grid-cols-2">
              <Detail label="Release" value={diagnostics.acquisition.selectedReleaseName} />
              <Detail label="Indexer" value={diagnostics.acquisition.selectedReleaseIndexer} />
              <Detail
                label="Score"
                value={
                  diagnostics.acquisition.selectedReleaseScore !== null
                    ? String(diagnostics.acquisition.selectedReleaseScore)
                    : null
                }
              />
              <Detail
                label="Confidence"
                value={
                  diagnostics.acquisition.selectedReleaseConfidence !== null
                    ? diagnostics.acquisition.selectedReleaseConfidence.toFixed(2)
                    : null
                }
              />
              <Detail label="Provider" value={diagnostics.acquisition.downloadProvider} />
              <Detail label="Torrent hash" value={diagnostics.acquisition.providerDownloadId} />
              <Detail
                label="Raw state"
                value={
                  diagnostics.providerState && 'state' in diagnostics.providerState
                    ? String(diagnostics.providerState.state)
                    : null
                }
              />
              <Detail label="Events" value={String(diagnostics.events.length)} />
            </dl>
          )}
        </div>
      )}
    </div>
  )
}

function Detail({ label, value }: { label: string; value: string | null }) {
  return (
    <div className="flex gap-2">
      <dt className="shrink-0 text-ink-faint">{label}:</dt>
      <dd className="truncate text-ink-soft">{value ?? '-'}</dd>
    </div>
  )
}

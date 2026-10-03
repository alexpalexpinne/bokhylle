import { useEffect, useState } from 'react'
import { Link } from 'react-router-dom'
import { Download, Send } from 'lucide-react'
import {
  createAcquisitionForBook, fetchAcquisition, fetchBookAcquisitions,
  isActiveStatus, retryAcquisition, statusLabel,
  type Acquisition,
} from '../../api/acquisitions'
import { ApiError } from '../../api/client'
import { useAuth } from '../../auth/useAuth'
import { SendToReaderDialog } from '../../components/SendToReaderDialog'
import { Button } from '../../components/ui/Button'
import { useMutation } from '../../lib/useMutation'

function downloadPhrase(item: Acquisition) {
  switch (item.status) {
    case 'REQUESTED': return 'Download requested'
    case 'SEARCHING': case 'EVALUATING': return 'Finding a suitable file…'
    case 'NEEDS_SELECTION': return 'Waiting for a version to be selected'
    case 'QUEUED': return 'Queued for download'
    case 'DOWNLOADING': return `Downloading · ${Math.floor(Math.max(0, Math.min(100, item.progress)))}%`
    case 'DOWNLOADED': case 'INSPECTING': case 'IDENTIFIED': case 'IMPORTING': return 'Adding to the library…'
    case 'NEEDS_REVIEW': return 'The downloaded files need review'
    case 'READY': return 'Download complete'
    default: return statusLabel(item)
  }
}

export function BookAcquisitionStatus({ bookId, hasFile, onReady, onChoose }: {
  bookId: number; hasFile: boolean; onReady: () => void; onChoose: (id: string) => void
}) {
  const { user } = useAuth()
  const [items, setItems] = useState<Acquisition[] | null>(null)
  const [readError, setReadError] = useState<string | null>(null)
  const [reload, setReload] = useState(0)
  const [sendId, setSendId] = useState<string | null>(null)
  const mutation = useMutation()
  const canAcquire = user?.role === 'admin' || !!user?.canAcquire

  useEffect(() => {
    let cancelled = false
    let timer: ReturnType<typeof setTimeout>
    let lastReady = ''
    let pollDelay = 15000
    async function poll() {
      try {
        const data = await fetchBookAcquisitions(bookId)
        if (cancelled) return
        setItems(data)
        setReadError(null)
        const ready = data.filter((item) => item.status === 'READY').map((item) => `${item.id}:${item.deliveryStatus}`).join(',')
        if (ready && ready !== lastReady) onReady()
        lastReady = ready
        pollDelay = data.some((item) => isActiveStatus(item.status) || item.deliverOnReady || item.deliveryStatus === 'PENDING') ? 2000 : 15000
      } catch (caught) {
        if (!cancelled) setReadError(caught instanceof ApiError ? caught.message : 'Could not load download status')
      } finally {
        if (!cancelled) timer = setTimeout(() => void poll(), pollDelay)
      }
    }
    void poll()
    return () => { cancelled = true; clearTimeout(timer) }
  }, [bookId, onReady, reload])

  const pending = items?.filter((item) => isActiveStatus(item.status)) ?? []
  const latest = items?.[0]
  const failed = latest && ['NO_RELEASE_FOUND', 'DOWNLOAD_FAILED', 'IMPORT_FAILED'].includes(latest.status)
  const stopped = pending.length === 0 && latest && (failed || (!hasFile && latest.status === 'CANCELLED')) ? latest : null

  function update(item: Acquisition) {
    setItems((current) => [item, ...(current ?? []).filter((other) => other.id !== item.id)])
  }

  function getBook() {
    void mutation.run('get', async () => {
      const start = await createAcquisitionForBook(bookId)
      return fetchAcquisition(start.id)
    }, 'Could not start downloading this book', (item) => {
      update(item)
      onReady()
      if (item.askBeforeDownload && (user?.role === 'admin' || item.requestedByUserId === user?.id)) onChoose(item.id)
    })
  }

  if (hasFile && !pending.length && !stopped && !readError && !mutation.error) return null

  return <div className="space-y-3">
    {!hasFile && !items && !readError && <p role="status" className="text-sm text-ink-muted">Checking download status…</p>}
    {readError && <div className="flex flex-wrap items-center gap-2 text-sm text-danger">
      <p role="alert">{readError}</p><Button variant="ghost" size="sm" onClick={() => setReload((value) => value + 1)}>Refresh status</Button>
    </div>}
    {pending.map((item) => {
      const canManage = user?.role === 'admin' || item.requestedByUserId === user?.id
      return <div key={item.id} className="space-y-3 border-l-2 border-accent pl-3">
        <div role="status" className="space-y-1 text-sm">
          {hasFile && <p className="text-xs text-ink-muted">Another version is being added. Your current file is ready to read or send.</p>}
          <p className="font-medium text-ink">{downloadPhrase(item)}</p>
          {item.preferredLanguage && <p className="text-xs text-ink-muted">{item.preferredLanguage.toUpperCase()}{item.selectedReleaseFormat ? ` · ${item.selectedReleaseFormat.toUpperCase()}` : ''}</p>}
          {item.deliverOnReady && <button type="button" title="Change or cancel your scheduled send" className="min-h-11 break-words text-left text-xs text-ink-soft underline decoration-line underline-offset-4 hover:text-ink" onClick={() => setSendId(item.id)}>{item.scheduledDeliveryAddress ? `Will send to ${item.scheduledDeliveryAddress} when ready.` : 'Will send to your default reader when ready.'}</button>}
          {item.status === 'NEEDS_SELECTION' && !canManage && <p className="text-xs text-ink-muted">{item.requestedBy ?? 'The requester'} or an administrator can select the file.</p>}
        </div>
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          {item.status === 'NEEDS_SELECTION' && canManage && <Button variant="primary" onClick={() => onChoose(item.id)}>Select a version</Button>}
          <Link to={user?.role === 'admin' && !item.requestedByMe ? '/activity?scope=household' : '/activity'} className="inline-flex min-h-11 items-center text-sm text-accent hover:text-accent-strong">View in Activity</Link>
          {!hasFile && item.requestedByMe && !item.deliverOnReady && <Button variant="secondary" disabled={!!mutation.busyKey} onClick={() => setSendId(item.id)}><Send size={16} aria-hidden />Send when ready</Button>}
        </div>
      </div>
    })}
    {stopped && <div className="space-y-2 text-sm">
      <p role="status" className="font-medium text-ink">{hasFile ? `Another version: ${downloadPhrase(stopped)}` : downloadPhrase(stopped)}</p>
      {hasFile && <p className="text-xs text-ink-muted">Your current file is still ready to read or send.</p>}
      {stopped.errorMessage && <p className="break-words text-ink-muted">{stopped.errorMessage}</p>}
      {stopped.keepLooking && <p className="text-xs text-ink-muted">Bokhylle will try again automatically.</p>}
      <div className="flex flex-wrap items-center gap-4">
        {canAcquire && (user?.role === 'admin' || stopped.requestedByUserId === user?.id) && stopped.status !== 'CANCELLED' && <Button variant={hasFile ? 'secondary' : 'primary'} disabled={!!mutation.busyKey}
          onClick={() => void mutation.run('retry', () => retryAcquisition(stopped.id), 'Could not try again', update)}>Try again</Button>}
        {canAcquire && stopped.status === 'CANCELLED' && <Button variant="primary" disabled={!!mutation.busyKey} onClick={getBook}>Get for my shelf</Button>}
        <Link to={user?.role === 'admin' && !stopped.requestedByMe ? '/activity?scope=household' : '/activity'} className="inline-flex min-h-11 items-center text-accent hover:text-accent-strong">View in Activity</Link>
      </div>
    </div>}
    {!hasFile && items && !readError && pending.length === 0 && !stopped && <div className="space-y-3">
      <p role="status" className="text-sm text-ink-muted">No downloaded file is available yet.</p>
      {canAcquire && <Button variant="primary" size="lg" disabled={!!mutation.busyKey} onClick={getBook}><Download size={16} aria-hidden />{mutation.busyKey === 'get' ? 'Getting…' : 'Get for my shelf'}</Button>}
    </div>}
    {mutation.error && <p role="alert" className="text-sm text-danger">{mutation.error}</p>}
    {sendId && <SendToReaderDialog bookId={bookId} acquisitionId={sendId} scheduled={items?.find((item) => item.id === sendId)?.deliverOnReady} initialAddress={items?.find((item) => item.id === sendId)?.scheduledDeliveryAddress} onClose={() => setSendId(null)} onSent={() => setReload((value) => value + 1)} />}
  </div>
}

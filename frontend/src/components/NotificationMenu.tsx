import { useEffect, useRef, useState } from 'react'
import { Link } from 'react-router-dom'
import { Bell, Check } from 'lucide-react'
import { type Notification } from '../api/notifications'
import { coverUrl } from '../api/library'
import {
  type BookRequest,
  approveBookRequest,
  declineBookRequest,
} from '../api/requests'
import { useMutation } from '../lib/useMutation'
import { decideDemoRequest } from '../api/demo'
import { useAuth } from '../auth/useAuth'
import { Button } from './ui/Button'

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

export function NotificationMenu({
  items,
  unread,
  pendingRequestItems,
  canDecide,
  markRead,
  onDecided,
}: {
  items: Notification[]
  unread: number
  pendingRequestItems: BookRequest[]
  /// Administrators decide requests; other readers track their own.
  canDecide: boolean
  markRead: () => Promise<void>
  onDecided: (id: number) => void
}) {
  const [open, setOpen] = useState(false)
  const { demo } = useAuth()
  const containerRef = useRef<HTMLDivElement>(null)
  const decision = useMutation()

  function decide(request: BookRequest, approve: boolean) {
    void decision.run(
      `request-${request.id}`,
      () =>
        demo
          ? decideDemoRequest(request.id, approve ? 'approve' : 'decline')
          : approve ? approveBookRequest(request.id) : declineBookRequest(request.id),
      approve ? 'Could not approve that request' : 'Could not decline that request',
      () => onDecided(request.id),
    )
  }

  useEffect(() => {
    if (!open) {
      return
    }

    function onPointerDown(event: PointerEvent) {
      if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
        setOpen(false)
      }
    }
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        setOpen(false)
      }
    }

    document.addEventListener('pointerdown', onPointerDown)
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('pointerdown', onPointerDown)
      document.removeEventListener('keydown', onKeyDown)
    }
  }, [open])

  return (
    <div ref={containerRef} className="relative">
      <button
        type="button"
        onClick={() => setOpen((current) => !current)}
        aria-expanded={open}
        aria-label={unread > 0 ? `Notifications, ${unread} unread` : 'Notifications'}
        className="relative rounded-full p-2 text-ink-soft transition-colors hover:bg-surface-2 hover:text-ink"
      >
        <Bell size={18} aria-hidden />
        {unread > 0 && (
          <span
            key={unread}
            className="animate-pop absolute right-1 top-1 flex h-4 min-w-4 items-center justify-center rounded-full bg-accent px-1 text-[10px] font-semibold text-accent-ink"
          >
            {unread > 9 ? '9+' : unread}
          </span>
        )}
      </button>

      {open && (
        <div
          className="fixed inset-x-4 top-[4.5rem] z-50 max-h-[calc(100dvh-6rem)] overflow-y-auto rounded-panel bg-surface shadow-modal sm:absolute sm:inset-x-auto sm:right-0 sm:top-full sm:mt-2 sm:w-80"
        >
          <div className="flex items-center justify-between border-b border-line px-4 py-3">
            <p className="text-sm font-medium text-ink">Notifications</p>
            {unread > 0 && (
              <button
                type="button"
                onClick={() => void markRead()}
                className="inline-flex items-center gap-1 text-xs text-ink-muted transition-colors hover:text-ink"
              >
                <Check size={13} aria-hidden />
                Mark all read
              </button>
            )}
          </div>

          <div className="max-h-96 overflow-y-auto">
            {canDecide && pendingRequestItems.length > 0 && (
              <div className="border-b border-line bg-surface-2/40 px-4 py-3">
                <p className="text-xs font-semibold uppercase tracking-[0.14em] text-ink-muted">
                  {pendingRequestItems.length === 1
                    ? '1 request awaiting you'
                    : `${pendingRequestItems.length} requests awaiting you`}
                </p>
                <ul className="mt-2 space-y-2">
                  {pendingRequestItems.map((request) => (
                    <li key={request.id} className="flex items-center gap-3">
                      <img
                        src={coverUrl(request.bookId)}
                        alt=""
                        loading="lazy"
                        className="h-12 w-8 shrink-0 rounded-[2px] object-cover"
                      />
                      <div className="min-w-0 flex-1">
                        <p className="truncate text-sm font-medium text-ink">{request.title}</p>
                        <p className="truncate text-xs text-ink-muted">
                          {request.authors[0] ?? 'Unknown author'} · asked by{' '}
                          {request.requester}
                        </p>
                      </div>
                      <div className="flex shrink-0 gap-1">
                        <Button
                          variant="primary"
                          size="sm"
                          disabled={decision.busyKey === `request-${request.id}`}
                          onClick={() => decide(request, true)}
                        >
                          Approve
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          disabled={decision.busyKey === `request-${request.id}`}
                          onClick={() => decide(request, false)}
                        >
                          Decline
                        </Button>
                      </div>
                    </li>
                  ))}
                </ul>
                {decision.error && (
                  <p className="mt-2 text-xs text-danger" role="alert">
                    {decision.error}
                  </p>
                )}
              </div>
            )}
            {items.length === 0 ? (
              <p className="px-4 py-8 text-center text-sm text-ink-muted">
                Nothing new. Books you request will report back here.
              </p>
            ) : (
              items.map((item) => (
                <div
                  key={item.id}
                  className={`border-b border-line/60 px-4 py-3 last:border-0 ${
                    item.read ? '' : 'bg-surface-2/40'
                  }`}
                >
                  <div className="flex items-start justify-between gap-3">
                    <p className="text-sm font-medium text-ink">{item.title}</p>
                    <span className="shrink-0 text-xs text-ink-faint">
                      {formatWhen(item.createdAt)}
                    </span>
                  </div>
                  {item.body && (
                    <p className="mt-0.5 line-clamp-2 text-xs text-ink-muted">{item.body}</p>
                  )}
                </div>
              ))
            )}
          </div>

          <div className="flex items-center justify-between gap-2 border-t border-line px-4 py-2.5">
            {canDecide && !demo ? (
              <Link
                to="/requests"
                onClick={() => setOpen(false)}
                className="text-xs text-ink-muted transition-colors hover:text-ink"
              >
                View all requests
              </Link>
            ) : (
              <span />
            )}
            <Button variant="ghost" size="sm" onClick={() => setOpen(false)}>
              Close
            </Button>
          </div>
        </div>
      )}
    </div>
  )
}

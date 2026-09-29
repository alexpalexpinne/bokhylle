import { useEffect, useState } from 'react'
import { RotateCw } from 'lucide-react'
import { type Delivery, fetchDeliveries, retryDelivery } from '../../api/delivery'
import { Button } from '../../components/ui/Button'
import { SectionMark } from '../../components/ui/SectionMark'
import { useMutation } from '../../lib/useMutation'

export function DeliveryHistory({ bookId, refreshToken }: { bookId: number; refreshToken: number }) {
  const [deliveries, setDeliveries] = useState<Delivery[]>([])
  const [retryToken, setRetryToken] = useState(0)
  const mutation = useMutation()

  useEffect(() => {
    let cancelled = false
    fetchDeliveries(bookId)
      .then((items) => {
        if (!cancelled) setDeliveries(items)
      })
      .catch(() => {
        if (!cancelled) setDeliveries([])
      })
    return () => {
      cancelled = true
    }
  }, [bookId, refreshToken, retryToken])

  if (deliveries.length === 0) return null

  return (
    <section>
      <SectionMark title="Sent to your reader" />
      <ul className="mt-2 divide-y divide-line">
        {deliveries.map((delivery) => (
          <li key={delivery.id} className="flex flex-wrap items-center justify-between gap-3 py-3.5 text-sm">
            <span className="text-ink-soft">
              {delivery.address}
              <span className="ml-3 font-sans text-[10px] uppercase tracking-[0.14em] text-ink-faint">
                {new Date(delivery.createdAt * 1000).toLocaleString(undefined, {
                  day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit',
                })}
              </span>
            </span>
            <span className={delivery.status === 'SENT' ? 'text-success' : delivery.status === 'FAILED' ? 'text-danger' : 'text-ink-muted'}>
              {delivery.status === 'SENT' ? 'Delivered' : delivery.status === 'FAILED' ? 'Failed' : 'Sending…'}
            </span>
            {delivery.errorMessage && <span className="w-full text-xs text-ink-faint">{delivery.errorMessage}</span>}
            {delivery.status === 'FAILED' && (
              <Button
                variant="ghost"
                size="sm"
                disabled={mutation.busyKey === `delivery-${delivery.id}`}
                onClick={() => void mutation.run(
                  `delivery-${delivery.id}`,
                  () => retryDelivery(delivery.id),
                  'Could not retry that delivery',
                  () => setRetryToken((token) => token + 1),
                )}
              >
                <RotateCw size={13} />
                Retry
              </Button>
            )}
          </li>
        ))}
      </ul>
      {mutation.error && <p role="alert" className="mt-3 border-l-2 border-danger pl-3 text-sm text-danger">{mutation.error}</p>}
    </section>
  )
}

import { useEffect, useState } from 'react'
import { ApiError } from '../api/client'
import { Input } from '../components/ui/Field'
import {
  type DeliveryTarget,
  createTarget,
  deliverBook,
  fetchDefaultReader,
  fetchTargets,
} from '../api/delivery'
import { Button } from './ui/Button'
import { Modal } from './ui/Modal'

type Destination =
  | { kind: 'target'; id: number }
  | { kind: 'household' }
  | { kind: 'new' }

type SendToReaderDialogProps = {
  bookId: number
  fileId: number
  format: string
  onClose: () => void
  onSent: () => void
}

export function SendToReaderDialog({
  bookId,
  fileId,
  format,
  onClose,
  onSent,
}: SendToReaderDialogProps) {
  const [targets, setTargets] = useState<DeliveryTarget[]>([])
  // One unambiguous destination: the radio shown as selected is always the
  // one that is sent to. Typing an address makes that address the destination;
  // choosing a target or the household reader clears the typed address.
  const [destination, setDestination] = useState<Destination | null>(null)
  const [newAddress, setNewAddress] = useState('')
  const [newType, setNewType] = useState<'kindle' | 'pocketbook' | 'other'>('kindle')
  const [loading, setLoading] = useState(true)
  const [sending, setSending] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [warning, setWarning] = useState<string | null>(null)
  const [result, setResult] = useState<'sent' | 'failed' | null>(null)
  const [householdReader, setHouseholdReader] = useState<string | null>(null)
  const [senderAddress, setSenderAddress] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false

    fetchTargets()
      .then((items) => {
        if (!cancelled) {
          setTargets(items)
          const preferred =
            items.find((target) => target.enabled && target.isDefault) ??
            items.find((target) => target.enabled)
          if (preferred) {
            setDestination({ kind: 'target', id: preferred.id })
          }
        }
      })
      .catch((caught: unknown) => {
        if (!cancelled) {
          setError(caught instanceof ApiError ? caught.message : 'Could not load your readers')
        }
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false)
        }
      })

    fetchDefaultReader()
      .then((reader) => {
        if (!cancelled) {
          if (reader.source === 'household') {
            setHouseholdReader(reader.address)
          }
          setSenderAddress(reader.senderAddress)
        }
      })
      .catch((caught: unknown) => {
        if (!cancelled) {
          console.warn('send_to_reader.default_reader.load_failed', caught)
          setWarning(
            'Could not load your household reader. You can still send to a listed address.',
          )
        }
      })

    return () => {
      cancelled = true
    }
  }, [])

  const enabledTargets = targets.filter((target) => target.enabled)

  useEffect(() => {
    if (loading) {
      return
    }
    if (enabledTargets.length === 0 && householdReader) {
      setDestination({ kind: 'household' })
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loading, targets, householdReader])

  function chooseTarget(id: number) {
    setDestination({ kind: 'target', id })
    setNewAddress('')
  }

  function chooseHousehold() {
    setDestination({ kind: 'household' })
    setNewAddress('')
  }

  function changeNewAddress(value: string) {
    setNewAddress(value)
    setDestination(value.trim() ? { kind: 'new' } : null)
  }

  async function send() {
    setSending(true)
    setError(null)

    try {
      let targetId: number | undefined
      if (destination?.kind === 'target') {
        targetId = destination.id
      } else if (destination?.kind === 'new') {
        const address = newAddress.trim()
        if (!address) {
          setError('Enter a reader address')
          return
        }
        const target = await createTarget(address, newType)
        targetId = target.id
        // Reuse the created reader on a retry instead of creating another.
        setTargets((current) => [...current, target])
        setDestination({ kind: 'target', id: target.id })
      } else if (destination?.kind !== 'household') {
        setError('Choose where to send this book')
        return
      }

      const delivery = await deliverBook(bookId, fileId, targetId)
      if (delivery.status === 'SENT') {
        setResult('sent')
        onSent()
      } else {
        setResult('failed')
        setError(delivery.errorMessage ?? 'The delivery failed')
      }
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not send the book')
    } finally {
      setSending(false)
    }
  }

  const selectedTarget =
    destination?.kind === 'target'
      ? targets.find((target) => target.id === destination.id)
      : undefined
  // The household fallback is the configured Kindle address.
  const effectiveType =
    destination?.kind === 'household'
      ? 'kindle'
      : destination?.kind === 'new'
        ? newType
        : selectedTarget?.type
  const deviceLabel =
    effectiveType === 'pocketbook' ? 'PocketBook' : effectiveType === 'kindle' ? 'Kindle' : 'reader'
  const deviceNoun = deviceLabel === 'reader' ? 'reader' : `${deviceLabel} address`

  const canSend =
    !sending &&
    result !== 'sent' &&
    (destination?.kind === 'target' ||
      destination?.kind === 'household' ||
      (destination?.kind === 'new' && newAddress.trim() !== ''))

  return (
    <Modal
      title="Send to your reader"
      description={`The ${format.toUpperCase()} file is emailed to your ${deviceNoun}.`}
      onClose={onClose}
      footer={
        result === 'sent' ? (
          <Button variant="primary" onClick={onClose}>
            Done
          </Button>
        ) : (
          <Button variant="primary" onClick={() => void send()} disabled={!canSend}>
            {sending ? 'Sending…' : 'Send'}
          </Button>
        )
      }
    >
      {error && (
        <p className="mb-4 rounded-card bg-surface-2 px-3.5 py-2.5 text-sm text-danger">{error}</p>
      )}

      {warning && (
        <p role="status" className="mb-4 border-l-2 border-warning pl-3 text-sm text-ink-soft">
          {warning}
        </p>
      )}

      {result === 'sent' && (
        <p className="mb-4 rounded-card bg-surface-2 px-3.5 py-2.5 text-sm text-success">
          Sent. It should arrive on your {deviceLabel} shortly.
        </p>
      )}

      {loading ? (
        <p className="text-sm text-ink-muted">Loading your readers…</p>
      ) : (
        <div className="space-y-2">
          {targets.map((target) => (
            <label
              key={target.id}
              className={`flex items-center gap-3 rounded-card px-4 py-3 transition-colors ${
                target.enabled
                  ? `cursor-pointer ${
                      destination?.kind === 'target' && destination.id === target.id
                        ? 'bg-surface-2'
                        : 'hover:bg-surface-2/60'
                    }`
                  : 'cursor-not-allowed opacity-50'
              }`}
            >
              <input
                type="radio"
                name="target"
                checked={destination?.kind === 'target' && destination.id === target.id}
                disabled={!target.enabled}
                onChange={() => chooseTarget(target.id)}
                className="h-4 w-4 accent-[var(--color-accent)]"
              />
              <span className="min-w-0">
                <span className="block truncate text-sm text-ink">{target.address}</span>
                <span className="block text-xs text-ink-faint">
                  {target.name}
                  {target.isDefault ? ' · default' : ''}
                  {target.enabled ? '' : ' · disabled'}
                </span>
              </span>
            </label>
          ))}

          {enabledTargets.length === 0 && householdReader && (
            <label
              className={`flex cursor-pointer items-center gap-3 rounded-card px-4 py-3 transition-colors ${
                destination?.kind === 'household' ? 'bg-surface-2' : 'hover:bg-surface-2/60'
              }`}
            >
              <input
                type="radio"
                name="target"
                checked={destination?.kind === 'household'}
                onChange={chooseHousehold}
                className="h-4 w-4 accent-[var(--color-accent)]"
              />
              <span className="min-w-0">
                <span className="block truncate text-sm text-ink">{householdReader}</span>
                <span className="block text-xs text-ink-faint">Household reader</span>
              </span>
            </label>
          )}

          <label className="block pt-2">
            <span className="mb-1.5 block text-xs text-ink-muted">Or add another reader address</span>
            <div className="mb-2 flex flex-wrap gap-1.5">
              {(
                [
                  ['kindle', 'Kindle'],
                  ['pocketbook', 'PocketBook'],
                  ['other', 'Other'],
                ] as const
              ).map(([value, label]) => (
                <button
                  key={value}
                  type="button"
                  aria-pressed={destination?.kind === 'new' && newType === value}
                  onClick={() => {
                    setNewType(value)
                    if (newAddress.trim()) {
                      setDestination({ kind: 'new' })
                    }
                  }}
                  className={`rounded-[3px] px-3 py-1.5 text-xs transition-colors ${
                    newType === value
                      ? 'bg-accent text-accent-ink'
                      : 'bg-surface-2 text-ink-soft hover:bg-surface-3'
                  }`}
                >
                  {label}
                </button>
              ))}
            </div>
            <Input
              value={newAddress}
              onChange={(event) => changeNewAddress(event.target.value)}
              placeholder={
                newType === 'kindle'
                  ? 'name@kindle.com'
                  : newType === 'pocketbook'
                    ? 'name@pbsync.com'
                    : 'reader@example.com'
              }
            />
          </label>

          {deviceLabel === 'Kindle' && senderAddress && (
            <p className="pt-2 text-xs text-ink-faint">
              Emails are sent from {senderAddress}; it must be on Amazon&apos;s approved personal
              document list.
            </p>
          )}

          {deviceLabel === 'PocketBook' && (
            <p className="pt-2 text-xs text-ink-faint">
              Find your Send-to-PocketBook address in the PocketBook app; no sender approval is
              needed.
            </p>
          )}
        </div>
      )}
    </Modal>
  )
}

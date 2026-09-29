import { Mail, Pencil, Plus, Star, Trash2 } from 'lucide-react'
import type { DeliveryTarget } from '../../api/delivery'
import { Button } from '../../components/ui/Button'

type ReaderEditor = {
  mode: 'create' | 'edit'
  target?: DeliveryTarget
  name: string
  address: string
  deviceType: string
}

type ReadersSectionProps = {
  targets: DeliveryTarget[]
  enabledTargets: DeliveryTarget[]
  loading: boolean
  error: string | null
  notice: string | null
  busy: number | null
  openEditor: (editor: ReaderEditor) => void
  openRemove: (target: DeliveryTarget) => void
  makeDefault: (target: DeliveryTarget) => void
  toggleEnabled: (target: DeliveryTarget) => void
}

export function ReadersSection({
  targets,
  enabledTargets,
  loading,
  error,
  notice,
  busy,
  openEditor,
  openRemove,
  makeDefault,
  toggleEnabled,
}: ReadersSectionProps) {
  const addReader = () => openEditor({ mode: 'create', name: 'Kindle', address: '', deviceType: 'kindle' })

  return (
    <section className="mt-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 className="text-base font-semibold text-ink">My readers</h2>
        {!loading && targets.length > 0 && (
          <Button variant="primary" size="sm" onClick={addReader}>
            <Plus size={15} aria-hidden />
            Add reader
          </Button>
        )}
      </div>
      <p className="mt-0.5 text-xs text-ink-faint">Where Bokhylle sends books. Emailed delivery uses the address your reader app gives you.</p>
      {notice && <p role="status" className="mt-4 border-l-2 border-success pl-4 text-sm text-ink-soft">{notice}</p>}
      {error && <p role="alert" className="mt-4 border-l-2 border-danger pl-4 text-sm text-danger">{error}</p>}

      {loading ? (
        <div className="mt-4 space-y-2">
          {Array.from({ length: 2 }).map((_, index) => <div key={index} className="h-20 animate-pulse rounded-panel bg-surface" />)}
        </div>
      ) : targets.length === 0 ? (
        <div className="mt-4 flex flex-col items-center rounded-panel bg-surface px-6 py-12 text-center">
          <span className="flex h-12 w-12 items-center justify-center rounded-full bg-surface-2 text-ink-faint"><Mail size={22} aria-hidden /></span>
          <p className="mt-4 font-display text-title text-ink">No readers yet</p>
          <p className="mt-2 max-w-sm text-sm text-ink-muted">Add your reader&apos;s email address and Bokhylle can send books straight to your device.</p>
          <Button variant="primary" className="mt-6" onClick={addReader}><Plus size={15} aria-hidden />Add reader</Button>
        </div>
      ) : (
        <div className="mt-4 space-y-2">
          {targets.map((target) => (
            <article key={target.id} className="flex flex-wrap items-center gap-4 rounded-panel bg-surface px-4 py-4">
              <span className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-surface-2 ${target.enabled ? 'text-accent' : 'text-ink-faint'}`}><Mail size={18} aria-hidden /></span>
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <p className="text-sm font-medium text-ink">{target.name}</p>
                  {target.isDefault && <span className="rounded-[3px] bg-accent/15 px-2 py-0.5 font-sans text-[10px] font-medium uppercase tracking-[0.14em] text-ink">Default</span>}
                  {!target.enabled && <span className="rounded-[3px] bg-surface-3 px-2 py-0.5 font-sans text-[10px] font-medium uppercase tracking-[0.14em] text-ink-faint">Disabled</span>}
                </div>
                <p className="mt-0.5 truncate text-xs text-ink-muted">{target.address}</p>
              </div>
              <div className="flex flex-wrap items-center gap-1.5">
                {target.enabled && !target.isDefault && <Button variant="secondary" size="sm" disabled={busy === target.id} onClick={() => makeDefault(target)}><Star size={13} aria-hidden />Set default</Button>}
                <Button variant="ghost" size="sm" onClick={() => openEditor({ mode: 'edit', target, name: target.name, address: target.address, deviceType: target.type })}><Pencil size={13} aria-hidden />Edit</Button>
                <Button variant="ghost" size="sm" disabled={busy === target.id} onClick={() => toggleEnabled(target)}>{target.enabled ? 'Disable' : 'Enable'}</Button>
                <Button variant="danger" size="sm" disabled={busy === target.id} onClick={() => openRemove(target)}><Trash2 size={13} aria-hidden />Remove</Button>
              </div>
            </article>
          ))}
        </div>
      )}

      {enabledTargets.length === 0 && targets.length > 0 && <p className="mt-4 text-xs text-ink-faint">All readers are disabled — sending a book will fall back to the address configured in Administration.</p>}
    </section>
  )
}

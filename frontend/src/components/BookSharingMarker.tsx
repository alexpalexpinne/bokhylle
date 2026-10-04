import { useId, useState } from 'react'
import { LockKeyhole, UsersRound } from 'lucide-react'
import type { BookSharing } from './BookSharingChoice'

/** Actual book visibility. Read-only markers explain their state on touch. */
export function BookSharingIcon({ value, label, onClick }: {
  value: BookSharing
  label: string
  onClick?: () => void
}) {
  const Icon = value === 'private' ? LockKeyhole : UsersRound
  const hintId = useId()
  const [hovered, setHovered] = useState(false)
  const [focused, setFocused] = useState(false)
  const [expanded, setExpanded] = useState(false)
  const [dismissed, setDismissed] = useState(false)
  const visible = !dismissed && (hovered || focused || expanded)
  return (
    <span className="relative inline-flex shrink-0"
      onPointerEnter={(event) => { if (event.pointerType !== 'touch') { setDismissed(false); setHovered(true) } }}
      onPointerLeave={() => setHovered(false)}>
      <button type="button" aria-label={label} aria-describedby={visible ? hintId : undefined}
        aria-haspopup={onClick ? 'dialog' : undefined}
        className={`inline-flex h-12 w-12 shrink-0 items-center justify-center rounded-[3px] transition-colors focus-visible:outline-2 focus-visible:outline-focus ${onClick
          ? 'cursor-pointer text-ink-soft hover:bg-surface-3 hover:text-ink focus-visible:bg-surface-3'
          : 'cursor-help text-ink-muted hover:text-ink-soft'}`}
        onFocus={(event) => { setDismissed(false); setFocused(event.currentTarget.matches(':focus-visible')) }}
        onBlur={() => { setFocused(false); setExpanded(false) }}
        onKeyDown={(event) => { if (event.key === 'Escape' && visible) { event.stopPropagation(); setDismissed(true); setExpanded(false) } }}
        onClick={() => {
          if (onClick) { setHovered(false); setFocused(false); setExpanded(false); onClick() }
          else { setDismissed(false); setExpanded(!expanded) }
        }}>
        <Icon size={18} aria-hidden />
      </button>
      {visible && <span id={hintId} role="tooltip"
        className="absolute right-0 top-full z-30 mt-1 w-64 max-w-[calc(100vw-2.5rem)] rounded-[3px] border border-line bg-surface px-3 py-2 text-left text-xs leading-relaxed text-ink shadow-card">
        {label}
      </span>}
    </span>
  )
}

export function BookSharingMarker({ value, label }: { value: BookSharing; label: string }) {
  return <BookSharingIcon value={value} label={label} />
}

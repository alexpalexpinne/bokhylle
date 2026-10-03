import { useEffect, useId, useRef, useState, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { MoreHorizontal, X } from 'lucide-react'
import { Button } from '../../components/ui/Button'

const FOCUSABLE = 'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), summary, [tabindex]:not([tabindex="-1"])'

function Actions({ children, onAction }: { children: (close: () => void) => ReactNode; onAction: () => void }) {
  return children(onAction)
}

/** A compact desktop popover and a mobile sheet for occasional book actions. */
export function BookActionsMenu({ children }: { children: (close: () => void) => ReactNode }) {
  const [open, setOpen] = useState(false)
  const triggerRef = useRef<HTMLButtonElement>(null)
  const panelRef = useRef<HTMLDivElement>(null)
  const restoreFocusRef = useRef(true)
  const previousOverflowRef = useRef<string | null>(null)
  const panelId = useId()
  const titleId = useId()

  // Keep the action components mounted when this panel closes: their dialogs
  // are portalled separately and must survive the transition out of More.
  function closeForAction() {
    restoreFocusRef.current = false
    // Hand scroll and focus back before the next dialog captures them.
    if (previousOverflowRef.current !== null) {
      document.body.style.overflow = previousOverflowRef.current
      previousOverflowRef.current = null
    }
    triggerRef.current?.focus()
    setOpen(false)
  }

  useEffect(() => {
    if (!open) return
    restoreFocusRef.current = true
    const trigger = triggerRef.current
    const previousOverflow = document.body.style.overflow
    previousOverflowRef.current = previousOverflow
    document.body.style.overflow = 'hidden'

    function position() {
      const panel = panelRef.current
      const trigger = triggerRef.current
      if (!panel || !trigger) return
      const bounds = trigger.getBoundingClientRect()
      const below = Math.max(0, window.innerHeight - bounds.bottom - 24)
      const above = Math.max(0, bounds.top - 24)
      const openBelow = below >= Math.min(panel.scrollHeight, 240) || below >= above
      const availableHeight = Math.max(44, openBelow ? below : above)
      const height = Math.min(panel.scrollHeight, availableHeight)
      const left = Math.max(16, Math.min(bounds.right - panel.offsetWidth, window.innerWidth - panel.offsetWidth - 16))
      const top = openBelow
        ? bounds.bottom + 8
        : Math.max(16, bounds.top - height - 8)
      panel.style.setProperty('--book-menu-left', `${left}px`)
      panel.style.setProperty('--book-menu-top', `${top}px`)
      panel.style.setProperty('--book-menu-max-height', `${availableHeight}px`)
    }

    position()
    panelRef.current?.focus()
    const observer = new ResizeObserver(position)
    if (panelRef.current) observer.observe(panelRef.current)
    window.addEventListener('resize', position)

    function onKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        event.preventDefault()
        setOpen(false)
        return
      }
      if (event.key !== 'Tab') return
      const panel = panelRef.current
      if (!panel) return
      const controls = Array.from(panel.querySelectorAll<HTMLElement>(FOCUSABLE))
        .filter((control) => control.getClientRects().length > 0)
      const first = controls[0]
      const last = controls.at(-1)
      if (!first || !last) {
        event.preventDefault()
      } else if (event.shiftKey && (document.activeElement === first || document.activeElement === panel)) {
        event.preventDefault()
        last.focus()
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault()
        first.focus()
      }
    }

    window.addEventListener('keydown', onKeyDown)
    return () => {
      observer.disconnect()
      window.removeEventListener('resize', position)
      window.removeEventListener('keydown', onKeyDown)
      if (previousOverflowRef.current !== null) {
        document.body.style.overflow = previousOverflowRef.current
        previousOverflowRef.current = null
      }
      if (restoreFocusRef.current) trigger?.focus()
    }
  }, [open])

  return <>
    <button
      ref={triggerRef}
      type="button"
      aria-label="More"
      title="More book actions"
      aria-haspopup="dialog"
      aria-expanded={open}
      aria-controls={panelId}
      onClick={() => setOpen((current) => !current)}
      className="inline-flex min-h-12 min-w-0 flex-1 items-center justify-center gap-2 rounded-[3px] px-3 text-sm font-medium text-ink-soft transition-colors hover:bg-surface-2 hover:text-ink focus-visible:outline-2 focus-visible:outline-focus sm:min-h-11 sm:flex-none"
    >
      <MoreHorizontal size={20} aria-hidden />
      <span className="hidden sm:inline">More</span>
    </button>
    {createPortal(<div className={open ? 'fixed inset-0 z-50' : 'hidden'}>
      <div className="absolute inset-0 bg-overlay backdrop-blur-sm sm:bg-transparent sm:backdrop-blur-none" onClick={() => setOpen(false)} aria-hidden />
      <div
        ref={panelRef}
        id={panelId}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        className="fixed inset-x-0 bottom-0 max-h-[85dvh] overflow-y-auto overscroll-contain rounded-t-panel bg-surface p-4 pb-[max(1rem,env(safe-area-inset-bottom))] shadow-modal outline-none sm:inset-x-auto sm:bottom-auto sm:left-[var(--book-menu-left)] sm:top-[var(--book-menu-top)] sm:max-h-[var(--book-menu-max-height)] sm:w-80 sm:rounded-panel sm:p-2"
      >
        <div className="mb-2 flex items-center justify-between sm:mb-0">
          <h2 id={titleId} className="font-display text-xl text-ink sm:sr-only">Book options</h2>
          <Button variant="ghost" size="sm" className="min-h-11 min-w-11 sm:hidden" aria-label="Close book options" onClick={() => setOpen(false)}><X size={18} aria-hidden /></Button>
        </div>
        <div className="space-y-1 [&_a]:min-h-11 [&_button]:min-h-11">
          <Actions onAction={closeForAction}>{children}</Actions>
        </div>
      </div>
    </div>, document.body)}
  </>
}

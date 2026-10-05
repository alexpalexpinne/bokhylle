import { useEffect, useRef, type ReactNode } from 'react'
import { X } from 'lucide-react'
import { IconButton } from './IconButton'

type ModalProps = {
  title: string
  description?: string
  onClose: () => void
  children: ReactNode
  headerAside?: ReactNode
  footer?: ReactNode
  wide?: boolean
}

const FOCUSABLE =
  'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])'

const modalStack: HTMLElement[] = []
let originalOverflow = ''

export function Modal({ title, description, onClose, children, headerAside, footer, wide }: ModalProps) {
  const panelRef = useRef<HTMLDivElement>(null)
  const restoreRef = useRef<HTMLElement | null>(null)
  const closeRef = useRef(onClose)

  useEffect(() => {
    closeRef.current = onClose
  }, [onClose])

  useEffect(() => {
    restoreRef.current = document.activeElement as HTMLElement | null
    const frame = panelRef.current?.parentElement
    if (!frame) return
    const previous = modalStack.at(-1)
    if (previous) {
      previous.inert = true
      previous.setAttribute('aria-hidden', 'true')
    } else {
      originalOverflow = document.body.style.overflow
    }
    modalStack.push(frame)
    panelRef.current?.focus()

    document.body.style.overflow = 'hidden'

    function onKeyDown(event: KeyboardEvent) {
      if (modalStack.at(-1) !== frame) return
      if (event.key === 'Escape') {
        closeRef.current()
        return
      }
      if (event.key !== 'Tab') {
        return
      }

      const panel = panelRef.current
      if (!panel) {
        return
      }
      const focusable = Array.from(panel.querySelectorAll<HTMLElement>(FOCUSABLE))
      if (focusable.length === 0) {
        event.preventDefault()
        return
      }

      const first = focusable[0]
      const last = focusable[focusable.length - 1]
      const active = document.activeElement
      if (event.shiftKey && (active === first || active === panel)) {
        event.preventDefault()
        last.focus()
      } else if (!event.shiftKey && active === last) {
        event.preventDefault()
        first.focus()
      }
    }

    window.addEventListener('keydown', onKeyDown)
    return () => {
      window.removeEventListener('keydown', onKeyDown)
      const index = modalStack.indexOf(frame)
      if (index !== -1) modalStack.splice(index, 1)
      const active = modalStack.at(-1)
      if (active) {
        active.inert = false
        active.removeAttribute('aria-hidden')
      } else {
        document.body.style.overflow = originalOverflow
      }
      if (restoreRef.current?.isConnected) restoreRef.current.focus()
    }
    // Deliberately mount-only: depending on `onClose` (usually an inline
    // arrow) would re-run the cleanup on every parent render and steal
    // focus from inputs on each keystroke.
  }, [])

  return (
    <div
      className="fixed inset-0 z-50 flex items-end justify-center bg-overlay pl-[env(safe-area-inset-left)] pr-[env(safe-area-inset-right)] pt-[env(safe-area-inset-top)] backdrop-blur-sm sm:items-center sm:px-4 sm:py-8"
      role="dialog"
      aria-modal="true"
      aria-label={title}
      onClick={onClose}
    >
      <div
        ref={panelRef}
        tabIndex={-1}
        className={`flex max-h-[calc(100dvh-env(safe-area-inset-top))] w-full flex-col ${wide ? 'max-w-2xl' : 'max-w-lg'} rounded-t-panel bg-surface shadow-modal outline-none sm:max-h-[calc(100dvh-4rem)] sm:rounded-panel`}
        onClick={(event) => event.stopPropagation()}
      >
        <div className="flex shrink-0 items-start justify-between gap-4 px-5 pt-5 sm:px-6 sm:pt-6">
          <div className="min-w-0 flex-1">
            <h2 className="font-display text-xl text-ink">{title}</h2>
            {description && <p className="mt-1 text-sm text-ink-muted">{description}</p>}
          </div>
          <div className="flex shrink-0 items-center">
            {headerAside}
            <IconButton label="Close" onClick={onClose}>
              <X size={18} aria-hidden />
            </IconButton>
          </div>
        </div>

        <div className={`min-h-0 flex-1 overflow-y-auto overscroll-contain px-5 py-5 sm:px-6 ${footer ? '' : 'pb-[calc(1.25rem+env(safe-area-inset-bottom))]'}`}>
          {children}
        </div>

        {footer && (
          <div className="flex shrink-0 flex-wrap justify-end gap-2 border-t border-line px-5 py-4 pb-[max(1rem,env(safe-area-inset-bottom))] sm:px-6">
            {footer}
          </div>
        )}
      </div>
    </div>
  )
}

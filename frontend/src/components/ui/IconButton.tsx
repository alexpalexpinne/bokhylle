import type { ReactNode } from 'react'

/// Icon-only control with a required accessible label.
export function IconButton({
  label,
  onClick,
  children,
  tone = 'neutral',
  size = 'md',
}: {
  label: string
  onClick: () => void
  children: ReactNode
  tone?: 'neutral' | 'danger'
  size?: 'sm' | 'md'
}) {
  return (
    <button
      type="button"
      aria-label={label}
      onClick={onClick}
      className={`inline-flex items-center justify-center rounded-[3px] transition-colors ${
        size === 'sm' ? 'h-8 w-8' : 'h-10 w-10'
      } ${
        tone === 'danger'
          ? 'text-ink-faint hover:bg-surface-3 hover:text-danger'
          : 'text-ink-faint hover:bg-surface-3 hover:text-ink'
      }`}
    >
      {children}
    </button>
  )
}

import type { ReactNode } from 'react'

/// The single empty-state presentation: a quiet panel, a display title, one
/// explanatory line and an optional action. Pages pass spacing via
/// `className` instead of re-styling the internals.
export function EmptyState({
  title,
  message,
  action,
  className = '',
}: {
  title: string
  message: string
  action?: ReactNode
  className?: string
}) {
  return (
    <div
      className={`rounded-panel bg-surface px-6 py-16 text-center ${className}`.trim()}
    >
      <p className="font-display text-title text-ink">{title}</p>
      <p className="mx-auto mt-2 max-w-sm text-sm text-ink-muted">{message}</p>
      {action && <div className="mt-6 flex justify-center">{action}</div>}
    </div>
  )
}

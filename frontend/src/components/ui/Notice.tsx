import type { ReactNode } from 'react'

/// One visual grammar for feedback: a left rule plus quiet text.
export function Notice({
  variant = 'info',
  children,
}: {
  variant?: 'info' | 'success' | 'warning' | 'danger'
  children: ReactNode
}) {
  const rule =
    variant === 'danger'
      ? 'border-danger'
      : variant === 'success'
        ? 'border-success'
        : variant === 'warning'
          ? 'border-warning'
          : 'border-accent'
  const text = variant === 'danger' ? 'text-danger' : 'text-ink-soft'
  return (
    <p className={`border-l-2 ${rule} ${text} pl-4 text-sm`}>{children}</p>
  )
}

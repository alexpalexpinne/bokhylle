import type { ReactNode } from 'react'

type PageHeaderProps = {
  title: string
  eyebrow?: string
  description?: string
  actions?: ReactNode
}

export function PageHeader({ title, eyebrow, description, actions }: PageHeaderProps) {
  return (
    <div className="flex flex-wrap items-end justify-between gap-4">
      <div>
        {eyebrow && (
          <p className="text-xs font-medium uppercase tracking-[0.18em] text-ink-faint">
            {eyebrow}
          </p>
        )}
        <h1 className="mt-2 font-display text-hero text-ink">{title}</h1>
        {description && (
          <p aria-live="polite" className="mt-2 max-w-2xl text-sm text-ink-muted">
            {description}
          </p>
        )}
      </div>
      {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
    </div>
  )
}

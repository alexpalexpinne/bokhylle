import type { ReactNode } from 'react'

type SectionMarkProps = {
  number?: string
  title: string
  action?: ReactNode
  className?: string
  rule?: boolean
}

/**
 * Editorial section header: optional catalogue number, title, hairline rule.
 * Replaces decorative panels as the default way to separate sections.
 */
export function SectionMark({ number, title, action, className = '', rule = true }: SectionMarkProps) {
  return (
    <>
      <div className={`flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1 ${className}`}>
        <div className="flex items-baseline gap-3">
          {number && (
            <span className="font-sans text-[11px] font-medium tabular-nums tracking-[0.2em] text-ink-faint">
              {number}
            </span>
          )}
          <h2 className="font-display text-title text-ink">{title}</h2>
        </div>
        {action && <div className="flex items-center gap-3">{action}</div>}
      </div>
      {rule && <div className="mt-3 h-px w-full bg-line" />}
    </>
  )
}

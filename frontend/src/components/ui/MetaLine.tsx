import type { ReactNode } from 'react'

type MetaLineProps = {
  items: (ReactNode | null | false)[]
  className?: string
  tone?: 'muted' | 'soft'
}

/** Catalogue-style metadata: small caps, dot separators, no pills. */
export function MetaLine({ items, className = '', tone = 'muted' }: MetaLineProps) {
  const visible = items.filter(Boolean) as ReactNode[]
  return (
    <p className={`font-sans text-[11px] uppercase tracking-[0.18em] ${tone === 'soft' ? 'text-ink-soft' : 'text-ink-muted'} ${className}`}>
      {visible.map((item, index) => (
        <span key={index}>
          {index > 0 && <span className={`mx-2 ${tone === 'soft' ? 'text-ink-soft' : 'text-ink-faint'}`}>·</span>}
          {item}
        </span>
      ))}
    </p>
  )
}

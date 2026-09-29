import { Search } from 'lucide-react'
import type { ReactNode } from 'react'

/// Two intentional search variants: `editorial` for major page search,
/// `compact` for dialogs, pickers and secondary fields.
export function SearchField({
  variant = 'editorial',
  value,
  onChange,
  placeholder,
  ariaLabel,
  action,
  autoFocus,
}: {
  variant?: 'editorial' | 'compact'
  value: string
  onChange: (value: string) => void
  placeholder: string
  ariaLabel: string
  action?: ReactNode
  autoFocus?: boolean
}) {
  if (variant === 'editorial') {
    return (
      <div className="flex items-center gap-3 border-b border-line pb-2 transition-colors focus-within:border-accent">
        <Search size={18} className="shrink-0 text-ink-faint" aria-hidden />
        <input
          value={value}
          onChange={(event) => onChange(event.target.value)}
          placeholder={placeholder}
          aria-label={ariaLabel}
          autoFocus={autoFocus}
          className="h-10 min-w-0 flex-1 bg-transparent font-display text-lg text-ink outline-none placeholder:text-ink-faint focus-visible:outline-none"
        />
        {action}
      </div>
    )
  }
  return (
    <div className="flex items-center gap-2 rounded-card bg-surface-2 px-3 py-2">
      <Search size={14} className="shrink-0 text-ink-faint" aria-hidden />
      <input
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        aria-label={ariaLabel}
        autoFocus={autoFocus}
        className="min-w-0 flex-1 bg-transparent text-sm text-ink outline-none placeholder:text-ink-faint"
      />
    </div>
  )
}

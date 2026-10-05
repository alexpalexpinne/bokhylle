/// Rectangular choice control for settings-style options (format, sort,
/// scopes). Filter tags stay pills and do not use this.
export function SegmentedControl<T extends string>({
  options,
  value,
  onChange,
  ariaLabel,
}: {
  options: { value: T; label: string }[]
  value: T
  onChange: (value: T) => void
  ariaLabel?: string
}) {
  return (
    <div role="group" aria-label={ariaLabel} className="flex flex-wrap gap-2">
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          aria-pressed={value === option.value}
          onClick={() => onChange(option.value)}
          className={`min-h-12 rounded-[3px] px-3.5 py-1.5 font-sans text-[11px] font-medium uppercase tracking-[0.14em] transition-colors sm:min-h-0 ${
            value === option.value
              ? 'bg-accent text-accent-ink'
              : 'bg-surface-2 text-ink-soft hover:bg-surface-3 hover:text-ink'
          }`}
        >
          {option.label}
        </button>
      ))}
    </div>
  )
}

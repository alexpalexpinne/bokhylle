/// Underline tabs for page-level scopes (Books/Authors, My shelf/Household,
/// My activity/Household).
export function ScopeTabs<T extends string>({
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
    <div role="group" aria-label={ariaLabel} className="flex items-center gap-6">
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          aria-pressed={value === option.value}
          onClick={() => onChange(option.value)}
          className={`border-b-2 px-0.5 pb-1 font-sans text-[11px] font-medium uppercase tracking-[0.18em] transition-colors ${
            value === option.value
              ? 'border-accent text-ink'
              : 'border-transparent text-ink-muted hover:text-ink'
          }`}
        >
          {option.label}
        </button>
      ))}
    </div>
  )
}

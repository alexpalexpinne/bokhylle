import { useState } from 'react'
import { X } from 'lucide-react'
import { SearchField } from './SearchField'

export type FacetOption = {
  value: string
  label: string
  count?: number
}

type FacetPickerProps = {
  label: string
  allLabel: string
  value: string
  options: FacetOption[]
  onChange: (value: string) => void
}

export function FacetPicker({ label, allLabel, value, options, onChange }: FacetPickerProps) {
  const [filter, setFilter] = useState('')
  const selected = options.find((option) => option.value === value)
  const needle = filter.trim().toLowerCase()
  const withResults = options.filter((option) => (option.count ?? 1) > 0)
  const visible = (
    needle
      ? withResults.filter((option) => option.label.toLowerCase().includes(needle))
      : [...withResults].sort((left, right) => (right.count ?? 0) - (left.count ?? 0))
  ).slice(0, 40)

  return (
    <div>
      <span className="text-xs uppercase tracking-[0.14em] text-ink-faint">{label}</span>

      {selected && (
        <div className="mt-2 flex items-center justify-between gap-2 rounded-full bg-accent/15 px-3 py-1.5 text-xs text-ink">
          <span className="truncate">{selected.label}</span>
          <button
            type="button"
            aria-label={`Clear ${label.toLowerCase()}`}
            onClick={() => onChange('')}
            className="rounded-full p-0.5 text-ink-muted transition-colors hover:text-ink"
          >
            <X size={13} aria-hidden />
          </button>
        </div>
      )}

      <div className="mt-2 overflow-hidden">
        <SearchField
          variant="compact"
          value={filter}
          onChange={setFilter}
          placeholder={`Search ${label.toLowerCase()}`}
          ariaLabel={`Search ${label.toLowerCase()}`}
        />
        <ul aria-label={label} className="max-h-44 overflow-y-auto pb-1">
          {!needle && (
            <li>
              <button
                type="button"
                aria-selected={value === ''}
                onClick={() => onChange('')}
                className={`flex w-full items-center px-3 py-1.5 text-left text-xs transition-colors ${
                  value === ''
                    ? 'bg-accent/15 text-ink'
                    : 'text-ink-soft hover:bg-surface-2 hover:text-ink'
                }`}
              >
                {allLabel}
              </button>
            </li>
          )}
          {visible.map((option) => (
            <li key={option.value}>
              <button
                type="button"
                aria-selected={option.value === value}
                onClick={() => onChange(option.value)}
                className={`flex w-full items-center justify-between gap-3 px-3 py-1.5 text-left text-xs transition-colors ${
                  option.value === value
                    ? 'bg-accent/15 text-ink'
                    : 'text-ink-soft hover:bg-surface-2 hover:text-ink'
                }`}
              >
                <span className="truncate">{option.label}</span>
                {option.count !== undefined && (
                  <span className="shrink-0 text-ink-faint">{option.count}</span>
                )}
              </button>
            </li>
          ))}
          {visible.length === 0 && (
            <li className="px-3 py-2 text-xs text-ink-faint">No matches</li>
          )}
        </ul>
      </div>
    </div>
  )
}

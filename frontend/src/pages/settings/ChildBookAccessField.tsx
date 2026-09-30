import { CHILD_BOOK_ACCESS, childBookAccess } from '../../lib/childBookAccess'

type Permissions = { canDiscover: boolean; canRequest: boolean }

export function ChildBookAccessField({ value, onChange }: {
  value: Permissions
  onChange: (permissions: Permissions) => void
}) {
  const mode = childBookAccess(value)
  return (
    <fieldset className="space-y-3">
      <legend className="mb-2 text-sm font-medium text-ink">How can this child find new books?</legend>
      {mode === 'browse-only' && (
        <p className="text-xs text-ink-muted">
          Current access: Explore only (existing setting). That access is kept unless you choose a mode below.
        </p>
      )}
      {CHILD_BOOK_ACCESS.map((option) => (
        <label key={option.value} className="flex cursor-pointer items-start gap-3">
          <input
            type="radio"
            name="child-book-access"
            value={option.value}
            checked={mode === option.value}
            onChange={() => onChange({ canDiscover: option.canDiscover, canRequest: option.canRequest })}
            className="mt-0.5 h-4 w-4 shrink-0 accent-accent"
          />
          <span>
            <span className="block text-sm text-ink">{option.label}</span>
            <span className="mt-1 block text-xs text-ink-muted">{option.description}</span>
          </span>
        </label>
      ))}
      <p className="text-xs text-ink-muted">
        Other household books stay hidden until assigned. Public catalogue results are not age filtered.
        Requests always need administrator approval.
      </p>
    </fieldset>
  )
}

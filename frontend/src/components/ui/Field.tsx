import type { InputHTMLAttributes, ReactNode, SelectHTMLAttributes } from 'react'

const inputClass =
  'w-full rounded-card bg-surface-2 px-4 py-2.5 text-sm text-ink outline-none placeholder:text-ink-faint focus-visible:outline-2 focus-visible:outline-focus'

export function Field({
  label,
  hint,
  children,
}: {
  label: string
  hint?: ReactNode
  children: ReactNode
}) {
  return (
    <label className="block">
      <span className="mb-1.5 block text-xs text-ink-muted">{label}</span>
      {children}
      {hint && <span className="mt-1.5 block text-xs text-ink-faint">{hint}</span>}
    </label>
  )
}

export function Input(props: InputHTMLAttributes<HTMLInputElement>) {
  return <input {...props} className={`${inputClass} ${props.className ?? ''}`.trim()} />
}

export function Select(props: SelectHTMLAttributes<HTMLSelectElement>) {
  return <select {...props} className={`${inputClass} ${props.className ?? ''}`.trim()} />
}

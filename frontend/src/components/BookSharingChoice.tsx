import { Field, Select } from './ui/Field'
import type { components } from '../api/generated'

export type BookSharing = components['schemas']['BookSharing']

export function BookSharingChoice({ value, onChange, disabled, label = 'Book sharing' }: {
  value: BookSharing
  onChange: (value: BookSharing) => void
  disabled?: boolean
  label?: string
}) {
  return (
    <Field label={label} hint={value === 'private'
      ? 'Other owners keep their access. If another owner shares this book, it stays available in the household collection.'
      : 'Make this book available in the household collection. Your personal shelf stays private.'}>
      <Select value={value} disabled={disabled} onChange={(event) => onChange(event.target.value as BookSharing)}>
        <option value="private">Private</option>
        <option value="shared">Shared with the household</option>
      </Select>
    </Field>
  )
}

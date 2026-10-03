import { LockKeyhole, UsersRound } from 'lucide-react'
import type { BookSharing } from './BookSharingChoice'

/** Read-only visibility or saved default; editing belongs to the owner's book page. */
export function BookSharingMarker({ value, label }: { value: BookSharing; label: string }) {
  const Icon = value === 'private' ? LockKeyhole : UsersRound
  return (
    <span role="img" aria-label={label} title={label}
      className="inline-flex h-11 w-11 shrink-0 items-center justify-center text-ink-muted">
      <Icon size={18} aria-hidden />
    </span>
  )
}

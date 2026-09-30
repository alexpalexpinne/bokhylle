import { useId } from 'react'
import { PROFILE_MARKS, type ProfileMarkId } from '../lib/profileMarks'
import { ProfileAvatar } from './ProfileAvatar'

export function ProfileMarkPicker({ value, onChange, username, displayName, disabled = false, optional = false }: {
  value: ProfileMarkId | null
  onChange: (value: ProfileMarkId | null) => void
  username: string
  displayName: string | null
  disabled?: boolean
  optional?: boolean
}) {
  const name = useId()
  const choiceClass = 'relative flex cursor-pointer items-center rounded-[3px] border border-line bg-surface-2/30 text-ink-soft transition-colors hover:bg-surface-2 peer-checked:border-accent peer-checked:bg-surface-2 peer-checked:text-ink peer-focus-visible:outline-2 peer-focus-visible:outline-offset-2 peer-focus-visible:outline-focus peer-disabled:cursor-default peer-disabled:opacity-60'
  return (
    <fieldset disabled={disabled}>
      <legend className="mb-2 text-xs text-ink-muted">{optional ? 'Profile mark (optional)' : 'Bokhylle profile marks'}</legend>
      <label className="relative block">
        <input type="radio" name={name} value="initial" checked={value === null} onChange={() => onChange(null)} className="peer absolute inset-0 z-10 h-full w-full cursor-pointer opacity-0 disabled:cursor-default" />
        <span className={`${choiceClass} mb-2 gap-3 px-3 py-2`}>
          <ProfileAvatar user={{ username, displayName }} className="h-9 w-9 text-base" />
          <span className="text-sm">Initials</span>
        </span>
      </label>
      <div className="grid grid-cols-5 gap-1.5">
        {PROFILE_MARKS.map((mark) => (
          <label key={mark.id} className="relative min-w-0">
            <input type="radio" name={name} value={mark.id} checked={value === mark.id} onChange={() => onChange(mark.id)} className="peer absolute inset-0 z-10 h-full w-full cursor-pointer opacity-0 disabled:cursor-default" />
            <span className={`${choiceClass} flex-col gap-1 px-0.5 py-2`}>
              <ProfileAvatar user={{ username, displayName, avatarPreset: mark.id }} className="h-12 w-12" />
              <span className="text-[11px]">{mark.label}</span>
            </span>
          </label>
        ))}
      </div>
    </fieldset>
  )
}

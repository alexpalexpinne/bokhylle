import type { ButtonHTMLAttributes } from 'react'
import type { LoginUser } from '../../api/auth'
import { ProfileAvatar } from '../../components/ProfileAvatar'

type Props = Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children' | 'className'> & {
  profile: Pick<LoginUser, 'username' | 'displayName' | 'avatarUrl' | 'avatarPreset'>
  selected?: boolean
}

export function LoginProfileButton({ profile, selected = false, hidden, ...props }: Props) {
  return (
    <button
      {...props}
      type="button"
      hidden={hidden}
      data-profile-username={profile.username}
      className={hidden ? 'hidden' : 'group flex w-24 flex-col items-center gap-3 rounded-[3px] py-1 text-center disabled:opacity-60 sm:w-28'}
    >
      <ProfileAvatar
        user={profile}
        src={profile.avatarUrl}
        className={`h-20 w-20 text-3xl ring-2 ring-offset-4 ring-offset-canvas transition-[box-shadow] ${selected ? 'ring-accent' : 'ring-transparent group-hover:ring-line'}`}
      />
      <span className={`w-full break-words font-display text-lg leading-snug transition-colors ${selected ? 'text-accent' : 'text-ink group-hover:text-accent'}`}>
        {profile.displayName || profile.username}
      </span>
    </button>
  )
}

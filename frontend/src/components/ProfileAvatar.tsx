import type { User } from '../auth/context'

type AvatarUser = Pick<User, 'username' | 'displayName'> & Partial<Pick<User, 'avatarVersion'>>

export function ProfileAvatar({ user, src, className = '' }: {
  user: AvatarUser
  src?: string | null
  className?: string
}) {
  const picture = src === undefined && user.avatarVersion != null
    ? `/api/profile/avatar?v=${user.avatarVersion}`
    : src
  return (
    <span aria-hidden="true" className={`relative flex shrink-0 items-center justify-center overflow-hidden rounded-full bg-surface-3 font-display text-ink ${className}`}>
      {(user.displayName ?? user.username).slice(0, 1).toUpperCase()}
      {picture && (
        <img
          key={`${user.username}-${picture}`}
          src={picture}
          alt=""
          className="absolute inset-0 h-full w-full object-cover"
          onError={(event) => { event.currentTarget.style.display = 'none' }}
        />
      )}
    </span>
  )
}

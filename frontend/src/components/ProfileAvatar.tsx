import type { User } from '../auth/context'

export function ProfileAvatar({ user, className = '' }: { user: User; className?: string }) {
  return (
    <span className={`relative flex shrink-0 items-center justify-center overflow-hidden rounded-full bg-surface-3 font-display text-ink ${className}`}>
      {(user.displayName ?? user.username).slice(0, 1).toUpperCase()}
      {user.avatarVersion !== null && (
        <img
          key={`${user.id}-${user.avatarVersion}`}
          src={`/api/profile/avatar?v=${user.avatarVersion}`}
          alt=""
          className="absolute inset-0 h-full w-full object-cover"
          onError={(event) => { event.currentTarget.style.display = 'none' }}
        />
      )}
    </span>
  )
}

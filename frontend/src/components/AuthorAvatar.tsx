export function AuthorAvatar({
  authorId,
  provider,
  providerKey,
  name,
  className = '',
}: {
  authorId?: number | null
  provider?: string | null
  providerKey?: string | null
  name: string
  className?: string
}) {
  const photo = authorId != null
    ? `/api/authors/${authorId}/photo`
    : provider === 'openlibrary' && providerKey
      ? `/api/discover/authors/photo?providerKey=${encodeURIComponent(providerKey)}`
      : null
  return (
    <span
      className={`relative flex shrink-0 items-center justify-center overflow-hidden rounded-full bg-surface-3 font-display text-ink ${className}`}
    >
      {name.slice(0, 1).toUpperCase()}
      {photo && (
        <img
          src={photo}
          alt=""
          loading="lazy"
          onError={(event) => {
            event.currentTarget.style.display = 'none'
          }}
          className="absolute inset-0 h-full w-full object-cover"
        />
      )}
    </span>
  )
}

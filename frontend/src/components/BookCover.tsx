import { useState, type CSSProperties } from 'react'
import { BrandMark } from './BrandMark'

type BookCoverProps = {
  src: string
  className?: string
  decorative?: boolean
  loading?: 'lazy' | 'eager'
  fetchPriority?: 'high' | 'low' | 'auto'
  onReady?: () => void
  style?: CSSProperties
}

export function BookCover({
  src,
  className = '',
  decorative = false,
  loading = 'lazy',
  fetchPriority,
  onReady,
  style,
}: BookCoverProps) {
  const [failedSrc, setFailedSrc] = useState<string | null>(null)

  if (failedSrc === src) {
    return (
      <span
        aria-hidden
        style={style}
        className={`flex aspect-[2/3] items-center justify-center rounded-[3px] bg-surface-2 ${className}`}
      >
        <BrandMark className="h-1/3 w-1/3 max-h-20 max-w-20 text-ink-faint" />
      </span>
    )
  }

  return (
    <img
      src={src}
      alt=""
      loading={loading}
      fetchPriority={fetchPriority}
      onLoad={onReady}
      onError={() => {
        setFailedSrc(src)
        onReady?.()
      }}
      style={style}
      className={`${decorative ? 'object-cover' : 'object-contain'} ${className}`}
    />
  )
}

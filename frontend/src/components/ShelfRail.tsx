import type { ReactNode } from 'react'
import { authorList } from '../api/library'
import { BookCover } from './BookCover'
import { BrandMark } from './BrandMark'
import { ShelfSurface } from './ShelfStructure'

/** A continuous shelf inside the scrolling content, shared by local and provider books. */
export function ShelfRail({ children, label, loading = false }: { children: ReactNode; label: string; loading?: boolean }) {
  return (
    <div className="shelf-rail rail-scroll" data-browse-rail={loading ? undefined : label} role={loading ? undefined : 'region'} aria-label={loading ? undefined : label} aria-hidden={loading || undefined} tabIndex={loading ? undefined : 0}>
      <div className="shelf-track"><ShelfSurface />{children}</div>
    </div>
  )
}

export function ShelfCover({ cover, fallback }: { cover: string | null; fallback?: ReactNode }) {
  return (
    <div data-book-cover className="shelf-cover-stage">
      {cover ? (
        <BookCover src={cover} />
      ) : (
        <span aria-hidden className="flex items-center justify-center rounded-[3px] bg-surface-2">
          {fallback ?? <BrandMark className="h-10 w-10 text-ink-faint" />}
        </span>
      )}
    </div>
  )
}

/** Presentation only: the caller owns navigation, actions and the source of metadata. */
export function ShelfBook({
  title,
  authors,
  cover,
  context,
  headingLevel = 3,
}: {
  title: string
  authors: string[]
  cover: string | null
  context?: ReactNode
  headingLevel?: 2 | 3
}) {
  const Heading = headingLevel === 2 ? 'h2' : 'h3'
  return (
    <>
      <ShelfCover cover={cover} />
      <div className="shelf-book-info">
        <Heading className="line-clamp-2 font-display text-base leading-snug text-ink">{title}</Heading>
        {authors.length > 0 && <p className="mt-1 line-clamp-1 text-xs text-ink-muted">{authorList(authors)}</p>}
        {context && <div className="mt-2 text-xs text-ink-muted">{context}</div>}
      </div>
    </>
  )
}

export function ShelfBookSkeleton() {
  return (
    <div className="shelf-book animate-pulse" aria-hidden>
      <div className="shelf-cover-stage">
        <span className="rounded-[3px] bg-surface-2" />
      </div>
      <div className="shelf-book-info">
        <div className="h-3.5 w-4/5 rounded bg-surface-2" />
        <div className="mt-2 h-3 w-2/5 rounded bg-surface-2" />
      </div>
    </div>
  )
}

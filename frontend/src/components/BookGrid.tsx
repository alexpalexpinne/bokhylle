import type { ReactNode } from 'react'
import { type BookSummary } from '../api/library'
import { BookCard, BookCardSkeleton } from './BookCard'
import { EmptyState } from './ui/EmptyState'
import { ShelfGrid } from './ShelfGrid'
import { ShelfBookSkeleton } from './ShelfRail'

type BookGridProps = {
  books: BookSummary[]
  loading?: boolean
  emptyMessage?: string
  emptyAction?: ReactNode
  skeletonCount?: number
  letterFor?: (book: BookSummary) => string
  className?: string
  appearance?: 'default' | 'shelf'
  selectedIds?: ReadonlySet<number>
  onSelect?: (bookId: number, selected: boolean) => void
}

export const bookGridClass =
  'grid grid-cols-2 gap-x-4 gap-y-8 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6'

export function BookGrid({
  books,
  loading,
  emptyMessage,
  emptyAction,
  skeletonCount = 6,
  letterFor,
  className = '',
  appearance = 'default',
  selectedIds,
  onSelect,
}: BookGridProps) {
  const Grid = appearance === 'shelf' ? ShelfGrid : 'div'
  const gridClass = appearance === 'shelf' ? `${className} ${onSelect ? 'shelf-grid-selecting' : ''}` : `${bookGridClass} ${className}`
  if (loading) {
    return (
      <Grid className={gridClass}>
        {Array.from({ length: skeletonCount }).map((_, index) => (
          appearance === 'shelf' ? <ShelfBookSkeleton key={index} /> : <BookCardSkeleton key={index} />
        ))}
      </Grid>
    )
  }

  if (books.length === 0) {
    return (
      <EmptyState
        className="mt-8"
        title="Nothing here yet."
        message={emptyMessage ?? 'Books will appear here once they match.'}
        action={emptyAction}
      />
    )
  }

  return (
    <Grid className={gridClass}>
      {books.map((book) => (
        <div key={book.id} data-letter={letterFor ? letterFor(book) : undefined}>
          {onSelect && <label className="mb-2 flex min-h-11 cursor-pointer items-center gap-2 text-xs text-ink-muted">
            <input type="checkbox" checked={selectedIds?.has(book.id) ?? false} onChange={(event) => onSelect(book.id, event.target.checked)} aria-label={`Select ${book.title}`} className="h-4 w-4 accent-[var(--color-accent)]" />
            Select
          </label>}
          <BookCard book={book} headingLevel={2} appearance={appearance} />
        </div>
      ))}
    </Grid>
  )
}

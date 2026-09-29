import { Link } from 'react-router-dom'
import { X } from 'lucide-react'
import { IconButton } from './ui/IconButton'
import { type BookSummary } from '../api/library'
import { BookCard, BookCardSkeleton } from './BookCard'
import { SectionMark } from './ui/SectionMark'
import { ShelfBookSkeleton, ShelfRail } from './ShelfRail'

type BookRailProps = {
  title: string
  books: BookSummary[]
  seeAllHref?: string
  loading?: boolean
  emptyMessage?: string
  onHide?: () => void
  index?: string
  appearance?: 'default' | 'shelf'
  subtitle?: string | null
}

const CARD_WIDTH = 'w-32 sm:w-36 md:w-40'

export function BookRail({
  title,
  books,
  seeAllHref,
  loading,
  emptyMessage,
  onHide,
  index,
  appearance = 'default',
  subtitle,
}: BookRailProps) {
  const action = (
    <>
      {seeAllHref && (
        <Link
          to={seeAllHref}
          className={appearance === 'shelf' ? 'text-xs font-medium text-accent hover:text-accent-strong' : 'text-xs font-medium text-ink-muted transition-colors hover:text-ink'}
        >
          See all{appearance === 'shelf' && <span aria-hidden> →</span>}
        </Link>
      )}
      {onHide && (
        <IconButton label={`Not interested in ${title}`} size="sm" onClick={onHide}>
          <X size={13} aria-hidden />
        </IconButton>
      )}
    </>
  )

  return (
    <section className={appearance === 'shelf' ? 'home-shelf-section' : undefined}>
      <div className={appearance === 'shelf' ? 'home-shelf-heading' : undefined}>
        <SectionMark
          number={index === undefined ? undefined : String(index)}
          title={title}
          action={action}
          rule={appearance !== 'shelf'}
        />
        {subtitle && <p className="mt-1 text-sm text-ink-muted">{subtitle}</p>}
      </div>

      {appearance === 'shelf' && (loading || books.length > 0) ? (
        <ShelfRail label={`${title} books`} loading={loading}>
          {loading
            ? Array.from({ length: 6 }, (_, index) => <ShelfBookSkeleton key={index} />)
            : books.map((book) => <BookCard key={book.id} book={book} appearance="shelf" />)}
        </ShelfRail>
      ) : loading ? (
        <div className="rail-scroll -mx-4 mt-4 flex gap-4 overflow-x-hidden px-4 pb-2 sm:mx-0 sm:px-0">
          {Array.from({ length: 4 }).map((_, index) => (
            <div key={index} className={`${CARD_WIDTH} shrink-0`}>
              <BookCardSkeleton />
            </div>
          ))}
        </div>
      ) : books.length === 0 ? (
        <p className={appearance === 'shelf' ? 'mt-6 text-sm text-ink-muted' : 'mt-4 rounded-card bg-surface/70 px-4 py-6 text-sm text-ink-muted'}>
          {emptyMessage ?? 'Nothing here yet.'}
        </p>
      ) : (
        <div className="rail-scroll -mx-4 mt-4 flex snap-x gap-4 overflow-x-auto px-4 pb-2 sm:mx-0 sm:px-0">
          {books.map((book) => (
            <div key={book.id} className={`${CARD_WIDTH} shrink-0 snap-start`}>
              <BookCard book={book} />
            </div>
          ))}
        </div>
      )}
    </section>
  )
}

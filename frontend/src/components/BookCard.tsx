import type { MouseEvent, ReactNode } from 'react'
import { Link } from 'react-router-dom'
import { type BookSummary, authorList, coverUrl } from '../api/library'
import { BookCover } from './BookCover'
import { ShelfBook } from './ShelfRail'

type BookCardProps = {
  book: BookSummary
  to?: string
  status?: ReactNode
  headingLevel?: 2 | 3
  appearance?: 'default' | 'shelf'
}

export function BookCard({ book, to, status, headingLevel = 3, appearance = 'default' }: BookCardProps) {
  const Heading = headingLevel === 2 ? 'h2' : 'h3'
  // Give only the clicked card the shared transition name, so the browser
  // pairs it with the Book Detail hero even when the same cover appears in
  // several rails at once.
  function prepareTransition(event: MouseEvent<HTMLAnchorElement>) {
    const cover = event.currentTarget.querySelector('[data-book-cover]')
    if (cover instanceof HTMLElement) {
      cover.style.viewTransitionName = 'book-cover'
    }
  }

  return (
    <Link
      to={to ?? `/library/${book.id}`}
      viewTransition
      className={appearance === 'shelf' ? 'shelf-book group block' : 'group block'}
      onClick={prepareTransition}
    >
      {appearance === 'shelf' ? (
        <ShelfBook title={book.title} authors={book.authors} cover={coverUrl(book.id)} context={status} headingLevel={headingLevel} />
      ) : (
        <>
          <div
            data-book-cover
            className="relative aspect-[2/3] overflow-hidden rounded-[3px] bg-surface-2 shadow-card transition-transform duration-200 ease-smooth motion-safe:group-hover:-translate-y-0.5"
          >
            <BookCover
              src={coverUrl(book.id)}
              className="h-full w-full"
            />
            {status && (
              <div className="absolute inset-x-0 bottom-0 bg-canvas/85 px-2.5 py-2 backdrop-blur-sm">
                {status}
              </div>
            )}
          </div>
          <Heading className="mt-3 line-clamp-2 font-display text-base leading-snug text-ink">
            {book.title}
          </Heading>
          <p className="mt-0.5 line-clamp-1 text-xs text-ink-soft">{authorList(book.authors)}</p>
        </>
      )}
    </Link>
  )
}

export function BookCardSkeleton() {
  return (
    <div className="animate-pulse">
      <div className="aspect-[2/3] rounded-[3px] bg-surface-2" />
      <div className="mt-3 h-3.5 w-4/5 rounded bg-surface-2" />
      <div className="mt-2 h-3 w-2/5 rounded bg-surface-2" />
    </div>
  )
}

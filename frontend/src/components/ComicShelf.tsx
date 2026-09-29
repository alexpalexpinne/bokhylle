import { useEffect, useState } from 'react'
import { Link } from 'react-router-dom'
import { ApiError } from '../api/client'
import { coverUrl } from '../api/library'
import { fetchComicShelf, type ComicShelfPage } from '../api/series'
import { BookCard } from './BookCard'
import { ShelfBook } from './ShelfRail'
import { ShelfGrid } from './ShelfGrid'
import { Button } from './ui/Button'
import { EmptyState } from './ui/EmptyState'

export function ComicShelf({ mine, user, sort }: {
  mine: boolean
  user?: number
  sort: 'recent' | 'title'
}) {
  const scopeKey = `${mine}|${user ?? ''}|${sort}`
  const [pagination, setPagination] = useState({ scopeKey, page: 1 })
  const page = pagination.scopeKey === scopeKey ? pagination.page : 1
  const requestKey = `${scopeKey}|${page}`
  const [response, setResponse] = useState<{
    key: string
    result?: ComicShelfPage
    error?: string
  } | null>(null)

  useEffect(() => {
    let cancelled = false
    fetchComicShelf(page, mine, user, sort)
      .then((value) => { if (!cancelled) setResponse({ key: requestKey, result: value }) })
      .catch((caught: unknown) => {
        if (!cancelled) setResponse({ key: requestKey, error: caught instanceof ApiError ? caught.message : 'Could not load comics' })
      })
    return () => { cancelled = true }
  }, [page, mine, user, sort, requestKey])

  const active = response?.key === requestKey ? response : null
  const error = active?.error
  const result = active?.result
  if (error) return <p role="alert" className="border-l-2 border-danger pl-4 text-sm text-danger">{error}</p>
  if (!result) return <p role="status" className="text-sm text-ink-muted">Loading comics and manga…</p>
  if (result.items.length === 0) return <EmptyState title="No comics here yet." message="Classified comics and manga on this shelf will appear here." />

  const scope = new URLSearchParams()
  if (!mine) scope.set('scope', 'household')
  if (user) scope.set('scope', `user-${user}`)
  const suffix = scope.size ? `?${scope}` : ''

  return <>
    <ShelfGrid>
      {result.items.map((item) => item.type === 'series' ? (
        <Link key={`series-${item.value.id}`} to={`/series/${item.value.id}${suffix}`} className="shelf-book group block">
          <ShelfBook
            title={item.value.name}
            authors={[]}
            cover={coverUrl(item.value.coverBookId)}
            context={`${item.value.volumeCount} ${item.value.volumeCount === 1 ? 'volume' : 'volumes'}`}
            headingLevel={2}
          />
        </Link>
      ) : (
        <BookCard key={`book-${item.value.id}`} book={item.value} appearance="shelf" headingLevel={2} />
      ))}
    </ShelfGrid>
    {result.total > result.pageSize && <div className="mt-8 flex items-center justify-center gap-4">
      <Button variant="ghost" disabled={page <= 1} onClick={() => setPagination({ scopeKey, page: page - 1 })}>Previous</Button>
      <span className="text-xs tabular-nums text-ink-muted">{page} of {Math.ceil(result.total / result.pageSize)}</span>
      <Button variant="ghost" disabled={page * result.pageSize >= result.total} onClick={() => setPagination({ scopeKey, page: page + 1 })}>Next</Button>
    </div>}
  </>
}

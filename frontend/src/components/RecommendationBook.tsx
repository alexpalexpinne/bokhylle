import { useState } from 'react'
import { Link, useLocation } from 'react-router-dom'
import { coverUrl, type SpotlightItem } from '../api/library'
import { discoverCoverUrl } from '../api/discover'
import { recommendationFeedback } from '../api/recommendations'
import { useAuth } from '../auth/useAuth'
import { useRecommendationImpression } from '../lib/useRecommendationImpressions'
import { ShelfBook } from './ShelfRail'
import { Button } from './ui/Button'
import { ActionMenu } from './ui/ActionMenu'
import type { RecommendationNotice } from './RecommendationFeedbackNotice'

export function RecommendationBook({ item, onFeedback, headingLevel = 3 }: { item: SpotlightItem; onFeedback?: (notice: RecommendationNotice) => void; headingLevel?: 2 | 3 }) {
  const location = useLocation()
  const { user } = useAuth()
  const child = user?.profileType === 'child'
  const ref = useRecommendationImpression(item.recommendationKey)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  async function feedback(action: 'like' | 'not_for_me' | 'dismiss', close: () => void) {
    if (!item.recommendationKey || busy) return
    setBusy(true)
    setError(null)
    try {
      const receipt = await recommendationFeedback(item.recommendationKey, action)
      close()
      onFeedback?.({ key: item.recommendationKey, undoToken: receipt.undoToken, title: item.title, action })
    } catch { setError('Could not update this suggestion. Please try again.') }
    finally { setBusy(false) }
  }
  return <div className="shelf-book group">
    <div ref={ref}>
      <Link className="block" to={item.bookId ? `/library/${item.bookId}` : `/discover?provider=${encodeURIComponent(item.provider ?? 'openlibrary')}&providerKey=${encodeURIComponent(item.providerKey ?? '')}`}
        state={item.bookId || child ? undefined : { backgroundLocation: location }}>
        <ShelfBook title={item.title} authors={item.authors} headingLevel={headingLevel}
          cover={item.bookId ? coverUrl(item.bookId) : item.coverId ? discoverCoverUrl(item.coverId, item.title, item.provider ?? undefined) : null}
          context={<span className="line-clamp-2">{item.reasonLabel}</span>} />
      </Link>
    </div>
    {!child && item.recommendationKey && <ActionMenu label={`Change suggestions for ${item.title}`} title={`Your suggestion: ${item.title}`} className="mt-1">
      {(close) => <>
      <p className="text-sm text-ink-muted">{item.reasonLabel}</p>
      <div className="mt-5 flex flex-col items-start gap-3">
        <Button variant="ghost" className="w-full justify-start" disabled={busy} onClick={() => void feedback('like', close)}>Like</Button>
        <Button variant="ghost" className="w-full justify-start" disabled={busy} onClick={() => void feedback('not_for_me', close)}>Not for me</Button>
        <Button variant="ghost" className="w-full justify-start" disabled={busy} onClick={() => void feedback('dismiss', close)}>Show something else</Button>
      </div>
      <p className="mt-4 text-xs text-ink-muted">Show something else sets this book aside for seven days without changing your taste.</p>
      {error && <p role="alert" className="mt-3 text-sm text-danger">{error}</p>}
      </>}
    </ActionMenu>}
  </div>
}

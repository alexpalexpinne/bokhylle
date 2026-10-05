import { useEffect, useRef, useState } from 'react'
import { Link, useLocation, useSearchParams } from 'react-router-dom'
import { ApiError } from '../api/client'
import { useAuth } from '../auth/useAuth'
import { fetchRecommendations, type RecommendationsPage } from '../api/recommendations'
import { HOME_RECOMMENDATIONS_CHANGED, homeRecommendationRevision, homeSnapshotGeneration } from '../lib/homeSnapshot'
import { browseGeneration, browseSessionGeneration, readBrowseState, saveBrowseState } from '../lib/browseState'
import { RecommendationBook } from '../components/RecommendationBook'
import { RecommendationFeedbackNotice, type RecommendationNotice } from '../components/RecommendationFeedbackNotice'
import { PageHeader } from '../components/ui/PageHeader'
import { Button } from '../components/ui/Button'
import { EmptyState } from '../components/ui/EmptyState'
import { RetryNotice } from '../components/ui/RetryNotice'
import { ShelfGrid } from '../components/ShelfGrid'
import { ShelfGridSkeleton } from '../components/ShelfGridSkeleton'

export function Recommendations() {
  const { user } = useAuth()
  const [params] = useSearchParams()
  const profile = JSON.stringify([user?.id, user?.role, user?.profileType, user?.canDiscover])
  return <RecommendationsPageView key={`${profile}|${params.get('subject') ?? ''}`} profile={profile} />
}

function RecommendationsPageView({ profile }: { profile: string }) {
  const { user } = useAuth()
  const location = useLocation()
  const [retry, setRetry] = useState(0)
  const [params, setParams] = useSearchParams()
  const subject = params.get('subject') ?? ''
  const visitKey = `recommendations:${profile}:${location.key}:${subject}`
  const viewKey = `view:${visitKey}`
  const [data, setData] = useState<RecommendationsPage | null>(() => readBrowseState<RecommendationsPage>(visitKey) ?? null)
  const [visibleCount, setVisibleCount] = useState(() => readBrowseState<number>(viewKey) ?? 24)
  const [loading, setLoading] = useState(!data?.items.length)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<RecommendationNotice | null>(null)
  const requestId = useRef(0)
  const dataRef = useRef(data)
  const dataGeneration = useRef(browseGeneration())
  const sessionGeneration = useRef(browseSessionGeneration())

  useEffect(() => {
    if (data) saveBrowseState(visitKey, data, dataGeneration.current)
    saveBrowseState(viewKey, visibleCount, sessionGeneration.current)
  }, [visitKey, viewKey, data, visibleCount])

  useEffect(() => {
    let cancelled = false
    const generation = homeSnapshotGeneration()
    async function load(feedback = false) {
      const id = ++requestId.current
      const revision = homeRecommendationRevision()
      const current = () => !cancelled && id === requestId.current && generation === homeSnapshotGeneration() && revision === homeRecommendationRevision()
      const adopt = (value: RecommendationsPage) => {
        dataGeneration.current = browseGeneration()
        dataRef.current = value
        setData(value)
      }
      setLoading(!dataRef.current?.items.length)
      setError(null)
      try {
        const cached = await fetchRecommendations(true, subject)
        if (!current()) return
        if (feedback || !dataRef.current?.items.length) adopt(cached)
        setLoading(!dataRef.current?.items.length)
        const fresh = await fetchRecommendations(false, subject)
        if (!current()) return
        // A return visit keeps its expanded selection. Explicit feedback and
        // an empty first visit can replace books; impressions cannot reorder it.
        if (feedback || !dataRef.current?.items.length) adopt(fresh)
      } catch (caught) {
        if (current()) setError(caught instanceof ApiError ? caught.message : 'Could not load your suggestions')
      } finally { if (current()) setLoading(false) }
    }
    void load()
    const changed = () => { void load(true) }
    window.addEventListener(HOME_RECOMMENDATIONS_CHANGED, changed)
    return () => { cancelled = true; window.removeEventListener(HOME_RECOMMENDATIONS_CHANGED, changed) }
  }, [subject, retry])

  function feedback(next: RecommendationNotice) {
    setNotice(next)
    if (next.action !== 'like' && dataRef.current) {
      const nextData = { ...dataRef.current, items: dataRef.current.items.filter((item) => item.recommendationKey !== next.key) }
      dataRef.current = nextData
      setData(nextData)
    }
  }
  const subjects = Array.from(new Set([...(data?.subjects ?? []), ...(subject ? [subject] : [])]))

  return <>
    <PageHeader title="Picked for you" description="Books selected from your reading interests, likes, and followed authors." actions={<Link className="inline-flex min-h-12 items-center text-sm text-accent" to="/discover">Search the catalogue</Link>} />
    <label className="mt-6 flex flex-wrap items-center gap-3 text-sm text-ink-muted">Reading interest
      <select className="min-h-12 max-w-full rounded-[3px] border border-line bg-surface px-3 py-2 text-ink" value={subject} onChange={(event) => setParams(event.target.value ? { subject: event.target.value } : {})}>
        <option value="">All interests</option>
        {subjects.map((topic) => <option key={topic} value={topic}>{topic}</option>)}
      </select>
    </label>
    {error && <RetryNotice className="mt-4" message={error} busy={loading} onRetry={() => setRetry((count) => count + 1)} />}
    {data && data.items.length > 0 ? <>
      <ShelfGrid className="mt-8">
        {data.items.slice(0, visibleCount).map((item) => <RecommendationBook key={item.recommendationKey ?? `${item.provider}-${item.providerKey}-${item.bookId}`} item={item} onFeedback={feedback} headingLevel={2} />)}
      </ShelfGrid>
      {data.items.length > visibleCount && <Button className="mt-8" variant="secondary" onClick={() => setVisibleCount((count) => count + 24)}>Show more suggestions</Button>}
    </> : loading ? <>
      <p className="sr-only" role="status">Finding books for you…</p>
      <ShelfGridSkeleton className="mt-8" />
    </> : !error ? <EmptyState className="mt-10" title={subject ? 'No suggestions for this interest' : 'No suggestions yet'} message={subject ? 'Try all interests, or choose a different reading interest.' : 'Like a few books or follow an author to help choose your next read.'} action={subject ? <Button onClick={() => setParams({})}>Show all interests</Button> : <Link className="inline-flex min-h-12 items-center text-accent" to={user?.profileType === 'child' ? '/discover' : '/welcome'}>{user?.profileType === 'child' ? 'Search the catalogue' : 'Choose reading interests'}</Link>} /> : null}
    {notice && <RecommendationFeedbackNotice notice={notice} onDismiss={() => setNotice((current) => current?.undoToken === notice.undoToken ? null : current)} />}
  </>
}

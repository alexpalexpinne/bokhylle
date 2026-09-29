import { useEffect, useState } from 'react'
import { Link, useSearchParams, useParams } from 'react-router-dom'
import { ArrowLeft } from 'lucide-react'
import { ApiError } from '../api/client'
import { authorList, coverUrl } from '../api/library'
import { setBookCompletion } from '../api/reader'
import { fetchComicSeries, updateAdminSeries, type SeriesDetail } from '../api/series'
import { useAuth } from '../auth/useAuth'
import { BookCard } from '../components/BookCard'
import { BookCover } from '../components/BookCover'
import { ShelfGrid } from '../components/ShelfGrid'
import { Button, ButtonLink } from '../components/ui/Button'
import { Modal } from '../components/ui/Modal'
import { PageHeader } from '../components/ui/PageHeader'
import { SectionMark } from '../components/ui/SectionMark'

export function ComicSeries() {
  const { seriesId } = useParams()
  const id = Number(seriesId)
  const [searchParams] = useSearchParams()
  const { user } = useAuth()
  const scope = searchParams.get('scope')
  const member = user?.role === 'admin' && scope?.startsWith('user-') ? Number(scope.slice(5)) : undefined
  const mine = user?.profileType === 'child' || (scope !== 'household' && member === undefined)
  const requestKey = `${id}|${mine}|${member ?? ''}`
  const [loaded, setLoaded] = useState<{ key: string, series?: SeriesDetail, error?: string } | null>(null)
  const [savingVolume, setSavingVolume] = useState<number | null>(null)
  const [progressError, setProgressError] = useState<string | null>(null)
  const [edit, setEdit] = useState(false)
  const [seriesName, setSeriesName] = useState('')
  const [sortName, setSortName] = useState('')
  const [direction, setDirection] = useState<'' | 'ltr' | 'rtl'>('')
  const [saving, setSaving] = useState(false)
  const [saveError, setSaveError] = useState<string | null>(null)

  useEffect(() => {
    if (!Number.isSafeInteger(id) || id <= 0) return
    let cancelled = false
    fetchComicSeries(id, mine, member)
      .then((value) => { if (!cancelled) setLoaded({ key: requestKey, series: value }) })
      .catch((caught: unknown) => {
        if (!cancelled) setLoaded({ key: requestKey, error: caught instanceof ApiError ? caught.message : 'Could not load this series' })
      })
    return () => { cancelled = true }
  }, [id, mine, member, requestKey])

  const active = loaded?.key === requestKey ? loaded : null
  const series = active?.series
  const error = active?.error
  if (!Number.isSafeInteger(id) || id <= 0) return <p className="text-danger">Invalid series link.</p>
  if (error) return <p role="alert" className="border-l-2 border-danger pl-4 text-sm text-danger">{error}</p>
  if (!series) return <p role="status" className="text-sm text-ink-muted">Loading series…</p>

  const current = series.volumes.find((volume) => volume.id === series.reading.current?.bookId)
  const next = series.volumes.find((volume) => volume.id === series.reading.nextBookId)
  const allFinished = series.reading.finishedBookIds.length === series.volumes.length
  const featured = current ?? next ?? series.volumes.find((volume) => !series.reading.finishedBookIds.includes(volume.id)) ?? series.volumes[0]
  const continueFile = current && series.reading.current?.browserFileId
  const authors = series.volumes[0]?.authors ?? []
  const libraryQuery = scope ? `?scope=${encodeURIComponent(scope)}&category=comics` : '?category=comics'

  async function saveSeries() {
    setSaving(true)
    setSaveError(null)
    try {
      const updated = await updateAdminSeries(id, {
        name: seriesName,
        sortName: sortName.trim() || null,
        defaultReadingDirection: direction || null,
      })
      setLoaded({ key: requestKey, series: { ...series!, name: updated.name, sortName: updated.sortName, defaultReadingDirection: updated.defaultReadingDirection } })
      setEdit(false)
    } catch (caught) {
      setSaveError(caught instanceof ApiError ? caught.message : 'Could not save this series')
    } finally {
      setSaving(false)
    }
  }

  async function toggleFinished(bookId: number, finished: boolean) {
    setSavingVolume(bookId)
    setProgressError(null)
    try {
      await setBookCompletion(bookId, !finished)
      const updated = await fetchComicSeries(id, mine, member)
      setLoaded({ key: requestKey, series: updated })
    } catch (caught) {
      setProgressError(caught instanceof ApiError ? caught.message : 'Could not update reading progress')
    } finally {
      setSavingVolume(null)
    }
  }

  return <article className="space-y-10">
    <Link to={`/library${libraryQuery}`} className="inline-flex items-center gap-1.5 text-xs text-ink-muted hover:text-ink">
      <ArrowLeft size={14} /> Library
    </Link>
    <PageHeader
      eyebrow="Series"
      title={series.name}
      description={`${authorList(authors)}${authors.length ? ' · ' : ''}${series.volumes.length} ${series.volumes.length === 1 ? 'volume' : 'volumes'}${series.defaultReadingDirection === 'rtl' ? ' · Right to left' : ''}`}
      actions={user?.role === 'admin' ? <Button variant="ghost" size="sm" onClick={() => {
        setSeriesName(series.name)
        setSortName(series.sortName ?? '')
        setDirection(series.defaultReadingDirection === 'ltr' || series.defaultReadingDirection === 'rtl' ? series.defaultReadingDirection : '')
        setEdit(true)
      }}>Fix series</Button> : undefined}
    />
    <div className="flex flex-wrap items-center gap-7 border-b border-line pb-8">
      <BookCover src={coverUrl(featured.id)} className="h-44 w-32 shrink-0 shadow-card" />
      <div className="space-y-3">
        <p className="text-xs uppercase tracking-[0.14em] text-ink-muted">{current ? 'Continue reading' : next ? 'Next in series' : allFinished ? 'Series complete' : 'Start reading'}</p>
        <p className="font-display text-xl">{featured.title}</p>
        <ButtonLink to={current && continueFile ? `/read/${current.id}/${continueFile}` : `/library/${featured.id}`} variant="primary">
          {current && continueFile ? 'Continue reading' : 'Open volume'}
        </ButtonLink>
        {next && next.id !== featured.id && <p className="text-sm text-ink-soft">Next in series: <Link className="text-accent hover:underline" to={`/library/${next.id}`}>{next.title}</Link></p>}
        {series.reading.missingNextVolume && <p className="text-sm text-ink-soft">Volume {series.reading.missingNextVolume} is missing. No next volume is suggested yet.</p>}
      </div>
    </div>
    <section>
      <SectionMark title="Volumes" />
      <p className="mt-2 text-sm text-ink-muted">Your progress · {series.reading.finishedBookIds.length} of {series.volumes.length} finished</p>
      {progressError && <p role="alert" className="mt-3 text-sm text-danger">{progressError}</p>}
      <ShelfGrid className="mt-7">
        {series.volumes.map((volume) => {
          const finished = series.reading.finishedBookIds.includes(volume.id)
          const reading = series.reading.current?.bookId === volume.id
          return <div key={volume.id} className="space-y-2">
            <BookCard book={volume} appearance="shelf" headingLevel={2} status={[volume.seriesNumber ? `Volume ${volume.seriesNumber}` : null, finished ? 'Finished' : reading ? 'Reading' : null].filter(Boolean).join(' · ')} />
            <Button variant="ghost" size="sm" disabled={savingVolume !== null} onClick={() => void toggleFinished(volume.id, finished)} aria-label={`${finished ? 'Mark unfinished' : 'Mark finished'}: ${volume.title}`}>
              {finished ? 'Mark unfinished' : 'Mark finished'}
            </Button>
          </div>
        })}
      </ShelfGrid>
    </section>
    {edit && <Modal title="Fix series" description="Correct the local series identity and its inherited reading direction." onClose={() => setEdit(false)} footer={<>
      <Button variant="ghost" onClick={() => setEdit(false)}>Cancel</Button>
      <Button variant="primary" disabled={saving || !seriesName.trim()} onClick={() => void saveSeries()}>{saving ? 'Saving…' : 'Save'}</Button>
    </>}>
      {saveError && <p role="alert" className="mb-4 text-sm text-danger">{saveError}</p>}
      <div className="space-y-4">
        <label className="block text-sm">Name<input value={seriesName} onChange={(event) => setSeriesName(event.target.value)} className="mt-1 block w-full rounded-card bg-surface-2 px-3 py-2 text-ink" /></label>
        <label className="block text-sm">Sort name<input value={sortName} onChange={(event) => setSortName(event.target.value)} className="mt-1 block w-full rounded-card bg-surface-2 px-3 py-2 text-ink" /></label>
        <label className="block text-sm">Default reading direction<select value={direction} onChange={(event) => setDirection(event.target.value as '' | 'ltr' | 'rtl')} className="mt-1 block w-full rounded-card bg-surface-2 px-3 py-2 text-ink">
          <option value="">Inherit from file</option><option value="ltr">Left to right</option><option value="rtl">Right to left</option>
        </select></label>
      </div>
    </Modal>}
  </article>
}

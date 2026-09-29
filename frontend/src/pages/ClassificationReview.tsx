import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { Link, useSearchParams } from 'react-router-dom'
import { ApiError } from '../api/client'
import {
  type ClassificationDecision,
  type ClassificationReviewItem,
  type ClassificationReviewPage,
  type ReviewAttention,
  type ReviewFilters,
  type ReviewStatus,
  type SuggestedKind,
  fetchClassificationReview,
  saveClassificationReview,
} from '../api/classification'
import { coverUrl } from '../api/library'
import { fetchAdminSeries, type SeriesRecord } from '../api/series'
import { BookCover } from '../components/BookCover'
import { Button, ButtonLink } from '../components/ui/Button'
import { Modal } from '../components/ui/Modal'
import { PageHeader } from '../components/ui/PageHeader'
import { ScopeTabs } from '../components/ui/ScopeTabs'

const MAX_BULK = 1000
const BULK_FETCH_SIZE = 500
const inputClass = 'w-full rounded-card border border-line bg-surface px-3 py-2 text-sm text-ink outline-none focus-visible:outline-2 focus-visible:outline-focus'

type Draft = {
  kind: string
  seriesChoice: string
  newSeriesName: string
  volume: string
  sortOrder: string
  direction: '' | 'ltr' | 'rtl'
}

type Preview = {
  action: 'apply' | 'dismiss'
  rows: { item: ClassificationReviewItem; decision: ClassificationDecision }[]
}

function initialDraft(item: ClassificationReviewItem, series: SeriesRecord[]): Draft {
  const name = item.seriesName ?? item.suggestion.seriesName ?? ''
  const match = series.filter((entry) => entry.name.toLocaleLowerCase() === name.toLocaleLowerCase())
  return {
    kind: item.publicationKind === 'unknown' ? item.suggestion.publicationKind : item.publicationKind,
    seriesChoice: item.seriesId ? String(item.seriesId) : name ? match.length === 1 ? String(match[0].id) : 'new' : '',
    newSeriesName: name && match.length !== 1 && !item.seriesId ? name : '',
    volume: item.seriesNumber ?? item.suggestion.seriesNumber ?? '',
    sortOrder: item.seriesSortOrder == null ? '' : String(item.seriesSortOrder),
    direction: item.readingDirection === 'rtl' || item.readingDirection === 'ltr' ? item.readingDirection : '',
  }
}

function decision(item: ClassificationReviewItem, draft: Draft, action: 'apply' | 'dismiss'): ClassificationDecision {
  if (action === 'dismiss') return { bookId: item.id, action, onlyIfPending: item.reviewedAt === null }
  return {
    bookId: item.id,
    action,
    onlyIfPending: item.reviewedAt === null,
    publicationKind: draft.kind,
    seriesId: draft.seriesChoice && draft.seriesChoice !== 'new' ? Number(draft.seriesChoice) : undefined,
    newSeriesName: draft.seriesChoice === 'new' ? draft.newSeriesName.trim() : undefined,
    seriesNumber: draft.volume.trim() || null,
    seriesSortOrder: draft.sortOrder.trim() ? Number(draft.sortOrder) : undefined,
    readingDirection: draft.direction || undefined,
  }
}

function volumeChange(draft: Draft, volume: string): Partial<Draft> {
  const oldNumber = Number(draft.volume)
  const keptInSync = draft.sortOrder === '' || (Number.isFinite(oldNumber) && draft.sortOrder === String(oldNumber))
  return { volume, sortOrder: keptInSync ? '' : draft.sortOrder }
}

function kindLabel(kind: string | null | undefined): string {
  return kind === 'manga' ? 'Manga' : kind === 'comic' ? 'Comic' : kind === 'book' ? 'Book' : kind ?? 'Unknown'
}

function previewDetail(row: Preview['rows'][number], series: SeriesRecord[]): string {
  if (row.decision.action === 'dismiss') return 'Details unchanged'
  const details = [kindLabel(row.decision.publicationKind)]
  const name = row.decision.newSeriesName
    ?? series.find((entry) => entry.id === row.decision.seriesId)?.name
  if (name) details.push(name)
  if (row.decision.seriesNumber) details.push(`Vol. ${row.decision.seriesNumber}`)
  if (row.decision.seriesSortOrder != null) details.push(`Order ${row.decision.seriesSortOrder}`)
  if (row.decision.readingDirection) details.push(row.decision.readingDirection.toUpperCase())
  return details.join(' · ')
}

export function ClassificationReview() {
  const [searchParams, setSearchParams] = useSearchParams()
  const status: ReviewStatus = searchParams.get('status') === 'all' ? 'all' : 'pending'
  const kindParam = searchParams.get('kind')
  const kind: SuggestedKind = kindParam === 'book' || kindParam === 'comic' || kindParam === 'manga' ? kindParam : 'all'
  const attentionParam = searchParams.get('attention')
  const attention: ReviewAttention = attentionParam === 'simple' || attentionParam === 'review' ? attentionParam : 'all'
  const filters: ReviewFilters = { status, kind, attention }
  const rawPage = Number(searchParams.get('page') ?? '1')
  const page = Number.isSafeInteger(rawPage) && rawPage > 0 ? rawPage : 1
  const [result, setResult] = useState<{ key: string; data?: ClassificationReviewPage; series?: SeriesRecord[]; error?: string } | null>(null)
  const [drafts, setDrafts] = useState<Record<number, Draft>>({})
  const [selected, setSelected] = useState<Record<number, ClassificationReviewItem>>({})
  const [preview, setPreview] = useState<Preview | null>(null)
  const [busy, setBusy] = useState(false)
  const [selectingAll, setSelectingAll] = useState(false)
  const [saveError, setSaveError] = useState<string | null>(null)
  const [reload, setReload] = useState(0)
  const selectionRequestRef = useRef(0)
  const filterRef = useRef(filters)
  const key = `${status}|${kind}|${attention}|${page}|${reload}`

  useEffect(() => { filterRef.current = { status, kind, attention } }, [status, kind, attention])

  useEffect(() => {
    let cancelled = false
    Promise.all([fetchClassificationReview(page, { status, kind, attention }), fetchAdminSeries()])
      .then(([data, series]) => {
        if (cancelled) return
        setResult({ key, data, series })
        setDrafts((current) => {
          const next = { ...current }
          for (const item of data.items) next[item.id] ??= initialDraft(item, series)
          return next
        })
      })
      .catch((caught: unknown) => {
        if (!cancelled) setResult({ key, error: caught instanceof ApiError ? caught.message : 'Could not load import review' })
      })
    return () => { cancelled = true }
  }, [key, page, status, kind, attention])

  const active = result?.key === key ? result : null
  const items = active?.data?.items ?? []
  const total = active?.data?.total ?? 0
  const selectedItems = Object.values(selected)
  const selectedOnPage = items.filter((item) => selected[item.id])
  const allOnPage = items.length > 0 && selectedOnPage.length === items.length

  function writeLocation(next: ReviewFilters, nextPage: number) {
    const params = new URLSearchParams()
    if (next.status === 'all') params.set('status', 'all')
    if (next.kind !== 'all') params.set('kind', next.kind)
    if (next.attention !== 'all') params.set('attention', next.attention)
    if (nextPage > 1) params.set('page', String(nextPage))
    setSearchParams(params)
    setSaveError(null)
  }

  function changeFilters(patch: Partial<ReviewFilters>) {
    const next = { ...filterRef.current, ...patch }
    filterRef.current = next
    selectionRequestRef.current += 1
    setSelectingAll(false)
    setSelected({})
    setDrafts({})
    setPreview(null)
    writeLocation(next, 1)
  }

  function updateDraft(id: number, update: Partial<Draft>) {
    setDrafts((current) => ({ ...current, [id]: { ...current[id], ...update } }))
  }

  function togglePage() {
    setSelected((current) => {
      const next = { ...current }
      for (const item of items) {
        if (allOnPage) delete next[item.id]
        else next[item.id] = item
      }
      return next
    })
  }

  async function selectAllMatching() {
    if (!active?.data || total === 0) return
    if (total > MAX_BULK) {
      setSaveError(`This filter has ${total} files. Narrow it to ${MAX_BULK} or fewer before selecting all.`)
      return
    }
    const requestId = ++selectionRequestRef.current
    setSelectingAll(true)
    setSaveError(null)
    try {
      const all: ClassificationReviewItem[] = []
      for (let nextPage = 1; all.length < total; nextPage += 1) {
        const batch = await fetchClassificationReview(nextPage, filters, BULK_FETCH_SIZE)
        if (requestId !== selectionRequestRef.current) return
        if (batch.total !== total || batch.items.length === 0) throw new Error('The review list changed. Refresh and select again.')
        all.push(...batch.items)
      }
      const unique = new Map(all.map((item) => [item.id, item]))
      if (unique.size !== total) throw new Error('The review list changed. Refresh and select again.')
      setDrafts((current) => {
        const next = { ...current }
        for (const item of unique.values()) next[item.id] ??= initialDraft(item, active.series ?? [])
        return next
      })
      setSelected(Object.fromEntries(unique.entries()))
    } catch (caught) {
      setSaveError(caught instanceof ApiError || caught instanceof Error ? caught.message : 'Could not select matching files')
    } finally {
      if (requestId === selectionRequestRef.current) setSelectingAll(false)
    }
  }

  function openPreview(action: 'apply' | 'dismiss', chosen: ClassificationReviewItem[]) {
    if (chosen.length === 0 || chosen.length > MAX_BULK) {
      setSaveError(`Select 1–${MAX_BULK} files.`)
      return
    }
    const rows = chosen.map((item) => ({
      item,
      decision: decision(item, drafts[item.id] ?? initialDraft(item, active?.series ?? []), action),
    }))
    const invalid = rows.find(({ decision: choice }) => choice.action === 'apply'
      && (choice.newSeriesName === ''
        || (choice.newSeriesName?.length ?? 0) > 200
        || (choice.seriesNumber?.length ?? 0) > 40
        || (choice.seriesSortOrder != null && !Number.isFinite(choice.seriesSortOrder))))
    if (invalid) {
      setSaveError(`Check the series name and sort order for ${invalid.item.title}.`)
      return
    }
    setSaveError(null)
    setPreview({ action, rows })
  }

  async function commit(decisions: ClassificationDecision[]) {
    setBusy(true)
    setSaveError(null)
    try {
      await saveClassificationReview(decisions)
      setPreview(null)
      setSelected({})
      setDrafts({})
      if (page > 1) writeLocation(filters, 1)
      setReload((current) => current + 1)
    } catch (caught) {
      setSaveError(caught instanceof ApiError ? caught.message : 'Could not save the review')
    } finally {
      setBusy(false)
    }
  }

  const counts = active?.data?.counts
  const previewNeedsReview = preview?.rows.filter((row) => row.item.suggestion.needsReview).length ?? 0
  const previewKinds = preview?.rows.reduce((result, row) => {
    const kind = row.decision.publicationKind ?? 'unknown'
    result[kind] = (result[kind] ?? 0) + 1
    return result
  }, {} as Record<string, number>) ?? {}

  return (
    <section>
      <PageHeader
        eyebrow="Library · Administration"
        title="Review imports"
        description="Check publication type, series and volume before grouping new files. Suggestions come from embedded metadata, titles and filenames; nothing is applied until you accept it."
        actions={<ButtonLink to="/library" variant="ghost">Back to library</ButtonLink>}
      />
      <div className="mt-8 border-b border-line">
        <ScopeTabs ariaLabel="Review status" value={status} onChange={(value) => changeFilters({ status: value })} options={[
          { value: 'pending', label: 'Needs review' },
          { value: 'all', label: 'All files' },
        ]} />
      </div>
      {active?.error && <p role="alert" className="mt-6 border-l-2 border-danger pl-4 text-sm text-danger">{active.error}</p>}
      {!active && <p role="status" className="mt-6 text-sm text-ink-muted">Loading imports…</p>}
      {active?.data && <>
        <div className="mt-7 overflow-x-auto border-b border-line">
          <ScopeTabs ariaLabel="Suggested publication type" value={kind} onChange={(value) => changeFilters({ kind: value })} options={[
            { value: 'all', label: `All ${counts?.total ?? 0}` },
            { value: 'book', label: `Books ${counts?.book ?? 0}` },
            { value: 'comic', label: `Comics ${counts?.comic ?? 0}` },
            { value: 'manga', label: `Manga ${counts?.manga ?? 0}` },
          ]} />
        </div>
        <div className="mt-5 flex flex-wrap items-end justify-between gap-4">
          <label className="block text-xs text-ink-muted">
            Review priority
            <select aria-label="Review priority" className={`mt-1 block min-w-52 ${inputClass}`} value={attention} onChange={(event) => changeFilters({ attention: event.target.value as ReviewAttention })}>
              <option value="all">All clues</option>
              <option value="simple">Straightforward · {counts?.simple ?? 0}</option>
              <option value="review">Closer look · {counts?.needsReview ?? 0}</option>
            </select>
          </label>
          <p className="max-w-lg text-xs text-ink-muted">Straightforward means a suggested book without a series clue. Comics, manga and series clues stay in Closer look.</p>
        </div>
        <div className="mt-7 flex flex-wrap items-center justify-between gap-3">
          <p className="text-xs text-ink-muted">{total} matching {status === 'pending' ? 'awaiting review' : 'library files'} · Page {page}</p>
          {items.length > 0 && <div className="flex flex-wrap items-center gap-3">
            <label className="flex items-center gap-2 text-xs text-ink-soft">
              <input type="checkbox" checked={allOnPage} onChange={togglePage} />
              Select this page
            </label>
            {total > items.length && <Button size="sm" variant="ghost" disabled={selectingAll || selectedItems.length === total} onClick={() => void selectAllMatching()}>
              {selectingAll ? 'Selecting…' : selectedItems.length === total ? `All ${total} selected` : `Select all ${total} matching`}
            </Button>}
          </div>}
        </div>
        {saveError && !preview && <p role="alert" className="mt-5 border-l-2 border-danger pl-4 text-sm text-danger">{saveError}</p>}
        {items.length === 0 && <p className="mt-10 text-sm text-ink-muted">{total === 0 && status === 'pending' && kind === 'all' && attention === 'all' ? 'Everything has been reviewed.' : 'No files match these filters.'}</p>}
        <div className="mt-5 divide-y divide-line border-y border-line">
          {items.map((item) => {
            const draft = drafts[item.id]
            if (!draft) return null
            return <article key={item.id} className="py-6">
              <div className="flex gap-4 sm:gap-6">
                <input
                  type="checkbox"
                  aria-label={`Select ${item.title}`}
                  checked={Boolean(selected[item.id])}
                  onChange={(event) => setSelected((current) => {
                    const next = { ...current }
                    if (event.target.checked) next[item.id] = item
                    else delete next[item.id]
                    return next
                  })}
                  className="mt-2 self-start"
                />
                <Link to={`/library/${item.id}`} className="h-28 w-[75px] shrink-0 bg-surface-2 sm:h-32 sm:w-[86px]">
                  <BookCover src={coverUrl(item.id)} className="h-full w-full" />
                </Link>
                <div className="min-w-0 flex-1">
                  <Link to={`/library/${item.id}`} className="font-display text-lg text-ink hover:text-accent">{item.title}</Link>
                  <p className="mt-1 break-all text-xs text-ink-muted">{item.fileName ?? 'Library file'} · {item.format?.toUpperCase() ?? 'Unknown format'}</p>
                  <p className="mt-3 text-sm text-ink-soft">Suggested: {kindLabel(item.suggestion.publicationKind)}{item.suggestion.seriesName ? ` · ${item.suggestion.seriesName}` : ''}{item.suggestion.seriesNumber ? ` · Vol. ${item.suggestion.seriesNumber}` : ''}</p>
                  <p className="mt-1 text-xs text-ink-muted">{item.suggestion.needsReview ? 'Closer look' : 'Straightforward'} · {item.suggestion.reason}</p>
                  <details className="mt-4">
                    <summary className="w-fit cursor-pointer text-xs font-medium text-accent hover:text-accent-strong">Review details</summary>
                    <div className="mt-4 grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
                      <label className="text-xs text-ink-muted">Publication type
                        <select className={`mt-1 ${inputClass}`} value={draft.kind} onChange={(event) => updateDraft(item.id, { kind: event.target.value })}>
                          <option value="book">Book</option><option value="comic">Comic</option><option value="manga">Manga</option><option value="magazine">Magazine</option><option value="catalogue">Catalogue</option>
                        </select>
                      </label>
                      <label className="text-xs text-ink-muted">Series
                        <select className={`mt-1 ${inputClass}`} value={draft.seriesChoice} onChange={(event) => updateDraft(item.id, { seriesChoice: event.target.value })}>
                          <option value="">No series</option>
                          {active.series?.map((entry) => <option key={entry.id} value={entry.id}>{entry.name}</option>)}
                          <option value="new">New series…</option>
                        </select>
                      </label>
                      {draft.seriesChoice === 'new' && <label className="text-xs text-ink-muted">New series name
                        <input className={`mt-1 ${inputClass}`} value={draft.newSeriesName} onChange={(event) => updateDraft(item.id, { newSeriesName: event.target.value })} maxLength={200} />
                      </label>}
                      <label className="text-xs text-ink-muted">Volume label
                        <input className={`mt-1 ${inputClass}`} value={draft.volume} onChange={(event) => updateDraft(item.id, volumeChange(draft, event.target.value))} maxLength={40} placeholder="e.g. 7 or 1.5" />
                      </label>
                      <label className="text-xs text-ink-muted">Sort order
                        <input className={`mt-1 ${inputClass}`} type="number" step="any" value={draft.sortOrder} onChange={(event) => updateDraft(item.id, { sortOrder: event.target.value })} placeholder="From volume label" />
                      </label>
                      <label className="text-xs text-ink-muted">Reading direction
                        <select className={`mt-1 ${inputClass}`} value={draft.direction} onChange={(event) => updateDraft(item.id, { direction: event.target.value as Draft['direction'] })}>
                          <option value="">Inherit</option><option value="ltr">Left to right</option><option value="rtl">Right to left</option>
                        </select>
                      </label>
                    </div>
                    {item.legacySeriesText && <p className="mt-3 text-xs text-ink-muted">Imported series text: {item.legacySeriesText}</p>}
                  </details>
                  <div className="mt-4 flex flex-wrap gap-2">
                    <Button size="sm" variant="primary" disabled={busy || selectingAll} onClick={() => openPreview('apply', [item])}>Accept</Button>
                    <Button size="sm" variant="ghost" disabled={busy || selectingAll} onClick={() => openPreview('dismiss', [item])}>Dismiss</Button>
                  </div>
                </div>
              </div>
            </article>
          })}
        </div>
        {selectedItems.length > 0 && <div className="sticky bottom-20 z-10 mt-5 flex flex-wrap items-center gap-2 border border-line bg-canvas p-3 shadow-card md:bottom-4">
          <span className="mr-auto text-xs text-ink-soft">{selectedItems.length} selected across this filter</span>
          <Button size="sm" variant="ghost" disabled={busy || selectingAll} onClick={() => setSelected({})}>Clear</Button>
          <Button size="sm" variant="ghost" disabled={busy || selectingAll} onClick={() => openPreview('dismiss', selectedItems)}>Preview dismissal</Button>
          <Button size="sm" variant="primary" disabled={busy || selectingAll} onClick={() => openPreview('apply', selectedItems)}>Preview acceptance</Button>
        </div>}
        {total > active.data.pageSize && <nav aria-label="Review pages" className="mt-8 flex justify-end gap-3">
          <Button size="sm" variant="ghost" disabled={page <= 1} onClick={() => writeLocation(filters, page - 1)}>Previous</Button>
          <Button size="sm" variant="ghost" disabled={page * active.data!.pageSize >= total} onClick={() => writeLocation(filters, page + 1)}>Next</Button>
        </nav>}
      </>}
      {preview && createPortal(
        <Modal
          title={preview.action === 'apply' ? `Accept ${preview.rows.length} ${preview.rows.length === 1 ? 'file' : 'files'}?` : `Dismiss ${preview.rows.length} ${preview.rows.length === 1 ? 'file' : 'files'}?`}
          description={preview.action === 'apply' ? 'Review the changes below before saving them together.' : 'These files will leave the pending queue. Their details will stay as they are.'}
          wide
          onClose={() => { if (!busy) { setPreview(null); setSaveError(null) } }}
          footer={<>
            <Button variant="ghost" disabled={busy} onClick={() => { setPreview(null); setSaveError(null) }}>Cancel</Button>
            <Button variant="primary" disabled={busy} onClick={() => void commit(preview.rows.map((row) => row.decision))}>{busy ? 'Saving…' : `Confirm ${preview.rows.length}`}</Button>
          </>}
        >
          {saveError && <p role="alert" className="mb-4 border-l-2 border-danger pl-4 text-sm text-danger">{saveError}</p>}
          {preview.action === 'apply' && <>
            <p className="text-sm text-ink-soft">{Object.entries(previewKinds).map(([kind, count]) => `${count} ${kindLabel(kind).toLowerCase()}`).join(' · ')}</p>
            {previewNeedsReview > 0 && <p className="mt-3 border-l-2 border-accent pl-4 text-sm text-ink-soft">{previewNeedsReview} selected {previewNeedsReview === 1 ? 'file has' : 'files have'} comic, manga, or series clues. Check their grouping and volume before confirming.</p>}
          </>}
          <div className="mt-5 max-h-[42vh] overflow-y-auto border-y border-line">
            {preview.rows.map((row) => <div key={row.item.id} className="grid gap-1 border-b border-line py-2.5 text-sm last:border-b-0 sm:grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)] sm:gap-4">
              <span className="min-w-0 truncate text-ink">{row.item.title}</span>
              <span className="text-ink-muted">{previewDetail(row, active?.series ?? [])}</span>
            </div>)}
          </div>
        </Modal>,
        document.body,
      )}
    </section>
  )
}

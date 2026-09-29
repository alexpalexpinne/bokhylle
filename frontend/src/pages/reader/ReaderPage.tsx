import { useCallback, useEffect, useRef, useState } from 'react'
import { Link, useParams } from 'react-router-dom'
import { ArrowLeft, BookOpen, Check, ChevronLeft, ChevronRight, Maximize2, Search, Settings2 } from 'lucide-react'
import { ApiError } from '../../api/client'
import { fetchBook, type BookDetail } from '../../api/library'
import {
  fetchBrowserPosition,
  fetchEpubBytes,
  contentUrl,
  positionUrl,
  saveBrowserPosition,
  setBookCompletion,
  setReadingDirection,
  comicPageUrl,
  type BrowserPosition,
} from '../../api/reader'
import { useAuth } from '../../auth/useAuth'
import { Button } from '../../components/ui/Button'
import { DEFAULT_READER_APPEARANCE, readReaderAppearance, storeReaderAppearance, type ReaderAppearance } from '../../lib/readerAppearance'
import { readStoredTheme, resolvedTheme } from '../../theme'
import { EpubAdapter } from './EpubAdapter'
import { PdfAdapter } from './PdfAdapter'
import { ImageAdapter } from './ImageAdapter'
import { positionLocator, type ReaderEngine, type ReaderPosition, type ReaderSearchHit, type ReaderTocEntry } from './ReaderEngine'
import { resolveReadingDirection, type ReadingDirection } from './readingDirection'

type Panel = 'contents' | 'search' | 'display' | null

const readerColors = [
  { value: 'app', label: 'Follow app', paper: '#f7f4ee', ink: '#16120e' },
  { value: 'paper', label: 'Paper', paper: '#f7f4ee', ink: '#16120e' },
  { value: 'warm', label: 'Warm', paper: '#efe2c8', ink: '#271d13' },
  { value: 'ink', label: 'Ink', paper: '#1a1511', ink: '#f1e9da' },
] as const

function errorText(caught: unknown): string {
  if (caught instanceof ApiError) {
    if (caught.status === 401 || caught.status === 403 || caught.status === 404) return 'This file is missing or you no longer have access to it.'
    return caught.message
  }
  if (caught instanceof DOMException && caught.name === 'AbortError') return 'Loading stopped.'
  return 'This file could not be opened. You can return to the book and use its other reading options.'
}

function TocList({ items, open }: { items: ReaderTocEntry[]; open: (target: string) => void }) {
  return <ol className="space-y-2 pl-4">
    {items.map((item, index) => <li key={`${item.target}-${index}`}>
      <button disabled={!item.target} className="min-h-10 text-left text-sm text-ink-soft hover:text-accent" onClick={() => open(item.target)}>{item.label}</button>
      {item.children.length ? <TocList items={item.children} open={open} /> : null}
    </li>)}
  </ol>
}

export function ReaderPage() {
  const { bookId, fileId } = useParams()
  const id = Number(bookId)
  const file = Number(fileId)
  const { user } = useAuth()
  const mount = useRef<HTMLDivElement>(null)
  const readerRoot = useRef<HTMLDivElement>(null)
  const touchStart = useRef<{ x: number; y: number } | null>(null)
  const adapter = useRef<ReaderEngine | null>(null)
  const draft = useRef<ReaderPosition | null>(null)
  const revision = useRef(0)
  const externalRevision = useRef(0)
  const externalSyncBlocked = useRef(false)
  const sha = useRef('')
  const completedRef = useRef(false)
  const saving = useRef(false)
  const pendingSave = useRef<Promise<void> | null>(null)
  const conflicted = useRef(false)
  const suspendSave = useRef(false)
  const awaitUserNavigation = useRef(false)
  const lastSaved = useRef('')
  const flush = useRef<() => void>(() => {})
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const [book, setBook] = useState<BookDetail | null>(null)
  const [phase, setPhase] = useState<'loading' | 'ready' | 'error'>('loading')
  const [error, setError] = useState<string | null>(null)
  const [location, setLocation] = useState<ReaderPosition | null>(null)
  const [format, setFormat] = useState<'epub' | 'pdf' | 'cbz'>('epub')
  const [completed, setCompleted] = useState(false)
  const [completionBusy, setCompletionBusy] = useState(false)
  const [completionError, setCompletionError] = useState<string | null>(null)
  const [newer, setNewer] = useState<(BrowserPosition & { external?: boolean }) | null>(null)
  const [syncWarning, setSyncWarning] = useState<string | null>(null)
  const [panel, setPanel] = useState<Panel>(null)
  const [query, setQuery] = useState('')
  const [hits, setHits] = useState<ReaderSearchHit[]>([])
  const [searched, setSearched] = useState(false)
  const [searchError, setSearchError] = useState<string | null>(null)
  const [toc, setToc] = useState<ReaderTocEntry[]>([])
  const [searching, setSearching] = useState(false)
  const [appearance, setAppearance] = useState(() => readReaderAppearance(user?.id))
  const appearanceRef = useRef(appearance)
  const [appearanceBusy, setAppearanceBusy] = useState(false)
  const [appearanceError, setAppearanceError] = useState<string | null>(null)
  const appearanceWorkRunning = useRef(false)
  const appearanceUpdating = useRef(false)
  const appearanceRequest = useRef(0)
  const skipReadyAppearance = useRef(false)
  const saveNow = useRef<() => Promise<void>>(() => Promise.resolve())
  const [resolvedAppTheme, setResolvedAppTheme] = useState(() => resolvedTheme(readStoredTheme()))
  const appTheme = useRef(resolvedAppTheme)
  const [epubTypography, setEpubTypography] = useState(true)
  const [pdfFit, setPdfFit] = useState<'page' | 'width'>('page')
  const [pdfZoom, setPdfZoom] = useState(1)
  const [comicSpread, setComicSpread] = useState(false)
  const [bookDirection, setBookDirection] = useState<ReadingDirection | null>(null)
  const [seriesDirection, setSeriesDirection] = useState<ReadingDirection | null>(null)
  const [embeddedDirection, setEmbeddedDirection] = useState<ReadingDirection | null>(null)
  const [directionOverride, setDirectionOverride] = useState<ReadingDirection | null>(null)
  const [readingDirection, setReadingDirectionState] = useState<ReadingDirection>('ltr')
  const directionRef = useRef<'ltr' | 'rtl'>('ltr')
  const [directionBusy, setDirectionBusy] = useState(false)
  const [directionError, setDirectionError] = useState<string | null>(null)
  const [comicThumbStart, setComicThumbStart] = useState(1)

  const beginNavigation = useCallback(() => {
    if (appearanceWorkRunning.current) return false
    if (awaitUserNavigation.current) {
      // Until the rendition reports the new location, the draft may still be
      // the page it rendered before applying KOReader's XPointer.
      draft.current = null
      if (timer.current) clearTimeout(timer.current)
    }
    awaitUserNavigation.current = false
    return true
  }, [])

  const requestAppearanceUpdate = useCallback(() => {
    appearanceRequest.current += 1
    if (appearanceWorkRunning.current || adapter.current?.type !== 'epub') return
    appearanceWorkRunning.current = true
    setAppearanceBusy(true)
    setAppearanceError(null)
    void (async () => {
      try {
        // Finish a real page turn before protecting the position from reflow.
        await adapter.current?.waitForNavigation?.()
        if (timer.current || pendingSave.current) await saveNow.current()
        appearanceUpdating.current = true
        awaitUserNavigation.current = true
        if (timer.current) clearTimeout(timer.current)
        const anchor = draft.current?.type === 'epub' ? draft.current.cfi : undefined
        while (adapter.current?.type === 'epub') {
          const request = appearanceRequest.current
          await adapter.current.setAppearance?.(appearanceRef.current, anchor)
          if (request === appearanceRequest.current) break
        }
      } catch {
        setAppearanceError('Could not apply these reading settings. Reopen the book to try again.')
      } finally {
        appearanceUpdating.current = false
        appearanceWorkRunning.current = false
        setAppearanceBusy(false)
      }
    })()
  }, [])

  useEffect(() => {
    if (!Number.isSafeInteger(id) || id <= 0 || !Number.isSafeInteger(file) || file <= 0) {
      return
    }
    let active = true
    let checkingAccess = false
    const controller = new AbortController()
    const url = positionUrl(id, file)

    function saveKey(value: ReaderPosition, isComplete: boolean) {
      return `${positionLocator(value)}|${value.percentage}|${isComplete}`
    }

    function saveDraft() {
      if (saving.current && pendingSave.current) return pendingSave.current
      const task = saveDraftOnce()
      pendingSave.current = task
      void task.finally(() => { if (pendingSave.current === task) pendingSave.current = null })
      return task
    }

    async function saveDraftOnce() {
      if (saving.current || suspendSave.current || appearanceUpdating.current || conflicted.current || awaitUserNavigation.current || !draft.current || !sha.current) return
      const current = draft.current
      const isComplete = completedRef.current
      const key = saveKey(current, isComplete)
      if (key === lastSaved.current) return
      saving.current = true
      try {
        let externalLocator: string | undefined
        if (current.type === 'epub' && adapter.current?.toExternal && !externalSyncBlocked.current) {
          try {
            externalLocator = await adapter.current.toExternal(current)
            setSyncWarning(null)
          } catch {
            setSyncWarning('This EPUB position could not be shared with KOReader. Browser progress is still saved.')
          }
        }
        const state = await saveBrowserPosition(id, file, {
          sha256: sha.current,
          locator: positionLocator(current),
          percentage: current.percentage,
          completed: isComplete,
          expectedRevision: revision.current,
          expectedExternalRevision: externalRevision.current,
          externalLocator,
        })
        if (!active) return
        revision.current = state.position?.revision ?? revision.current
        externalRevision.current = state.external?.revision ?? externalRevision.current
        lastSaved.current = key
      } catch (caught) {
        if (!active) return
        if (caught instanceof ApiError && caught.status === 409) {
          conflicted.current = true
          try {
            const state = await fetchBrowserPosition(id, file)
            if (!active) return
            if (state.sha256 !== sha.current) {
              setError('This file changed while it was open. Reload it before saving a position.')
              setPhase('error')
            } else {
              externalRevision.current = state.external?.revision ?? 0
              if (state.external?.source === 'koreader' && (!state.position || state.external.updatedAt >= state.position.updatedAt)) {
                setNewer({ locator: state.external.locator, percentage: state.external.percentage, completed: false, revision: state.position?.revision ?? 0, updatedAt: state.external.updatedAt, external: true })
              } else if (state.position) setNewer(state.position)
              else {
                revision.current = 0
                conflicted.current = false
              }
            }
          } catch {
            setError('Your position changed in another tab. Reload the reader to continue saving.')
            setPhase('error')
          }
        } else {
          setError(errorText(caught))
          setPhase('error')
        }
      } finally {
        saving.current = false
        if (active && !suspendSave.current && !conflicted.current && draft.current &&
            saveKey(draft.current, completedRef.current) !== lastSaved.current) {
          if (timer.current) clearTimeout(timer.current)
          timer.current = setTimeout(() => void saveDraft(), 1500)
        }
      }
    }
    flush.current = () => void saveDraft()
    saveNow.current = saveDraft

    function onLocation(value: ReaderPosition) {
      if (!active) return
      setLocation(value)
      if (appearanceUpdating.current) return
      draft.current = value
      if (suspendSave.current || conflicted.current || awaitUserNavigation.current) return
      if (timer.current) clearTimeout(timer.current)
      timer.current = setTimeout(() => void saveDraft(), 1500)
    }

    function onKeyDown(event: KeyboardEvent) {
      if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return
      if (event.key === 'Escape') {
        setPanel(null)
        return
      }
      const target = event.target as HTMLElement | null
      if (target?.closest('input, textarea, select, button, a, [contenteditable="true"]')) return
      if (event.key === 'ArrowLeft') {
        event.preventDefault()
        if (!beginNavigation()) return
        void (directionRef.current === 'rtl' ? adapter.current?.next() : adapter.current?.prev())
      } else if (event.key === 'ArrowRight') {
        event.preventDefault()
        if (!beginNavigation()) return
        void (directionRef.current === 'rtl' ? adapter.current?.prev() : adapter.current?.next())
      }
    }

    async function checkAccess() {
      if (!active || !adapter.current || checkingAccess) return
      checkingAccess = true
      let lostAccess: string | null = null
      try {
        const state = await fetchBrowserPosition(id, file)
        if (!active) return
        if (state.sha256 === sha.current) {
          if (state.external?.source === 'koreader' &&
              state.external.revision !== externalRevision.current &&
              (!state.position || state.external.updatedAt >= state.position.updatedAt)) {
            externalRevision.current = state.external.revision
            conflicted.current = true
            setNewer({ locator: state.external.locator, percentage: state.external.percentage, completed: false, revision: state.position?.revision ?? revision.current, updatedAt: state.external.updatedAt, external: true })
          }
          return
        }
        lostAccess = 'This file changed while it was open. Return to the book to reopen the current file.'
      } catch (caught) {
        if (!active || !(caught instanceof ApiError) || ![401, 403, 404].includes(caught.status)) return
        lostAccess = errorText(caught)
      } finally {
        if (active && lostAccess) {
          draft.current = null
          sha.current = ''
          if (timer.current) clearTimeout(timer.current)
          adapter.current?.destroy()
          adapter.current = null
          mount.current?.replaceChildren()
          setError(lostAccess)
          setPhase('error')
        }
        checkingAccess = false
      }
    }

    async function open() {
      try {
        const [detail, state] = await Promise.all([fetchBook(id), fetchBrowserPosition(id, file)])
        if (!active) return
        const selected = detail.files.find((entry) => entry.id === file && (entry.format === 'epub' || entry.format === 'pdf' || entry.format === 'cbz'))
        if (!selected) throw new ApiError('Readable file not found', 'not_found', 404)
        setFormat(selected.format as 'epub' | 'pdf' | 'cbz')
        setBook(detail)
        const sharedDirection = state.direction.bookDirection === 'rtl' || state.direction.bookDirection === 'ltr'
          ? state.direction.bookDirection : null
        const inheritedSeriesDirection = state.direction.seriesDirection === 'rtl' || state.direction.seriesDirection === 'ltr'
          ? state.direction.seriesDirection : null
        const override = state.direction.directionOverride === 'rtl' ? 'rtl' : state.direction.directionOverride === 'ltr' ? 'ltr' : null
        let effectiveDirection = resolveReadingDirection({ profileOverride: override, bookDirection: sharedDirection, seriesDirection: inheritedSeriesDirection, embeddedDirection: null })
        setBookDirection(sharedDirection)
        setSeriesDirection(inheritedSeriesDirection)
        setEmbeddedDirection(null)
        setDirectionOverride(override)
        setReadingDirectionState(effectiveDirection)
        directionRef.current = effectiveDirection
        sha.current = state.sha256
        revision.current = state.position?.revision ?? 0
        externalRevision.current = state.external?.revision ?? 0
        completedRef.current = state.bookCompleted
        setCompleted(completedRef.current)
        if (state.position) {
          lastSaved.current = `${state.position.locator}|${state.position.percentage}|${state.position.completed}`
        }
        const externalNewer = state.external?.source === 'koreader' && (!state.position || state.external.updatedAt >= state.position.updatedAt)
        // Opening or restyling an EPUB can report the start of the visible
        // page instead of the precise saved passage. Wait for actual navigation.
        awaitUserNavigation.current = selected.format === 'epub'
        const resumeLocator = selected.format !== 'epub' && externalNewer && state.external
          ? state.external.locator : state.position?.locator ?? null
        let reader: ReaderEngine
        if (selected.format === 'epub') {
          suspendSave.current = true
          const bytes = await fetchEpubBytes(id, file, controller.signal)
          if (!active || !mount.current) return
          reader = await EpubAdapter.open(mount.current, bytes, resumeLocator, onLocation, onKeyDown, (embedded) => {
            effectiveDirection = resolveReadingDirection({ profileOverride: override, bookDirection: sharedDirection, seriesDirection: inheritedSeriesDirection, embeddedDirection: embedded })
            setEmbeddedDirection(embedded)
            setReadingDirectionState(effectiveDirection)
            directionRef.current = effectiveDirection
            return effectiveDirection
          }, appearanceRef.current, beginNavigation)
          setEpubTypography(reader.supportsTypography !== false)
          if (externalNewer && state.external) {
            try {
              await reader.goToExternal?.(state.external.locator)
            } catch {
              awaitUserNavigation.current = false
              externalSyncBlocked.current = true
              setSyncWarning('KOReader’s saved position could not be opened in this EPUB. Your browser progress will stay separate for this session.')
            }
          }
          suspendSave.current = false
        } else if (selected.format === 'pdf') {
          if (!mount.current) return
          reader = await PdfAdapter.open(mount.current, contentUrl(id, file), Number(resumeLocator ?? 1), onLocation)
        } else {
          if (!mount.current) return
          reader = await ImageAdapter.open(mount.current, id, file, Number(resumeLocator ?? 1), onLocation)
        }
        if (!active) {
          reader.destroy()
          return
        }
        adapter.current = reader
        if (selected.format === 'cbz') reader.setComicLayout?.(false, effectiveDirection)
        setToc(reader.toc)
        skipReadyAppearance.current = true
        setPhase('ready')
      } catch (caught) {
        if (!active) return
        setError(errorText(caught))
        setPhase('error')
      }
    }

    function flushOnHide() {
      if (document.visibilityState === 'hidden') void saveDraft()
    }
    function flushOnExit() {
      if (!draft.current || conflicted.current || appearanceUpdating.current || awaitUserNavigation.current || saving.current || !sha.current) return
      const key = saveKey(draft.current, completedRef.current)
      if (key === lastSaved.current) return
      void fetch(url, {
        method: 'PUT',
        credentials: 'include',
        keepalive: true,
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          sha256: sha.current,
          locator: positionLocator(draft.current),
          percentage: draft.current.percentage,
          completed: completedRef.current,
          expectedRevision: revision.current,
          expectedExternalRevision: externalRevision.current,
        }),
      })
    }

    void open()
    const accessInterval = window.setInterval(() => void checkAccess(), 30_000)
    window.addEventListener('focus', checkAccess)
    window.addEventListener('keydown', onKeyDown)
    document.addEventListener('visibilitychange', flushOnHide)
    window.addEventListener('pagehide', flushOnExit)
    return () => {
      flushOnExit()
      active = false
      controller.abort()
      if (timer.current) clearTimeout(timer.current)
      window.clearInterval(accessInterval)
      window.removeEventListener('focus', checkAccess)
      window.removeEventListener('keydown', onKeyDown)
      document.removeEventListener('visibilitychange', flushOnHide)
      window.removeEventListener('pagehide', flushOnExit)
      adapter.current?.destroy()
      adapter.current = null
      flush.current = () => {}
      saveNow.current = () => Promise.resolve()
    }
  }, [id, file, beginNavigation])

  useEffect(() => {
    if (phase !== 'ready' || format !== 'epub') return
    if (skipReadyAppearance.current) {
      skipReadyAppearance.current = false
      return
    }
    requestAppearanceUpdate()
  }, [appearance, format, phase, requestAppearanceUpdate])

  useEffect(() => {
    const onAppThemeChange = () => {
      const next = resolvedTheme(readStoredTheme())
      if (next === appTheme.current) return
      appTheme.current = next
      setResolvedAppTheme(next)
      if (appearanceRef.current.theme === 'app') requestAppearanceUpdate()
    }
    const observer = new MutationObserver(onAppThemeChange)
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] })
    const media = window.matchMedia('(prefers-color-scheme: dark)')
    media.addEventListener('change', onAppThemeChange)
    return () => {
      observer.disconnect()
      media.removeEventListener('change', onAppThemeChange)
    }
  }, [requestAppearanceUpdate])

  async function search() {
    if (!adapter.current || !query.trim()) return
    setSearchError(null)
    setHits([])
    setSearched(false)
    setSearching(true)
    try {
      setHits(await adapter.current.search(query))
      setSearched(true)
      setPanel('search')
    } catch {
      setSearchError('Search could not finish for this file.')
    } finally {
      setSearching(false)
    }
  }

  function changeAppearance(next: ReaderAppearance) {
    appearanceRef.current = next
    setAppearance(next)
    if (user) storeReaderAppearance(user.id, next)
  }

  async function changeDirection(value: 'ltr' | 'rtl' | null) {
    setDirectionBusy(true)
    setDirectionError(null)
    try {
      const state = await setReadingDirection(id, file, value)
      const shared = state.bookDirection === 'rtl' || state.bookDirection === 'ltr' ? state.bookDirection : null
      const series = state.seriesDirection === 'rtl' || state.seriesDirection === 'ltr' ? state.seriesDirection : null
      const override = state.directionOverride === 'rtl' ? 'rtl' : state.directionOverride === 'ltr' ? 'ltr' : null
      const effective = resolveReadingDirection({ profileOverride: override, bookDirection: shared, seriesDirection: series, embeddedDirection })
      setBookDirection(shared)
      setSeriesDirection(series)
      setDirectionOverride(override)
      setReadingDirectionState(effective)
      directionRef.current = effective
      adapter.current?.setReadingDirection?.(effective)
      adapter.current?.setComicLayout?.(comicSpread, effective)
    } catch (caught) {
      setDirectionError(caught instanceof ApiError ? caught.message : 'Could not save reading direction.')
    } finally {
      setDirectionBusy(false)
    }
  }

  async function markFinished() {
    setCompletionBusy(true)
    setCompletionError(null)
    suspendSave.current = true
    if (timer.current) clearTimeout(timer.current)
    try {
      await pendingSave.current
      await setBookCompletion(id, true)
      const state = await fetchBrowserPosition(id, file)
      revision.current = state.position?.revision ?? 0
      externalRevision.current = state.external?.revision ?? 0
      lastSaved.current = state.position
        ? `${state.position.locator}|${state.position.percentage}|${state.position.completed}` : ''
      conflicted.current = false
      setNewer(null)
      awaitUserNavigation.current = false
      completedRef.current = true
      setCompleted(true)
    } catch (caught) {
      setCompletionError(caught instanceof ApiError ? caught.message : 'Could not mark this book finished.')
    } finally {
      suspendSave.current = false
      flush.current()
      setCompletionBusy(false)
    }
  }

  async function restart() {
    setCompletionBusy(true)
    setCompletionError(null)
    suspendSave.current = true
    if (timer.current) clearTimeout(timer.current)
    try {
      await pendingSave.current
      await setBookCompletion(id, false)
      const state = await fetchBrowserPosition(id, file)
      revision.current = state.position?.revision ?? 0
      externalRevision.current = state.external?.revision ?? 0
      lastSaved.current = state.position
        ? `${state.position.locator}|${state.position.percentage}|${state.position.completed}` : ''
      conflicted.current = false
      setNewer(null)
      completedRef.current = false
      setCompleted(false)
      if (!beginNavigation()) return
      await adapter.current?.goToStart()
    } catch (caught) {
      setCompletionError(caught instanceof ApiError ? caught.message : 'Could not restart this book.')
    } finally {
      suspendSave.current = false
      flush.current()
      setCompletionBusy(false)
    }
  }

  async function acceptSavedPosition() {
    if (!newer || !adapter.current) return
    suspendSave.current = true
    if (timer.current) clearTimeout(timer.current)
    awaitUserNavigation.current = format === 'epub'
    revision.current = newer.revision
    completedRef.current = newer.completed
    setCompleted(newer.completed)
    try {
      if (newer.external && adapter.current.goToExternal) await adapter.current.goToExternal(newer.locator)
      else await adapter.current.goTo(newer.locator)
      // Accepting a saved position is a read, not a move made by this reader.
      // Wait for actual navigation before writing it back.
      if (draft.current) lastSaved.current = `${positionLocator(draft.current)}|${draft.current.percentage}|${newer.completed}`
      externalSyncBlocked.current = false
      conflicted.current = false
      setNewer(null)
    } catch {
      setError('The saved position could not be opened. Reload this file to continue.')
      setPhase('error')
    } finally {
      suspendSave.current = false
    }
  }

  function keepThisPosition() {
    if (!newer) return
    awaitUserNavigation.current = false
    revision.current = newer.revision
    // The visible position may match the last browser save, but it still
    // needs to replace the newer KOReader locator.
    lastSaved.current = ''
    externalSyncBlocked.current = false
    conflicted.current = false
    setNewer(null)
    flush.current()
  }

  function endTouch(event: React.TouchEvent) {
    const start = touchStart.current
    touchStart.current = null
    if (!start || event.changedTouches.length !== 1 || panel) return
    const dx = event.changedTouches[0].clientX - start.x
    const dy = event.changedTouches[0].clientY - start.y
    if (Math.abs(dx) < 60 || Math.abs(dx) < Math.abs(dy) * 1.4) return
    const forward = directionRef.current === 'rtl' ? dx > 0 : dx < 0
    if (!beginNavigation()) return
    void (forward ? adapter.current?.next() : adapter.current?.prev())
  }

  if (!Number.isSafeInteger(id) || id <= 0 || !Number.isSafeInteger(file) || file <= 0) {
    return <p className="p-6 text-danger">Invalid reading link.</p>
  }

  return (
    <div ref={readerRoot} data-reader-theme={appearance.theme === 'app' ? undefined : appearance.theme} style={appearance.theme === 'app' ? { colorScheme: resolvedAppTheme === 'ink' ? 'dark' : 'light' } : undefined} className="bokhylle-reader flex h-dvh min-h-0 flex-col overflow-hidden bg-canvas text-ink">
      <header className="flex shrink-0 flex-wrap items-center justify-between gap-3 border-b border-line px-4 py-3 sm:px-6">
        <div className="flex min-w-0 items-center gap-3">
          <Link to={`/library/${id}`} className="inline-flex min-h-11 items-center gap-2 text-sm text-ink-soft hover:text-ink">
            <ArrowLeft size={18} /> <span className="sr-only sm:not-sr-only">Book details</span>
          </Link>
          <span className="h-6 border-l border-line" aria-hidden />
          <div className="min-w-0">
            <p className="truncate font-display text-lg leading-tight">{book?.title ?? 'Reading'}</p>
            <p className="text-xs text-ink-muted">{book?.authors.join(', ') || 'Bokhylle'}</p>
          </div>
        </div>
        <div className="flex items-center gap-1" role="group" aria-label="Reader tools">
          <Button size="sm" variant={panel === 'contents' ? 'secondary' : 'ghost'} onClick={() => {
            if (panel === 'contents') setPanel(null)
            else {
              if (format === 'cbz' && location?.type === 'cbz') setComicThumbStart(Math.floor((location.page - 1) / 24) * 24 + 1)
              setPanel('contents')
            }
          }} aria-label={format === 'cbz' ? 'Page thumbnails' : 'Contents'}><BookOpen size={18} /></Button>
          {format !== 'cbz' && <Button size="sm" variant={panel === 'search' ? 'secondary' : 'ghost'} onClick={() => setPanel(panel === 'search' ? null : 'search')} aria-label="Search book"><Search size={18} /></Button>}
          <Button size="sm" variant={panel === 'display' ? 'secondary' : 'ghost'} onClick={() => setPanel(panel === 'display' ? null : 'display')} aria-label="Display settings"><Settings2 size={18} /></Button>
          <Button size="sm" variant="ghost" onClick={() => { if (document.fullscreenElement) void document.exitFullscreen(); else void readerRoot.current?.requestFullscreen() }} aria-label="Toggle fullscreen"><Maximize2 size={18} /></Button>
        </div>
      </header>

      {newer && (
        <div className="flex flex-wrap items-center justify-between gap-3 border-b border-line bg-surface-2 px-4 py-3 text-sm" role="alert">
          <span>Your position changed in another tab. Choose which position to use.</span>
          <div className="flex gap-2">
            <Button size="sm" onClick={() => void acceptSavedPosition()}>Use saved position</Button>
            <Button size="sm" variant="ghost" onClick={keepThisPosition}>Keep this tab</Button>
          </div>
        </div>
      )}
      {syncWarning && <p className="border-b border-line bg-surface-2 px-4 py-2 text-sm text-ink-soft" role="status">{syncWarning}</p>}

      <div className="relative flex min-h-0 flex-1 overflow-hidden">
        {panel && phase === 'ready' && (
          <aside className="absolute inset-y-0 left-0 z-20 flex min-h-0 w-[min(22rem,90vw)] flex-col overflow-hidden border-r border-line bg-surface p-5 shadow-lg md:static md:shrink-0 md:shadow-none" aria-label={panel}>
            {panel === 'contents' && <>
              <h2 className="mb-4 shrink-0 font-display text-2xl">{format === 'cbz' ? 'Pages' : 'Contents'}</h2>
              {format === 'cbz' && location?.type === 'cbz' ? <>
                <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain" aria-label="Comic page thumbnails">
                  <div className="grid grid-cols-3 gap-3" dir={readingDirection}>
                    {Array.from({ length: Math.min(24, location.totalPages - comicThumbStart + 1) }, (_, index) => comicThumbStart + index).map((page) =>
                      <button key={page} type="button" className={`rounded border p-1 text-xs ${location.page === page ? 'border-accent text-accent' : 'border-line text-ink-soft'}`} onClick={() => { if (!beginNavigation()) return; void adapter.current?.goTo(String(page)); setPanel(null) }} aria-label={`Go to page ${page}`}>
                        <img src={comicPageUrl(id, file, page)} alt="" loading="lazy" decoding="async" className="mx-auto h-28 w-full object-contain" />
                        <span>{page}</span>
                      </button>)}
                  </div>
                </div>
                <div className={`mt-3 flex shrink-0 items-center justify-between gap-2 text-xs text-ink-muted ${readingDirection === 'rtl' ? 'flex-row-reverse' : ''}`}>
                  <Button size="sm" variant="ghost" disabled={comicThumbStart === 1} onClick={() => setComicThumbStart(Math.max(1, comicThumbStart - 24))}>Earlier</Button>
                  <span>{comicThumbStart}–{Math.min(comicThumbStart + 23, location.totalPages)} of {location.totalPages}</span>
                  <Button size="sm" variant="ghost" disabled={comicThumbStart + 24 > location.totalPages} onClick={() => setComicThumbStart(comicThumbStart + 24)}>Later</Button>
                </div>
              </> : <nav className="min-h-0 flex-1 overflow-y-auto overscroll-contain" aria-label="Book contents">
                {toc.length ? (
                  <TocList items={toc} open={(href) => { if (!beginNavigation()) return; void adapter.current?.goTo(href); setPanel(null) }} />
                ) : <p className="text-sm text-ink-muted">This file has no contents list.</p>}
              </nav>}
            </>}
            {panel === 'search' && <>
              <h2 className="mb-4 shrink-0 font-display text-2xl">Search this book</h2>
              <form onSubmit={(event) => { event.preventDefault(); void search() }} className="flex shrink-0 gap-2">
                <input value={query} onChange={(event) => setQuery(event.target.value)} aria-label="Search text" className="min-w-0 flex-1 rounded border border-line bg-canvas px-3 py-2 text-sm" />
                <Button size="sm" type="submit" disabled={searching || !query.trim()}>Find</Button>
              </form>
              {searchError && <p className="mt-3 text-xs text-danger" role="alert">{searchError}</p>}
              <p className="mt-3 shrink-0 text-xs text-ink-muted" role="status">{searchError ? '' : searching ? 'Searching…' : hits.length ? `${hits.length} result${hits.length === 1 ? '' : 's'}${hits.length === 50 ? ' (first 50)' : ''}` : searched ? 'No results found.' : 'Enter words to search.'}</p>
              <ol className="mt-3 min-h-0 flex-1 space-y-3 overflow-y-auto overscroll-contain">{hits.map((hit, index) => <li key={`${hit.target}-${index}`}>
                <button className="w-full border-l-2 border-line py-1 pl-3 text-left text-sm hover:border-accent" onClick={() => { if (!beginNavigation()) return; void adapter.current?.goTo(hit.target); setPanel(null) }}>
                  <span className="block text-xs font-semibold text-ink-muted">{hit.section}{hit.percentage === null ? '' : ` · about ${Math.round(hit.percentage * 100)}%`}</span>{hit.excerpt}
                </button>
              </li>)}</ol>
            </>}
            {panel === 'display' && <>
              <h2 className="mb-4 font-display text-2xl">Display</h2>
              <p className="mb-5 text-xs text-ink-muted">Reading appearance is saved for this profile in this browser.</p>
              <fieldset>
                <legend className="text-sm text-ink-soft">{format === 'epub' && epubTypography ? 'Reading colors' : 'Reader surface'}</legend>
                <div className="mt-3 grid grid-cols-2 gap-2">
                  {readerColors.map((choice) => <button key={choice.value} type="button" aria-pressed={appearance.theme === choice.value} onClick={() => changeAppearance({ ...appearance, theme: choice.value })} className={`flex min-h-11 items-center gap-2 rounded-[3px] border px-3 text-left text-sm ${appearance.theme === choice.value ? 'border-accent bg-surface-2 text-ink' : 'border-line text-ink-soft hover:border-accent'}`}>
                    <span className="h-5 w-5 shrink-0 rounded-full border border-current" style={{ backgroundColor: choice.value === 'app' ? 'var(--c-canvas)' : choice.paper, color: choice.value === 'app' ? 'var(--c-ink)' : choice.ink }} aria-hidden />
                    {choice.label}
                  </button>)}
                </div>
              </fieldset>
              {format === 'epub' && epubTypography && <div className="mt-6 space-y-5 border-t border-line pt-5">
                <div>
                  <label htmlFor="reader-font-family" className="block text-sm text-ink-soft">Font</label>
                  <select id="reader-font-family" value={appearance.fontFamily} onChange={(event) => changeAppearance({ ...appearance, fontFamily: event.target.value as ReaderAppearance['fontFamily'] })} className="mt-2 w-full rounded border border-line bg-canvas px-3 py-2 text-sm">
                    <option value="serif">Georgia</option><option value="sans">Sans serif</option>
                  </select>
                </div>
                <div>
                  <label htmlFor="reader-font" className="block text-sm text-ink-soft">Text size: {appearance.textScale}%</label>
                  <input id="reader-font" type="range" min="80" max="200" step="10" value={appearance.textScale} onChange={(event) => changeAppearance({ ...appearance, textScale: Number(event.target.value) })} className="mt-3 w-full accent-accent" />
                </div>
                <div>
                  <label htmlFor="reader-line-spacing" className="block text-sm text-ink-soft">Line spacing</label>
                  <select id="reader-line-spacing" value={appearance.lineSpacing} onChange={(event) => changeAppearance({ ...appearance, lineSpacing: event.target.value as ReaderAppearance['lineSpacing'] })} className="mt-2 w-full rounded border border-line bg-canvas px-3 py-2 text-sm">
                    <option value="compact">Compact</option><option value="standard">Standard</option><option value="spacious">Spacious</option>
                  </select>
                </div>
              </div>}
              {format === 'epub' && !epubTypography && <p className="mt-5 text-sm text-ink-muted">This book has a fixed page layout, so its typography stays as published.</p>}
              {format !== 'epub' && <div className="mt-6 border-t border-line pt-5">
                <label htmlFor="reader-fit" className="block text-sm text-ink-soft">Page fit</label>
                <select id="reader-fit" value={pdfFit} onChange={(event) => { const fit = event.target.value as 'page' | 'width'; setPdfFit(fit); adapter.current?.setFit?.(fit, pdfZoom) }} className="mt-2 w-full rounded border border-line bg-canvas px-3 py-2 text-sm">
                  <option value="page">Fit page</option><option value="width">Fit width</option>
                </select>
                {format === 'pdf' ? <>
                  <label htmlFor="reader-zoom" className="mt-5 block text-sm text-ink-soft">Zoom: {Math.round(pdfZoom * 100)}%</label>
                  <input id="reader-zoom" type="range" min="50" max="300" step="25" value={pdfZoom * 100} onChange={(event) => { const zoom = Number(event.target.value) / 100; setPdfZoom(zoom); adapter.current?.setFit?.(pdfFit, zoom) }} className="mt-3 w-full accent-accent" />
                </> : <>
                  <label className="mt-5 flex items-center gap-2 text-sm text-ink-soft"><input type="checkbox" checked={comicSpread} onChange={(event) => { setComicSpread(event.target.checked); adapter.current?.setComicLayout?.(event.target.checked, readingDirection) }} />Double spread</label>
                </>}
              </div>}
              {appearanceBusy && <p className="mt-4 text-xs text-ink-muted" role="status">Adjusting page…</p>}
              {appearanceError && <p className="mt-4 text-xs text-danger" role="alert">{appearanceError}</p>}
              <Button variant="ghost" className="mt-5 w-full justify-start" onClick={() => changeAppearance(format === 'epub' && epubTypography ? { ...DEFAULT_READER_APPEARANCE } : { ...appearance, theme: 'app' })}>{format === 'epub' && epubTypography ? 'Reset reading appearance' : 'Reset reader surface'}</Button>
              <label htmlFor="reader-direction" className="mt-5 block text-sm text-ink-soft">Reading direction</label>
              <select id="reader-direction" value={directionOverride ?? 'default'} disabled={directionBusy || appearanceBusy} onChange={(event) => void changeDirection(event.target.value === 'default' ? null : event.target.value as 'ltr' | 'rtl')} className="mt-2 w-full rounded border border-line bg-canvas px-3 py-2 text-sm">
                <option value="default">Inherit ({resolveReadingDirection({ profileOverride: null, bookDirection, seriesDirection, embeddedDirection }) === 'rtl' ? 'right to left' : 'left to right'})</option>
                <option value="ltr">Left to right</option><option value="rtl">Right to left</option>
              </select>
              {directionError && <p className="mt-2 text-xs text-danger" role="alert">{directionError}</p>}
              <div className="mt-6 space-y-2 border-t border-line pt-5">
                <Button variant="ghost" className="w-full justify-start" disabled={completionBusy || appearanceBusy} onClick={() => void restart()}>Start from beginning</Button>
                <Button variant="ghost" className="w-full justify-start" disabled={completionBusy || appearanceBusy || completed} onClick={() => void markFinished()}>{completed ? <Check size={16} /> : null}{completed ? 'Finished' : 'Mark finished'}</Button>
                {completionError && <p className="text-xs text-danger" role="alert">{completionError}</p>}
              </div>
            </>}
          </aside>
        )}

        <main className="relative min-h-0 min-w-0 flex-1" aria-label={`${format.toUpperCase()} reading area`} aria-describedby="reader-instructions" onTouchStart={(event) => { if (event.touches.length === 1) touchStart.current = { x: event.touches[0].clientX, y: event.touches[0].clientY } }} onTouchEnd={endTouch}>
          <p id="reader-instructions" className="sr-only">Use the Previous and Next buttons, the left and right arrow keys, or swipe sideways to turn pages.</p>
          {phase === 'loading' && <div role="status" className="absolute inset-0 z-10 flex items-center justify-center bg-canvas text-sm text-ink-muted">Opening book…</div>}
          {phase === 'error' && <div role="alert" className="absolute inset-0 z-10 flex flex-col items-center justify-center gap-4 bg-canvas px-6 text-center">
            <p className="max-w-md text-sm text-danger">{error}</p>
            <Link to={`/library/${id}`} className="text-sm text-accent underline">Return to book</Link>
          </div>}
          <div ref={mount} className={`mx-auto h-full ${format === 'epub' ? 'max-w-5xl px-2 sm:px-8' : 'w-full p-2'}`} />
        </main>
      </div>

      <footer className="flex shrink-0 items-center justify-between gap-4 border-t border-line px-4 py-2 sm:px-6">
        {readingDirection === 'rtl'
          ? <Button variant="ghost" size="sm" disabled={phase !== 'ready' || appearanceBusy} onClick={() => { if (!beginNavigation()) return; void adapter.current?.next() }} aria-label="Next page"><ChevronLeft size={18} />Next</Button>
          : <Button variant="ghost" size="sm" disabled={phase !== 'ready' || appearanceBusy} onClick={() => { if (!beginNavigation()) return; void adapter.current?.prev() }} aria-label="Previous page"><ChevronLeft size={18} />Previous</Button>}
        {location && location.type !== 'epub' ? <form key={location.page} className="flex items-center gap-1 text-xs tabular-nums text-ink-muted" onSubmit={(event) => { event.preventDefault(); const input = event.currentTarget.elements.namedItem('page') as HTMLInputElement; if (!beginNavigation()) return; void adapter.current?.goTo(input.value) }}>
          <label htmlFor="reader-page" className="sr-only">Page number</label>
          <input id="reader-page" name="page" type="number" min="1" max={location.totalPages} defaultValue={location.page} className="w-14 rounded border border-line bg-canvas px-1 py-1 text-center text-ink" />
          <span>of {location.totalPages} · {Math.round(location.percentage * 100)}%{completed ? ' · Finished' : ''}</span>
          <button type="submit" className="sr-only focus:not-sr-only">Go</button>
        </form> : <span className="text-xs tabular-nums text-ink-muted" aria-live="polite">{location ? `${Math.round(location.percentage * 100)}%` : '—'}{completed ? ' · Finished' : ''}</span>}
        {readingDirection === 'rtl'
          ? <Button variant="ghost" size="sm" disabled={phase !== 'ready' || appearanceBusy} onClick={() => { if (!beginNavigation()) return; void adapter.current?.prev() }} aria-label="Previous page">Previous<ChevronRight size={18} /></Button>
          : <Button variant="ghost" size="sm" disabled={phase !== 'ready' || appearanceBusy} onClick={() => { if (!beginNavigation()) return; void adapter.current?.next() }} aria-label="Next page">Next<ChevronRight size={18} /></Button>}
      </footer>
    </div>
  )
}

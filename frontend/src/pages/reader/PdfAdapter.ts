import { getDocument, GlobalWorkerOptions, TextLayer, type PDFDocumentLoadingTask, type PDFDocumentProxy, type PDFPageProxy, type RenderTask } from 'pdfjs-dist/legacy/build/pdf.mjs'
import workerUrl from 'pdfjs-dist/legacy/build/pdf.worker.min.mjs?url'
import './pdfText.css'
import type { ReaderEngine, ReaderPosition, ReaderSearchHit, ReaderTocEntry } from './ReaderEngine'

GlobalWorkerOptions.workerSrc = workerUrl

const MAX_CACHED_PAGES = 3
// Canvas pixels alone can use about 48 MB at this limit; decoded PDF images add more.
const MAX_CACHED_PIXELS = 12_000_000

type PdfLayout = {
  key: string
  width: number
  height: number
  fit: 'width' | 'page'
  zoom: number
  pixelRatio: number
}

type CachedPage = {
  number: number
  canvas: HTMLCanvasElement
  textHost: HTMLDivElement
  page: PDFPageProxy | null
  renderTask: RenderTask | null
  textLayer: TextLayer | null
  width: number
  height: number
  cancelled: boolean
  finished: boolean
  ready: Promise<void>
}

export class PdfAdapter implements ReaderEngine {
  readonly type = 'pdf' as const
  readonly toc: ReaderTocEntry[] = []
  private page = 1
  private fit: 'width' | 'page' = 'page'
  private zoom = 1
  private generation = 0
  private destroyed = false
  private layoutKey = ''
  private prefetchTimer: ReturnType<typeof setTimeout> | null = null
  private readonly cache = new Map<number, CachedPage>()
  private readonly pageBox: HTMLDivElement
  private readonly observer: ResizeObserver
  private readonly host: HTMLElement
  private readonly pdf: PDFDocumentProxy
  private readonly loading: PDFDocumentLoadingTask
  private readonly onLocation: (position: ReaderPosition) => void

  private constructor(
    host: HTMLElement,
    pdf: PDFDocumentProxy,
    loading: PDFDocumentLoadingTask,
    onLocation: (position: ReaderPosition) => void,
  ) {
    this.host = host
    this.pdf = pdf
    this.loading = loading
    this.onLocation = onLocation
    this.pageBox = document.createElement('div')
    this.pageBox.className = 'relative mx-auto'
    this.host.classList.add('overflow-auto')
    this.host.append(this.pageBox)
    this.observer = new ResizeObserver(() => { void this.render() })
    this.observer.observe(host)
  }

  static async open(
    host: HTMLElement,
    url: string,
    initialPage: number,
    onLocation: (position: ReaderPosition) => void,
  ): Promise<PdfAdapter> {
    const loading = getDocument({ url, withCredentials: true, wasmUrl: `${import.meta.env.BASE_URL}pdfjs/wasm/` })
    const pdf = await loading.promise
    const adapter = new PdfAdapter(host, pdf, loading, onLocation)
    try {
      adapter.page = Math.min(pdf.numPages, Math.max(1, initialPage))
      await adapter.loadOutline()
      await adapter.render()
      return adapter
    } catch (error) {
      adapter.destroy()
      throw error
    }
  }

  private async loadOutline() {
    const outline = await this.pdf.getOutline()
    if (!outline) return
    const convert = async (items: typeof outline): Promise<ReaderTocEntry[]> => {
      const result: ReaderTocEntry[] = []
      for (const item of items) {
        let destination = typeof item.dest === 'string'
          ? await this.pdf.getDestination(item.dest)
          : item.dest
        if (!destination?.[0]) destination = null
        let target = ''
        if (destination) {
          const index = await this.pdf.getPageIndex(destination[0])
          target = String(index + 1)
        }
        result.push({ label: item.title, target, children: await convert(item.items) })
      }
      return result
    }
    this.toc.push(...await convert(outline))
  }

  private publishLocation() {
    const totalPages = this.pdf.numPages
    this.onLocation({
      type: 'pdf', page: this.page, totalPages,
      percentage: this.page / totalPages,
      atEnd: this.page === totalPages,
    })
  }

  private currentLayout(): PdfLayout {
    const width = Math.max(200, this.host.clientWidth - 32)
    const height = Math.max(200, this.host.clientHeight - 24)
    const pixelRatio = Math.min(2, window.devicePixelRatio || 1)
    return {
      key: `${width}:${height}:${this.fit}:${this.zoom}:${pixelRatio}`,
      width, height, fit: this.fit, zoom: this.zoom, pixelRatio,
    }
  }

  private discard(entry: CachedPage) {
    entry.cancelled = true
    entry.renderTask?.cancel()
    entry.textLayer?.cancel()
    if (entry.finished) entry.page?.cleanup()
  }

  private clearCache() {
    if (this.prefetchTimer) clearTimeout(this.prefetchTimer)
    this.prefetchTimer = null
    for (const entry of this.cache.values()) this.discard(entry)
    this.cache.clear()
  }

  private trimCache() {
    const pixels = () => [...this.cache.values()].reduce((total, entry) => total + entry.canvas.width * entry.canvas.height, 0)
    while (this.cache.size > MAX_CACHED_PAGES || pixels() > MAX_CACHED_PIXELS) {
      const victim = [...this.cache.values()]
        .filter((entry) => entry.number !== this.page)
        .sort((a, b) => Math.abs(b.number - this.page) - Math.abs(a.number - this.page))[0]
      if (!victim) break
      this.cache.delete(victim.number)
      this.discard(victim)
    }
  }

  private async loadPage(entry: CachedPage, layout: PdfLayout) {
    try {
      const page = await this.pdf.getPage(entry.number)
      entry.page = page
      if (entry.cancelled || this.destroyed) return
      const unit = page.getViewport({ scale: 1 })
      const base = layout.fit === 'width' ? layout.width / unit.width : Math.min(layout.width / unit.width, layout.height / unit.height)
      const viewport = page.getViewport({ scale: base * layout.zoom })
      // Bound canvas memory even when a huge page is opened at high zoom.
      const memoryScale = Math.min(1, Math.sqrt(32_000_000 / (viewport.width * viewport.height * layout.pixelRatio * layout.pixelRatio)))
      const outputScale = layout.pixelRatio * memoryScale
      entry.width = viewport.width
      entry.height = viewport.height
      entry.canvas.width = Math.max(1, Math.floor(viewport.width * outputScale))
      entry.canvas.height = Math.max(1, Math.floor(viewport.height * outputScale))
      entry.canvas.style.width = `${viewport.width}px`
      entry.canvas.style.height = `${viewport.height}px`
      entry.textHost.style.setProperty('--total-scale-factor', String(viewport.scale))
      const context = entry.canvas.getContext('2d')
      if (!context) throw new Error('Canvas is unavailable')
      const task = page.render({
        canvas: entry.canvas,
        canvasContext: context,
        viewport,
        transform: [outputScale, 0, 0, outputScale, 0, 0],
      })
      entry.renderTask = task
      await task.promise
      entry.renderTask = null
      if (entry.cancelled || this.destroyed) return
      const content = await page.getTextContent()
      if (entry.cancelled || this.destroyed) return
      const textLayer = new TextLayer({ textContentSource: content, container: entry.textHost, viewport })
      entry.textLayer = textLayer
      await textLayer.render()
    } catch (error) {
      if (!entry.cancelled && !this.destroyed) throw error
    } finally {
      entry.renderTask = null
      entry.finished = true
      if (entry.cancelled || this.destroyed) entry.page?.cleanup()
    }
  }

  private cachedPage(number: number, layout: PdfLayout): CachedPage {
    const existing = this.cache.get(number)
    if (existing) return existing
    const canvas = document.createElement('canvas')
    canvas.className = 'block max-w-none shadow-lg'
    canvas.setAttribute('aria-label', 'PDF page')
    const textHost = document.createElement('div')
    textHost.className = 'bokhylle-pdf-text'
    const entry: CachedPage = {
      number, canvas, textHost, page: null, renderTask: null, textLayer: null,
      width: 0, height: 0, cancelled: false, finished: false, ready: Promise.resolve(),
    }
    entry.ready = this.loadPage(entry, layout).catch((error) => {
      if (this.cache.get(number) === entry) this.cache.delete(number)
      this.discard(entry)
      throw error
    })
    // Prefetches have no direct caller. Navigation still observes the rejection.
    void entry.ready.catch(() => {})
    this.cache.set(number, entry)
    this.trimCache()
    return entry
  }

  private schedulePrefetch(layout: PdfLayout) {
    if (this.prefetchTimer) clearTimeout(this.prefetchTimer)
    const currentEntry = this.cache.get(this.page)
    if (currentEntry && currentEntry.canvas.width * currentEntry.canvas.height > MAX_CACHED_PIXELS / 2) return
    const current = this.page
    this.prefetchTimer = setTimeout(() => {
      if (this.destroyed || this.page !== current || this.layoutKey !== layout.key) return
      const neighbors = [current + 1, current - 1].filter((number) => number >= 1 && number <= this.pdf.numPages)
      void (async () => {
        for (const number of neighbors) {
          if (this.destroyed || this.page !== current || this.layoutKey !== layout.key) return
          try {
            await this.cachedPage(number, layout).ready
            this.trimCache()
          } catch { /* A failed prefetch is retried when the page is opened. */ }
        }
      })()
    }, 50)
  }

  private async render() {
    if (this.destroyed) return
    const generation = ++this.generation
    const layout = this.currentLayout()
    if (layout.key !== this.layoutKey) {
      this.clearCache()
      this.layoutKey = layout.key
    }
    const entry = this.cachedPage(this.page, layout)
    await entry.ready
    if (generation !== this.generation || this.destroyed || entry.cancelled) return
    this.pageBox.style.width = `${entry.width}px`
    this.pageBox.style.height = `${entry.height}px`
    this.pageBox.replaceChildren(entry.canvas, entry.textHost)
    this.trimCache()
    this.publishLocation()
    this.schedulePrefetch(layout)
  }

  async goTo(target: string) {
    const page = Number(target)
    if (!Number.isSafeInteger(page) || page < 1 || page > this.pdf.numPages) throw new Error('Invalid PDF page')
    this.page = page
    this.host.scrollTo(0, 0)
    await this.render()
  }
  next() { return this.goTo(String(Math.min(this.pdf.numPages, this.page + 1))) }
  prev() { return this.goTo(String(Math.max(1, this.page - 1))) }
  goToStart() { return this.goTo('1') }
  setFit(fit: 'width' | 'page', zoom: number) {
    this.fit = fit
    this.zoom = Math.min(3, Math.max(0.5, zoom))
    void this.render()
  }

  async search(query: string): Promise<ReaderSearchHit[]> {
    const needle = query.trim().toLocaleLowerCase()
    if (!needle) return []
    const hits: ReaderSearchHit[] = []
    for (let index = 1; index <= this.pdf.numPages && !this.destroyed; index++) {
      const page = await this.pdf.getPage(index)
      const content = await page.getTextContent()
      const text = content.items.map((item) => 'str' in item ? item.str : '').join(' ')
      const found = text.toLocaleLowerCase().indexOf(needle)
      if (found !== -1) {
        hits.push({
          target: String(index),
          section: `Page ${index}`,
          excerpt: text.slice(Math.max(0, found - 55), Math.min(text.length, found + query.length + 85)),
          percentage: index / this.pdf.numPages,
        })
      }
      if (!this.cache.has(index)) page.cleanup()
      if (hits.length >= 50) break
    }
    return hits
  }

  destroy() {
    this.destroyed = true
    this.generation++
    this.observer.disconnect()
    this.clearCache()
    void this.loading.destroy()
    this.pageBox.remove()
  }
}

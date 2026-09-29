import { fetchComicManifest, comicPageUrl } from '../../api/reader'
import type { ReaderEngine, ReaderPosition, ReaderSearchHit, ReaderTocEntry } from './ReaderEngine'

export class ImageAdapter implements ReaderEngine {
  readonly type = 'cbz' as const
  readonly toc: ReaderTocEntry[] = []
  private page = 1
  private fit: 'width' | 'page' = 'page'
  private spread = false
  private direction: 'ltr' | 'rtl' = 'ltr'
  private destroyed = false
  private readonly row: HTMLDivElement
  private readonly host: HTMLElement
  private readonly bookId: number
  private readonly fileId: number
  private readonly totalPages: number
  private readonly onLocation: (position: ReaderPosition) => void

  private constructor(
    host: HTMLElement,
    bookId: number,
    fileId: number,
    totalPages: number,
    onLocation: (position: ReaderPosition) => void,
  ) {
    this.host = host
    this.bookId = bookId
    this.fileId = fileId
    this.totalPages = totalPages
    this.onLocation = onLocation
    this.row = document.createElement('div')
    this.row.className = 'flex min-h-full items-center justify-center gap-2'
    this.host.classList.add('overflow-auto')
    this.host.append(this.row)
  }

  static async open(
    host: HTMLElement,
    bookId: number,
    fileId: number,
    initialPage: number,
    onLocation: (position: ReaderPosition) => void,
  ): Promise<ImageAdapter> {
    const manifest = await fetchComicManifest(bookId, fileId)
    if (manifest.pages < 1) throw new Error('CBZ has no pages')
    const engine = new ImageAdapter(host, bookId, fileId, manifest.pages, onLocation)
    engine.page = Math.min(manifest.pages, Math.max(1, initialPage))
    engine.render()
    return engine
  }

  private render() {
    if (this.destroyed) return
    this.row.replaceChildren()
    this.row.style.flexDirection = this.direction === 'rtl' ? 'row-reverse' : 'row'
    const count = this.spread && this.page < this.totalPages ? 2 : 1
    for (let offset = 0; offset < count; offset++) {
      const image = document.createElement('img')
      image.src = comicPageUrl(this.bookId, this.fileId, this.page + offset)
      image.alt = `Page ${this.page + offset}`
      image.loading = 'eager'
      image.decoding = 'async'
      image.className = 'block shrink-0 object-contain shadow-lg'
      image.style.maxWidth = count === 2 ? 'calc(50% - 0.25rem)' : '100%'
      image.style.maxHeight = this.fit === 'page' ? 'calc(100dvh - 11rem)' : 'none'
      image.style.width = this.fit === 'width' ? (count === 2 ? 'calc(50% - 0.25rem)' : '100%') : 'auto'
      this.row.append(image)
    }
    this.onLocation({ type: 'cbz', page: this.page, totalPages: this.totalPages,
      percentage: this.page / this.totalPages, atEnd: this.page >= this.totalPages })
    const next = this.page + count
    if (next <= this.totalPages) {
      const preload = new Image()
      preload.src = comicPageUrl(this.bookId, this.fileId, next)
    }
  }

  goTo(target: string) {
    const page = Number(target)
    if (!Number.isSafeInteger(page) || page < 1 || page > this.totalPages) throw new Error('Invalid comic page')
    this.page = page
    this.host.scrollTo(0, 0)
    this.render()
  }
  next() { this.goTo(String(Math.min(this.totalPages, this.page + (this.spread ? 2 : 1)))) }
  prev() { this.goTo(String(Math.max(1, this.page - (this.spread ? 2 : 1)))) }
  goToStart() { this.goTo('1') }
  setFit(fit: 'width' | 'page') { this.fit = fit; this.render() }
  setComicLayout(spread: boolean, direction: 'ltr' | 'rtl') {
    this.spread = spread
    this.direction = direction
    this.render()
  }
  async search(_query: string): Promise<ReaderSearchHit[]> { return [] }
  destroy() { this.destroyed = true; this.row.remove() }
}

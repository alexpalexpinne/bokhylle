import ePub, { EpubCFI, type Book, type Rendition, type Location } from 'epubjs'
import type { NavItem } from 'epubjs/types/navigation'
import type Section from 'epubjs/types/section'
import type Contents from 'epubjs/types/contents'
import type { ReaderEngine, ReaderPosition, ReaderSearchHit, ReaderTocEntry } from './ReaderEngine'
import { cfiSpineIndex, cfiToXPointer, xpointerSpineIndex, xPointerToCfi } from './XPointer'
import type { ReadingDirection } from './readingDirection'
import type { ReaderAppearance } from '../../lib/readerAppearance'

// Standard Ebooks keeps title and imprint text available to assistive technology
// by positioning it far offscreen. Epub.js includes that distance in its page
// measurements, yielding dozens of empty pages. Keep the text visually hidden
// inside the page instead, before Epub.js measures the section.
const offscreenTextStyles = `
section.epub-type-contains-word-titlepage h1,
section.epub-type-contains-word-titlepage p,
section.epub-type-contains-word-colophon h2,
section.epub-type-contains-word-imprint h2 {
  position: absolute !important;
  left: 0 !important;
  width: 1px !important;
  height: 1px !important;
  overflow: hidden !important;
  clip-path: inset(50%) !important;
}`

export class EpubAdapter implements ReaderEngine {
  readonly type = 'epub' as const
  private readonly book: Book
  private readonly rendition: Rendition
  private readonly frameObserver: MutationObserver
  private readonly element: HTMLElement
  private lastNavigation: Promise<unknown> = Promise.resolve()
  readonly supportsTypography: boolean
  readonly toc: ReaderTocEntry[]

  private constructor(
    book: Book,
    rendition: Rendition,
    toc: NavItem[],
    frameObserver: MutationObserver,
    element: HTMLElement,
    supportsTypography: boolean,
  ) {
    this.book = book
    this.rendition = rendition
    const convert = (items: NavItem[]): ReaderTocEntry[] => items.map((item) => ({
      label: item.label,
      target: item.href,
      children: convert(item.subitems ?? []),
    }))
    this.toc = convert(toc)
    this.frameObserver = frameObserver
    this.element = element
    this.supportsTypography = supportsTypography
  }

  private get sections(): Section[] {
    return (this.book.spine as Book['spine'] & { spineItems: Section[] }).spineItems
  }

  static async open(
    element: HTMLElement,
    bytes: ArrayBuffer,
    initialCfi: string | null,
    onLocation: (location: ReaderPosition) => void,
    onKeyDown: (event: KeyboardEvent) => void,
    resolveDirection: (embedded: ReadingDirection | null) => ReadingDirection,
    appearance: ReaderAppearance,
    onContentNavigation?: () => boolean,
  ): Promise<EpubAdapter> {
    const book = ePub()
    let rendition: Rendition | null = null
    let frameObserver: MutationObserver | null = null
    try {
      await book.open(bytes)
      await (book as Book & { loaded: { displayOptions: Promise<unknown> } }).loaded.displayOptions
      // Epub.js exposes the spine's page-progression-direction at runtime;
      // its bundled Book type omits the package metadata field.
      const packageDirection = (book as Book & { package: { metadata: { direction?: string } } }).package.metadata.direction
      const embeddedDirection = packageDirection === 'ltr' || packageDirection === 'rtl'
        ? packageDirection : null
      const direction = resolveDirection(embeddedDirection)
      const packageLayout = (book as Book & { package: { metadata: { layout?: string } } }).package.metadata.layout
      const displayOptions = (book as Book & { displayOptions?: { fixedLayout?: string } }).displayOptions
      const supportsTypography = packageLayout !== 'pre-paginated' && displayOptions?.fixedLayout !== 'true'
      book.spine.hooks.content.register((document: Document) => {
        const style = document.createElementNS('http://www.w3.org/1999/xhtml', 'style')
        style.textContent = offscreenTextStyles
        document.head.appendChild(style)
      })
      const navigation = await book.loaded.navigation
      // Epub.js's type definition omits `gap`; the runtime uses it when
      // calculating columns. A nonzero gap creates a phantom page for short
      // sections whose content fills the available width.
      const layout = {
        width: '100%',
        height: '100%',
        spread: 'none',
        gap: 0,
        allowScriptedContent: false,
      }
      rendition = book.renderTo(element, layout)
      await rendition.started
      rendition.direction(direction)
      rendition.hooks.content.register((contents: Contents) => {
        contents.document.addEventListener('click', (event) => {
          if ((event.target as Element | null)?.closest?.('a[href]') && onContentNavigation?.() === false) {
            event.preventDefault()
            event.stopPropagation()
          }
        }, true)
      })
      frameObserver = new MutationObserver(() => {
        element.querySelectorAll('iframe').forEach((frame) => { frame.title = 'EPUB book content' })
      })
      frameObserver.observe(element, { childList: true, subtree: true })
      const sections = Math.max(1, (book.spine as Book['spine'] & { spineItems: Section[] }).spineItems.length)
      rendition.on('relocated', (location: Location) => {
        const start = location.start
        if (!start?.cfi) return
        const page = start.displayed?.page ?? 1
        const total = Math.max(1, start.displayed?.total ?? 1)
        const percentage = location.atEnd
          ? 1
          : Math.min(1, Math.max(0, (start.index + (page - 1) / total) / sections))
        onLocation({ type: 'epub', cfi: start.cfi, percentage, atEnd: location.atEnd })
      })
      rendition.on('keydown', onKeyDown)
      const adapter = new EpubAdapter(book, rendition, navigation.toc ?? [], frameObserver, element, supportsTypography)
      adapter.applyStyles(appearance)
      if (initialCfi) {
        try {
          await rendition.display(initialCfi)
        } catch {
          await rendition.display()
        }
      } else {
        await rendition.display()
      }
      return adapter
    } catch (error) {
      frameObserver?.disconnect()
      rendition?.destroy()
      book.destroy()
      throw error
    }
  }

  private applyStyles(appearance: ReaderAppearance) {
    if (!this.supportsTypography) return
    const rootStyle = getComputedStyle(this.element)
    const ink = rootStyle.getPropertyValue('--c-ink').trim() || '#16120e'
    const paper = rootStyle.getPropertyValue('--c-canvas').trim() || '#f7f4ee'
    this.rendition.themes.default({
      'html, body': { color: ink, background: paper },
      body: {
        'font-family': appearance.fontFamily === 'sans' ? 'system-ui, sans-serif' : 'Georgia, serif',
        'line-height': appearance.lineSpacing === 'compact' ? '1.4' : appearance.lineSpacing === 'spacious' ? '1.8' : '1.55',
      },
      'a:link': { color: ink },
    })
    this.rendition.themes.fontSize(`${appearance.textScale}%`)
  }

  async setAppearance(appearance: ReaderAppearance, anchorCfi?: string) {
    this.applyStyles(appearance)
    if (!anchorCfi) return
    // Epub.js resolves display before its queued relocated event. Request a
    // fresh location after restoration, then wait for that report to arrive.
    await this.rendition.display(anchorCfi)
    await this.waitForReportedLocation()
  }

  private waitForReportedLocation(): Promise<void> {
    return new Promise<void>((resolve, reject) => {
      const timer = window.setTimeout(() => { cleanup(); reject(new Error('EPUB location did not settle')) }, 12000)
      let settleFrame = 0
      let finalFrame = 0
      const onRelocated = () => {
        window.cancelAnimationFrame(settleFrame)
        window.cancelAnimationFrame(finalFrame)
        // Layout can report more than once. Finish after two quiet frames.
        settleFrame = window.requestAnimationFrame(() => {
          finalFrame = window.requestAnimationFrame(() => { cleanup(); resolve() })
        })
      }
      const cleanup = () => {
        window.clearTimeout(timer)
        window.cancelAnimationFrame(settleFrame)
        window.cancelAnimationFrame(finalFrame)
        this.rendition.off('relocated', onRelocated)
      }
      this.rendition.on('relocated', onRelocated)
      void this.rendition.reportLocation()
    })
  }

  private trackNavigation(task: Promise<unknown>) {
    this.lastNavigation = task.then(() => this.waitForReportedLocation())
    return this.lastNavigation
  }

  waitForNavigation() { return this.lastNavigation }

  setReadingDirection(direction: ReadingDirection) {
    this.rendition.direction(direction)
  }

  next() { return this.trackNavigation(this.rendition.next()) }
  prev() { return this.trackNavigation(this.rendition.prev()) }
  goTo(target: string) { return this.trackNavigation(this.rendition.display(target)) }
  goToStart() { return this.trackNavigation(this.rendition.display(0)) }

  async toExternal(position: ReaderPosition): Promise<string> {
    if (position.type !== 'epub') throw new Error('Expected an EPUB position')
    const section = this.sections[cfiSpineIndex(position.cfi)]
    if (!section) throw new Error('CFI spine item is absent')
    await section.load(this.book.load.bind(this.book))
    try { return cfiToXPointer(section, position.cfi) }
    finally { section.unload() }
  }

  async goToExternal(locator: string) {
    const section = this.sections[xpointerSpineIndex(locator)]
    if (!section) throw new Error('XPointer spine item is absent')
    await section.load(this.book.load.bind(this.book))
    let cfi: string
    try { cfi = xPointerToCfi(section, locator) }
    finally { section.unload() }
    await this.goTo(cfi)
  }

  private chapterFor(href: string, items: ReaderTocEntry[] = this.toc): string | null {
    for (const item of items) {
      const chapterHref = item.target.split('#')[0]
      if (chapterHref && href.endsWith(chapterHref)) return item.label
      const child = this.chapterFor(href, item.children)
      if (child) return child
    }
    return null
  }

  private searchPercentage(section: Section, cfi: string, offsets: Map<Node, number>, length: number): number | null {
    try {
      if (length === 0) return null
      const match = new EpubCFI(cfi).toRange(section.document)
      const start = offsets.get(match.startContainer)
      if (start === undefined) return null
      const fraction = Math.min(1, Math.max(0, (start + match.startOffset) / length))
      return Math.min(1, Math.max(0, (section.index + fraction) / this.sections.length))
    } catch {
      return null
    }
  }

  async search(query: string, limit = 50): Promise<ReaderSearchHit[]> {
    const found: ReaderSearchHit[] = []
    const needle = query.trim()
    if (!needle) return found
    for (const section of this.sections) {
      await section.load(this.book.load.bind(this.book))
      const matches = section.find(needle) as unknown as Array<{ cfi: string; excerpt: string }>
      const offsets = new Map<Node, number>()
      let textLength = 0
      if (matches.length && section.document.body) {
        const walker = section.document.createTreeWalker(section.document.body, NodeFilter.SHOW_TEXT)
        for (let node = walker.nextNode(); node; node = walker.nextNode()) {
          offsets.set(node, textLength)
          textLength += node.textContent?.length ?? 0
        }
      }
      for (const match of matches) {
        found.push({
          target: match.cfi,
          excerpt: match.excerpt,
          section: this.chapterFor(section.href) ?? 'Book',
          percentage: this.searchPercentage(section, match.cfi, offsets, textLength),
        })
        if (found.length >= limit) break
      }
      section.unload()
      if (found.length >= limit) break
    }
    return found
  }

  destroy() {
    this.frameObserver.disconnect()
    this.rendition.destroy()
    this.book.destroy()
  }
}

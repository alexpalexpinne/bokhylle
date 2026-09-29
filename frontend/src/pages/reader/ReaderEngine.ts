import type { ReaderAppearance } from '../../lib/readerAppearance'

export type ReaderPosition =
  | { type: 'epub'; cfi: string; percentage: number; atEnd: boolean }
  | { type: 'pdf'; page: number; totalPages: number; percentage: number; atEnd: boolean }
  | { type: 'cbz'; page: number; totalPages: number; percentage: number; atEnd: boolean }

export type ReaderSearchHit = {
  target: string
  excerpt: string
  section: string
  percentage: number | null
}

export type ReaderTocEntry = { label: string; target: string; children: ReaderTocEntry[] }

export interface ReaderEngine {
  readonly type: ReaderPosition['type']
  readonly toc: ReaderTocEntry[]
  readonly supportsTypography?: boolean
  next(): Promise<unknown> | void
  prev(): Promise<unknown> | void
  goTo(target: string): Promise<unknown> | void
  goToStart(): Promise<unknown> | void
  search(query: string): Promise<ReaderSearchHit[]>
  destroy(): void
  setAppearance?(appearance: ReaderAppearance, anchorCfi?: string): Promise<void> | void
  waitForNavigation?(): Promise<unknown>
  setFit?(fit: 'width' | 'page', zoom: number): void
  setComicLayout?(spread: boolean, direction: 'ltr' | 'rtl'): void
  setReadingDirection?(direction: 'ltr' | 'rtl'): void
  toExternal?(position: ReaderPosition): Promise<string>
  goToExternal?(locator: string): Promise<unknown>
}

export function positionLocator(position: ReaderPosition): string {
  return position.type === 'epub' ? position.cfi : String(position.page)
}

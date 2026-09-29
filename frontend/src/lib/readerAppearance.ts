export type ReaderTheme = 'app' | 'paper' | 'warm' | 'ink'
export type ReaderFont = 'serif' | 'sans'
export type ReaderLineSpacing = 'compact' | 'standard' | 'spacious'

export type ReaderAppearance = {
  theme: ReaderTheme
  fontFamily: ReaderFont
  textScale: number
  lineSpacing: ReaderLineSpacing
}

export const DEFAULT_READER_APPEARANCE: ReaderAppearance = {
  theme: 'app',
  fontFamily: 'serif',
  textScale: 100,
  lineSpacing: 'standard',
}

function appearanceKey(userId: number) {
  return `bokhylle.readerAppearance:${userId}`
}

function validTextScale(value: unknown): value is number {
  return typeof value === 'number' && Number.isInteger(value) && value >= 80 && value <= 200 && value % 10 === 0
}

export function readReaderAppearance(userId?: number): ReaderAppearance {
  if (userId === undefined) return { ...DEFAULT_READER_APPEARANCE }
  let stored: unknown
  try {
    const raw = window.localStorage.getItem(appearanceKey(userId))
    stored = raw ? JSON.parse(raw) : null
  } catch {
    stored = null
  }
  const value = stored && typeof stored === 'object' && !Array.isArray(stored)
    ? stored as Partial<ReaderAppearance> : {}
  const legacyTextScale = Number(window.localStorage.getItem(`bokhylle-reader-font-${userId}`))
  return {
    theme: value.theme === 'paper' || value.theme === 'warm' || value.theme === 'ink' ? value.theme : 'app',
    fontFamily: value.fontFamily === 'sans' ? 'sans' : 'serif',
    textScale: validTextScale(value.textScale) ? value.textScale
      : validTextScale(legacyTextScale) ? legacyTextScale : 100,
    lineSpacing: value.lineSpacing === 'compact' || value.lineSpacing === 'spacious' ? value.lineSpacing : 'standard',
  }
}

export function storeReaderAppearance(userId: number, value: ReaderAppearance) {
  window.localStorage.setItem(appearanceKey(userId), JSON.stringify(value))
}

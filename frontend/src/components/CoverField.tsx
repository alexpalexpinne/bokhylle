import { useEffect, useState } from 'react'
import { coverUrl } from '../api/library'
import { readStoredTheme, resolvedTheme } from '../theme'

type Tone = { h: number; s: number }

const memory = new Map<string, Tone>()

function storageKey(bookId: number) {
  return `bokhylle.coverTone.${bookId}`
}

function readTone(key: string, bookId?: number): Tone | null {
  const cached = memory.get(key)
  if (cached) {
    return cached
  }
  try {
    const raw = bookId ? window.localStorage.getItem(storageKey(bookId)) : null
    if (raw) {
      const tone = JSON.parse(raw) as Tone
      memory.set(key, tone)
      return tone
    }
  } catch {
    // Ignore malformed cache entries.
  }
  return null
}

function rgbToHsl(r: number, g: number, b: number): Tone {
  const max = Math.max(r, g, b)
  const min = Math.min(r, g, b)
  let h = 0
  let s = 0
  const l = (max + min) / 2
  if (max !== min) {
    const d = max - min
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min)
    switch (max) {
      case r:
        h = (g - b) / d + (g < b ? 6 : 0)
        break
      case g:
        h = (b - r) / d + 2
        break
      default:
        h = (r - g) / d + 4
    }
    h /= 6
  }
  return { h: h * 360, s }
}

function sampleTone(src: string): Promise<Tone | null> {
  return new Promise((resolve) => {
    const image = new Image()
    image.onload = () => {
      try {
        const size = 16
        const canvas = document.createElement('canvas')
        canvas.width = size
        canvas.height = size
        const context = canvas.getContext('2d')
        if (!context) {
          resolve(null)
          return
        }
        context.drawImage(image, 0, 0, size, size)
        const { data } = context.getImageData(0, 0, size, size)
        let r = 0
        let g = 0
        let b = 0
        let count = 0
        for (let index = 0; index < data.length; index += 4) {
          if (data[index + 3] < 200) {
            continue
          }
          r += data[index]
          g += data[index + 1]
          b += data[index + 2]
          count += 1
        }
        if (count === 0) {
          resolve(null)
          return
        }
        resolve(rgbToHsl(r / count / 255, g / count / 255, b / count / 255))
      } catch {
        resolve(null)
      }
    }
    image.onerror = () => resolve(null)
    image.src = src
  })
}

/** Flat cover-derived field shared by Home Spotlight and Book Detail. */
function toneCss(tone: Tone | null, mode: 'paper' | 'ink'): string {
  if (!tone) {
    return mode === 'ink' ? 'hsl(28 16% 16%)' : 'hsl(36 26% 87%)'
  }
  const saturation = Math.min(tone.s, mode === 'ink' ? 0.22 : 0.24)
  const lightness = mode === 'ink' ? 0.17 : 0.85
  return `hsl(${Math.round(tone.h)} ${Math.round(saturation * 100)}% ${Math.round(lightness * 100)}%)`
}

export function CoverField({ bookId, coverSrc }: { bookId?: number; coverSrc?: string }) {
  const src = coverSrc ?? (bookId ? coverUrl(bookId) : '')
  const key = bookId ? `book:${bookId}` : src
  const [sampledTone, setSampledTone] = useState<{ key: string; tone: Tone | null }>(() => ({ key, tone: readTone(key, bookId) }))
  const tone = sampledTone.key === key ? sampledTone.tone : readTone(key, bookId)
  const [mode, setMode] = useState<'paper' | 'ink'>(() => resolvedTheme(readStoredTheme()))

  useEffect(() => {
    if (tone) {
      return
    }
    let cancelled = false
    sampleTone(src).then((sampled) => {
      if (cancelled || !sampled) {
        return
      }
      memory.set(key, sampled)
      if (bookId) {
        try {
          window.localStorage.setItem(storageKey(bookId), JSON.stringify(sampled))
        } catch {
          // Cache is best-effort.
        }
      }
      setSampledTone({ key, tone: sampled })
    })
    return () => {
      cancelled = true
    }
  }, [bookId, key, src, tone])

  useEffect(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)')
    const update = () => setMode(resolvedTheme(readStoredTheme()))
    media.addEventListener('change', update)
    const observer = new MutationObserver(update)
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme'],
    })
    return () => {
      media.removeEventListener('change', update)
      observer.disconnect()
    }
  }, [])

  return (
    <div
      aria-hidden
      className="absolute inset-0 transition-colors duration-500"
      style={{ backgroundColor: toneCss(tone, mode) }}
    />
  )
}

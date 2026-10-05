import { useEffect, useState } from 'react'

export type ThemeChoice = 'system' | 'paper' | 'ink'

const STORAGE_KEY = 'bokhylle.theme'
const PAPER_COLOR = '#f7f4ee'
const INK_COLOR = '#1a1511'

export function readStoredTheme(): ThemeChoice {
  const value = window.localStorage.getItem(STORAGE_KEY)
  return value === 'paper' || value === 'ink' ? value : 'system'
}

export function resolvedTheme(choice: ThemeChoice): 'paper' | 'ink' {
  if (choice !== 'system') {
    return choice
  }
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'ink' : 'paper'
}

export function applyTheme(choice: ThemeChoice) {
  const root = document.documentElement
  if (choice === 'system') {
    root.removeAttribute('data-theme')
  } else {
    root.dataset.theme = choice
  }
  const meta = document.querySelector('meta[name="theme-color"]')
  if (meta) {
    meta.setAttribute('content', resolvedTheme(choice) === 'ink' ? INK_COLOR : PAPER_COLOR)
  }
}

export function initTheme() {
  applyTheme(readStoredTheme())
}

export function storeTheme(choice: ThemeChoice) {
  window.localStorage.setItem(STORAGE_KEY, choice)
  applyTheme(choice)
}

export function useThemeChoice() {
  const [choice, setChoice] = useState<ThemeChoice>(() => readStoredTheme())

  useEffect(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)')
    const onChange = () => {
      if (readStoredTheme() === 'system') {
        applyTheme('system')
      }
    }
    media.addEventListener('change', onChange)
    return () => media.removeEventListener('change', onChange)
  }, [])

  function choose(next: ThemeChoice) {
    setChoice(next)
    storeTheme(next)
  }

  return { choice, choose }
}

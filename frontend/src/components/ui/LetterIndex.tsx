import { useEffect, useState } from 'react'

const LETTERS = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ'.split('')

/// A vertical jump bar. Only letters that have results are clickable, the
/// current letter follows the scroll position, and a click jumps to it.
export function LetterIndex({
  active,
  available,
  onSelect,
}: {
  active: string
  available: Set<string>
  onSelect: (letter: string) => void
}) {
  const [scrollLetter, setScrollLetter] = useState('')

  useEffect(() => {
    let frame = 0
    const update = () => {
      frame = 0
      const nodes = Array.from(document.querySelectorAll<HTMLElement>('[data-letter]'))
      let current = ''
      for (const node of nodes) {
        if (node.getBoundingClientRect().top < 160) {
          current = node.dataset.letter ?? current
        } else {
          break
        }
      }
      setScrollLetter(current)
    }
    const onScroll = () => {
      if (!frame) {
        frame = requestAnimationFrame(update)
      }
    }
    update()
    window.addEventListener('scroll', onScroll, { passive: true })
    return () => {
      window.removeEventListener('scroll', onScroll)
      if (frame) {
        cancelAnimationFrame(frame)
      }
    }
  }, [])

  const current = scrollLetter || active

  return (
    <div className="fixed right-2.5 top-1/2 z-30 hidden -translate-y-1/2 flex-col items-center gap-0.5 rounded-full bg-surface/80 px-1.5 py-2.5 shadow-lift ring-1 ring-line/60 backdrop-blur lg:flex">
      <button
        type="button"
        onClick={() => onSelect('')}
        aria-label="Show all letters"
        className={`mb-1 h-1.5 w-1.5 rounded-full transition-colors ${
          active === '' ? 'bg-accent' : 'bg-ink-faint/40 hover:bg-ink-faint'
        }`}
      />
      {LETTERS.map((letter) => {
        const value = letter.toLowerCase()
        const enabled = available.has(value)
        const isCurrent = current === value
        return (
          <button
            key={letter}
            type="button"
            disabled={!enabled}
            aria-label={`Jump to ${letter}`}
            aria-pressed={active === value}
            onClick={() => onSelect(value)}
            onPointerEnter={(event) => {
              if (event.buttons === 1 && enabled) {
                onSelect(value)
              }
            }}
            className={`w-4 rounded-full py-px text-center font-sans text-[10px] font-semibold leading-tight transition-all ${
              isCurrent
                ? 'scale-110 text-accent'
                : enabled
                  ? 'text-ink-muted hover:text-ink'
                  : 'cursor-default text-ink-faint/30'
            }`}
          >
            {letter}
          </button>
        )
      })}
    </div>
  )
}

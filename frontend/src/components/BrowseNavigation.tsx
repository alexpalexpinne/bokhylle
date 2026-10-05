import { useLayoutEffect, useRef, type RefObject } from 'react'
import { useLocation, type NavigationType } from 'react-router-dom'
import { browseSessionGeneration, readBrowseState, saveBrowseState } from '../lib/browseState'

type Position = { y: number; rails: Record<string, number>; focus: string | null; focusRail: string | null }

/** Restore a history entry after its shelves have returned to the DOM. */
export function BrowseNavigation({ profile, main, navigation }: { profile: string; main: RefObject<HTMLElement | null>; navigation: NavigationType }) {
  const location = useLocation()
  const previousPath = useRef<string | null>(null)
  const key = `position:${profile}:${location.key}`
  useLayoutEffect(() => {
    const content = main.current
    if (!content) return
    const samePage = previousPath.current === location.pathname
    previousPath.current = location.pathname
    const generation = browseSessionGeneration()
    const saved = navigation === 'POP' ? readBrowseState<Position>(key) : undefined
    const previous = history.scrollRestoration
    history.scrollRestoration = 'manual'
    let restoring = !!saved
    let focus: string | null = saved?.focus ?? null
    let focusRail: string | null = saved?.focusRail ?? null
    const snapshot = () => {
      if (restoring) return
      const rails = Object.fromEntries(Array.from(content.querySelectorAll<HTMLElement>('[data-browse-rail]'))
        .map((rail) => [rail.dataset.browseRail!, rail.scrollLeft]))
      saveBrowseState(key, { y: window.scrollY, rails, focus, focusRail }, generation)
    }
    const restore = () => {
      if (!saved || !restoring) return
      window.scrollTo({ top: saved.y, behavior: 'instant' })
      let railsReady = true
      for (const [label, left] of Object.entries(saved.rails)) {
        const rail = Array.from(content.querySelectorAll<HTMLElement>('[data-browse-rail]'))
          .find((element) => element.dataset.browseRail === label)
        if (rail) rail.scrollLeft = left
        else if (left > 0) railsReady = false
      }
      const link = saved.focus ? Array.from(content.querySelectorAll<HTMLAnchorElement>('a[href]'))
        .find((element) => element.getAttribute('href') === saved.focus && (!saved.focusRail || (element.closest('[data-browse-rail]') as HTMLElement | null)?.dataset.browseRail === saved.focusRail)) : null
      if (Math.abs(window.scrollY - saved.y) < 2 && railsReady && (!saved.focus || link)) {
        restoring = false
        ;(link ?? content).focus({ preventScroll: true })
      }
    }
    const observer = new MutationObserver(restore)
    observer.observe(content, { childList: true, subtree: true })
    const sizes = new ResizeObserver(restore)
    sizes.observe(content)
    const frame = requestAnimationFrame(() => {
      if (saved) restore()
      else if (!samePage) {
        window.scrollTo({ top: 0, behavior: 'instant' })
        if (navigation !== 'POP') content.focus({ preventScroll: true })
      }
    })
    const stop = () => { restoring = false; snapshot() }
    const timer = window.setTimeout(stop, 4000)
    const onFocus = (event: FocusEvent) => {
      const target = event.target
      if (target instanceof HTMLAnchorElement && content.contains(target)) {
        focus = target.getAttribute('href')
        focusRail = (target.closest('[data-browse-rail]') as HTMLElement | null)?.dataset.browseRail ?? null
        snapshot()
      }
    }
    const onClick = (event: MouseEvent) => {
      const link = event.target instanceof Element ? event.target.closest('a[href]') : null
      if (link instanceof HTMLAnchorElement && content.contains(link)) {
        focus = link.getAttribute('href')
        focusRail = (link.closest('[data-browse-rail]') as HTMLElement | null)?.dataset.browseRail ?? null
      }
      snapshot()
    }
    const onScroll = () => { if (!restoring) snapshot() }
    const onKey = (event: KeyboardEvent) => {
      if (['ArrowDown', 'ArrowUp', 'PageDown', 'PageUp', 'Home', 'End', ' '].includes(event.key)) stop()
    }
    document.addEventListener('scroll', onScroll, true)
    document.addEventListener('focusin', onFocus)
    document.addEventListener('click', onClick, true)
    window.addEventListener('wheel', stop, { passive: true })
    window.addEventListener('touchstart', stop, { passive: true })
    window.addEventListener('keydown', onKey)
    window.addEventListener('pagehide', snapshot)
    return () => {
      // The route DOM has already changed during layout-effect cleanup and
      // may have clamped the window to a shorter book page. Keep the position
      // captured before navigation instead of overwriting it with that clamp.
      cancelAnimationFrame(frame)
      clearTimeout(timer)
      observer.disconnect()
      sizes.disconnect()
      document.removeEventListener('scroll', onScroll, true)
      document.removeEventListener('focusin', onFocus)
      document.removeEventListener('click', onClick, true)
      window.removeEventListener('wheel', stop)
      window.removeEventListener('touchstart', stop)
      window.removeEventListener('keydown', onKey)
      window.removeEventListener('pagehide', snapshot)
      history.scrollRestoration = previous
    }
  }, [key, navigation, location.pathname, main])
  return null
}

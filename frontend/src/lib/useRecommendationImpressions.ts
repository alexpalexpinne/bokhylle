import { useEffect, useRef } from 'react'
import { recordImpressions } from '../api/recommendations'
import { homeSnapshotGeneration } from './homeSnapshot'

const pending = new Set<string>()
let pendingGeneration = -1
let flushTimer: ReturnType<typeof setTimeout> | undefined
function recordVisible(key: string) {
  const generation = homeSnapshotGeneration()
  if (pendingGeneration !== generation) { pending.clear(); pendingGeneration = generation }
  pending.add(key)
  if (flushTimer) return
  flushTimer = setTimeout(() => {
    flushTimer = undefined
    const keys = [...pending]
    pending.clear()
    if (pendingGeneration !== homeSnapshotGeneration()) return
    for (let i = 0; i < keys.length; i += 80) {
      void recordImpressions(keys.slice(i, i + 80)).catch(() => { /* Keep browsing usable offline. */ })
    }
  }, 80)
}

// A suggestion is seen after at least half of its cover is visible for a
// second in an active tab. Horizontal rail overflow does not count.
export function useRecommendationImpression(key: string | null | undefined) {
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const element = ref.current
    if (!element || !key) return
    const generation = homeSnapshotGeneration()
    let visible = false
    let sent = false
    let timer: ReturnType<typeof setTimeout> | undefined
    const update = () => {
      clearTimeout(timer)
      if (!visible || document.hidden || document.querySelector('[role="dialog"], dialog[open]') || sent) return
      timer = setTimeout(() => {
        if (generation !== homeSnapshotGeneration()) return
        sent = true
        recordVisible(key)
      }, 1000)
    }
    const observer = new IntersectionObserver(([entry]) => {
      visible = entry.isIntersecting && entry.intersectionRatio >= 0.5
      update()
    }, { threshold: 0.5 })
    observer.observe(element.querySelector('[data-book-cover], .spotlight-cover') ?? element)
    const dialogs = new MutationObserver((records) => {
      if (records.some((record) => record.type === 'attributes' || [...record.addedNodes, ...record.removedNodes].some((node) => node instanceof Element && (node.matches('[role="dialog"], dialog') || node.querySelector('[role="dialog"], dialog'))))) update()
    })
    dialogs.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ['role', 'open'] })
    document.addEventListener('visibilitychange', update)
    return () => { clearTimeout(timer); observer.disconnect(); dialogs.disconnect(); document.removeEventListener('visibilitychange', update) }
  }, [key])
  return ref
}

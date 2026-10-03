import { useEffect, useState } from 'react'
import { useLocation } from 'react-router-dom'

// The linked row may arrive after the page loads. Focus it when it mounts,
// including when a notification opens another item on the current page.
export function useLinkedItemRef(enabled = true) {
  const [element, setElement] = useState<HTMLElement | null>(null)
  const { key } = useLocation()

  useEffect(() => {
    if (!element || !enabled) return
    const frame = window.requestAnimationFrame(() => {
      element.scrollIntoView({ block: 'center' })
      element.focus({ preventScroll: true })
    })
    return () => window.cancelAnimationFrame(frame)
  }, [element, key, enabled])

  return setElement
}

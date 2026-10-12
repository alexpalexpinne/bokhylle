import { useCallback, useEffect, useRef, useState } from 'react'
import { ApiError } from '../api/client'
import {
  type NotificationsResponse,
  dismissNotification,
  fetchNotifications,
  markNotificationsRead,
} from '../api/notifications'

/// One shared notification poll: the menu renders the list and the pinned
/// approval block, the layout uses the counts for its own chrome.
export function useNotifications(profile: string) {
  const [loadedFor, setLoadedFor] = useState<string | null>(null)
  const [data, setData] = useState<NotificationsResponse | null>(null)
  const activeProfile = useRef(profile)
  const latestLoad = useRef(0)
  const revision = useRef(0)
  useEffect(() => { activeProfile.current = profile }, [profile])

  const load = useCallback(() => {
    const sequence = ++latestLoad.current
    const startedAt = revision.current
    fetchNotifications()
      .then((data) => {
        if (activeProfile.current !== profile || sequence !== latestLoad.current || startedAt !== revision.current) return
        setLoadedFor(profile)
        setData(data)
      })
      .catch((caught: unknown) => {
        if (!(caught instanceof ApiError)) {
          console.error(caught)
        }
      })
  }, [profile])

  useEffect(() => {
    load()
    const timer = setInterval(load, 60000)
    return () => clearInterval(timer)
  }, [load])

  const markRead = useCallback(async () => {
    await markNotificationsRead()
    if (activeProfile.current !== profile) return
    revision.current += 1
    setData((current) => current && ({
      ...current,
      items: current.items.map((item) => ({ ...item, read: true })),
      unread: 0,
    }))
  }, [profile])

  const dismiss = useCallback(async (id: number) => {
    await dismissNotification(id)
    if (activeProfile.current !== profile) return
    revision.current += 1
    setData((current) => {
      if (!current) return current
      const item = current.items.find((item) => item.id === id)
      return {
        ...current,
        items: current.items.filter((item) => item.id !== id),
        unread: Math.max(0, current.unread - (item && !item.read ? 1 : 0)),
      }
    })
  }, [profile])

  const removePendingRequest = useCallback((id: number) => {
    if (activeProfile.current !== profile) return
    revision.current += 1
    setData((current) => current && ({
      ...current,
      pendingRequests: Math.max(0, current.pendingRequests - (current.pendingRequestItems.some((item) => item.id === id) ? 1 : 0)),
      pendingRequestItems: current.pendingRequestItems.filter((item) => item.id !== id),
    }))
  }, [profile])

  return {
    items: loadedFor === profile ? data?.items ?? [] : [],
    unread: loadedFor === profile ? data?.unread ?? 0 : 0,
    pendingRequestItems: loadedFor === profile ? data?.pendingRequestItems ?? [] : [],
    markRead,
    dismiss,
    removePendingRequest,
    reload: load,
  }
}

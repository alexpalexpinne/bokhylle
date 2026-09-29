import { useCallback, useEffect, useRef, useState } from 'react'
import { ApiError } from '../api/client'
import {
  type Notification,
  fetchNotifications,
  markNotificationsRead,
} from '../api/notifications'
import { type BookRequest } from '../api/requests'

/// One shared notification poll: the menu renders the list and the pinned
/// approval block, the layout uses the counts for its own chrome.
export function useNotifications(userId?: number) {
  const [loadedFor, setLoadedFor] = useState<number | undefined>(undefined)
  const [items, setItems] = useState<Notification[]>([])
  const [unread, setUnread] = useState(0)
  const [pendingRequestItems, setPendingRequestItems] = useState<BookRequest[]>([])
  const activeUser = useRef(userId)
  useEffect(() => { activeUser.current = userId }, [userId])

  const load = useCallback(() => {
    fetchNotifications()
      .then((data) => {
        if (activeUser.current !== userId) return
        setLoadedFor(userId)
        setItems(data.items)
        setUnread(data.unread)
        setPendingRequestItems(data.pendingRequestItems)
      })
      .catch((caught: unknown) => {
        if (!(caught instanceof ApiError)) {
          console.error(caught)
        }
      })
  }, [userId])

  useEffect(() => {
    load()
    const timer = setInterval(load, 60000)
    return () => clearInterval(timer)
  }, [load])

  const markRead = useCallback(async () => {
    try {
      await markNotificationsRead()
      setItems((current) => current.map((item) => ({ ...item, read: true })))
      setUnread(0)
    } catch (caught) {
      if (!(caught instanceof ApiError)) {
        console.error(caught)
      }
    }
  }, [])

  const removePendingRequest = useCallback((id: number) => {
    setPendingRequestItems((current) => current.filter((item) => item.id !== id))
  }, [])

  return {
    items: loadedFor === userId ? items : [],
    unread: loadedFor === userId ? unread : 0,
    pendingRequestItems: loadedFor === userId ? pendingRequestItems : [],
    markRead,
    removePendingRequest,
    reload: load,
  }
}

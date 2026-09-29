import type { components } from './generated'
import { apiRoute } from './client'

export type Notification = components['schemas']['Notification']

export type NotificationsResponse = components['schemas']['NotificationsResponse']

export function fetchNotifications() {
  return apiRoute('/api/notifications', '/api/notifications')
}

export function markNotificationsRead() {
  return apiRoute('/api/notifications/read', '/api/notifications/read', { method: 'POST' })
}

import type { components } from './generated'
import { apiRoute } from './client'

export type DemoGet = components['schemas']['DemoGetActivity']

export type DemoSend = components['schemas']['DemoSendActivity']

export type DemoActivity = components['schemas']['DemoActivity']

export function startDemoGet(bookId: number, sendWhenReady = false) {
  return apiRoute('/api/demo/get', '/api/demo/get', {
    method: 'POST',
    json: { bookId, sendWhenReady },
  })
}

export function sendDemoBook(bookId: number) {
  return apiRoute('/api/demo/send', '/api/demo/send', {
    method: 'POST',
    json: { bookId },
  })
}

export function fetchDemoActivity() {
  return apiRoute('/api/demo/activity', '/api/demo/activity')
}

export function decideDemoRequest(id: number, decision: 'approve' | 'decline') {
  return apiRoute(decision === 'approve' ? '/api/demo/requests/{id}/approve' : '/api/demo/requests/{id}/decline', `/api/demo/requests/${id}/${decision}`, { method: 'POST' })
}

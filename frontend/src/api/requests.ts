import type { components } from './generated'
import { apiRoute } from './client'

export type RequestableBook = components['schemas']['RequestSearchItem']
export type RequestBookDetail = components['schemas']['RequestBookDetail']

export type BookRequestPhase =
  | 'requested'
  | 'looking'
  | 'getting'
  | 'ready'
  | 'declined'
  | 'unavailable'

export type BookRequest = components['schemas']['BookRequestView']

export function searchRequestCatalogue(
  query: string,
  type = 'any',
) {
  return apiRoute('/api/requests/search', '/api/requests/search', { query: { q: query, type } })
}

export function fetchRequestBook(provider: string, providerKey: string) {
  return apiRoute('/api/requests/book', '/api/requests/book', {
    query: { provider, providerKey },
  })
}

export function createBookRequest(
  provider: string,
  providerKey: string,
  sharing?: components['schemas']['BookSharing'],
) {
  return apiRoute('/api/requests', '/api/requests', {
    method: 'POST',
    json: { provider, providerKey, sharing },
  })
}

export function fetchBookRequests() {
  return apiRoute('/api/requests', '/api/requests')
}

export function approveBookRequest(id: number) {
  return apiRoute('/api/requests/{id}/approve', `/api/requests/${id}/approve`, { method: 'POST' })
}

export function declineBookRequest(id: number) {
  return apiRoute('/api/requests/{id}/decline', `/api/requests/${id}/decline`, { method: 'POST' })
}

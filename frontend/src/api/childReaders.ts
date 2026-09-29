import { apiRoute } from './client'

export function fetchChildReaders(userId: number) {
  return apiRoute('/api/admin/children/{id}/readers', `/api/admin/children/${userId}/readers`)
}

export function createChildReader(userId: number, input: {
  name: string
  address: string
  connector: 'email'
  deviceType: 'kindle' | 'pocketbook' | 'other'
}) {
  return apiRoute('/api/admin/children/{id}/readers', `/api/admin/children/${userId}/readers`, {
    method: 'POST', json: input,
  })
}

export function updateChildReader(userId: number, targetId: number, input: {
  name?: string
  address?: string
  enabled?: boolean
}) {
  return apiRoute('/api/admin/children/{id}/readers/{target_id}', `/api/admin/children/${userId}/readers/${targetId}`, {
    method: 'PUT', json: input,
  })
}

export function defaultChildReader(userId: number, targetId: number) {
  return apiRoute('/api/admin/children/{id}/readers/{target_id}/default', `/api/admin/children/${userId}/readers/${targetId}/default`, { method: 'POST' })
}

export function deleteChildReader(userId: number, targetId: number) {
  return apiRoute('/api/admin/children/{id}/readers/{target_id}', `/api/admin/children/${userId}/readers/${targetId}`, { method: 'DELETE' })
}

export function fetchChildReaderTokens(userId: number) {
  return apiRoute('/api/admin/children/{id}/reader-tokens', `/api/admin/children/${userId}/reader-tokens`)
}

export function createChildReaderToken(userId: number, name: string) {
  return apiRoute('/api/admin/children/{id}/reader-tokens', `/api/admin/children/${userId}/reader-tokens`, {
    method: 'POST', json: { name },
  })
}

export function revokeChildReaderToken(userId: number, tokenId: number) {
  return apiRoute('/api/admin/children/{id}/reader-tokens/{token_id}', `/api/admin/children/${userId}/reader-tokens/${tokenId}`, { method: 'DELETE' })
}

import type { components } from './generated'
import { apiRoute } from './client'

export type AuthMode = 'legacy' | 'pin' | 'password'

export type LoginUser = components['schemas']['LoginUser']

export function fetchLoginUsers() {
  return apiRoute('/api/auth/users', '/api/auth/users')
}

export function changeCredential(change: {
  current: string
  credentialType: 'pin' | 'password'
  credential: string
}) {
  return apiRoute('/api/profile/credential', '/api/profile/credential', {
    method: 'PUT',
    json: change,
  })
}

export function logoutAllDevices() {
  return apiRoute('/api/auth/logout-all', '/api/auth/logout-all', { method: 'POST' })
}

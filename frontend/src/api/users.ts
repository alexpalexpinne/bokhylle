import type { components } from './generated'
import { apiRoute } from './client'

export type AdminUser = components['schemas']['AdminUserView']

export function fetchUsers() {
  return apiRoute('/api/admin/users', '/api/admin/users')
}

export function createUser(input: {
  username: string
  credential: string
  credentialType: 'pin' | 'password'
  role: 'admin' | 'user'
  profileType?: 'adult' | 'child'
  displayName?: string
  preferredLanguages?: string[]
  canRequest?: boolean
  canDiscover?: boolean
  canAcquire?: boolean
}) {
  return apiRoute('/api/admin/users', '/api/admin/users', {
    method: 'POST',
    json: input,
  })
}

export function updateUser(
  id: number,
  update: {
    displayName?: string
    role?: 'admin' | 'user'
    credential?: string
    credentialType?: 'pin' | 'password'
    disabled?: boolean
    preferredLanguages?: string[]
    canRequest?: boolean
    canDiscover?: boolean
    canAcquire?: boolean
  },
) {
  return apiRoute('/api/admin/users/{id}', `/api/admin/users/${id}`, {
    method: 'PUT',
    json: update,
  })
}

export type IntegrityReport = components['schemas']['IntegrityStatus']

export function fetchIntegrity() {
  return apiRoute('/api/admin/integrity', '/api/admin/integrity')
}

export function fetchLogs(limit = 300) {
  return apiRoute('/api/admin/logs', '/api/admin/logs', { query: { limit } })
}

export function fetchUserProfiles() {
  return apiRoute('/api/admin/users/profiles', '/api/admin/users/profiles')
}

export function restartOnboarding(userId: number) {
  return apiRoute('/api/admin/users/{id}/restart-onboarding', `/api/admin/users/${userId}/restart-onboarding`, { method: 'POST' })
}

export function setUserProfileType(userId: number, profileType: 'adult' | 'child') {
  return apiRoute('/api/admin/users/{id}/profile-type', `/api/admin/users/${userId}/profile-type`, {
    method: 'PUT',
    json: { profileType },
  })
}

export type HouseholdMember = components['schemas']['HouseholdMember']

export function fetchHouseholdMembers() {
  return apiRoute('/api/household/members', '/api/household/members')
}

export type ShelfUser = components['schemas']['ShelfUser']

export function fetchBookShelfUsers(bookId: number) {
  return apiRoute('/api/books/{id}/shelf-users', `/api/books/${bookId}/shelf-users`)
}

export function setBookShelf(userId: number, bookId: number, onShelf: boolean) {
  return apiRoute('/api/users/{id}/shelf/{book_id}', `/api/users/${userId}/shelf/${bookId}`, {
    method: 'PUT',
    json: { onShelf },
  })
}

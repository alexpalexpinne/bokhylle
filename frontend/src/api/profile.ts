import type { components } from './generated'
import { apiRoute, uploadBinary } from './client'

export type ProfileUpdate = components['schemas']['ProfileUpdate']

export type LikedBook = components['schemas']['LikedBook']

export type ProfileStats = components['schemas']['ProfileStats']

export function fetchProfileStats() {
  return apiRoute('/api/profile/stats', '/api/profile/stats')
}

export function fetchLikedBooks() {
  return apiRoute('/api/profile/liked', '/api/profile/liked')
}

export type OnboardingState = components['schemas']['OnboardingState']

export function fetchOnboarding() {
  return apiRoute('/api/profile/onboarding', '/api/profile/onboarding')
}

export function saveInterests(subjects: string[]) {
  return apiRoute('/api/profile/interests', '/api/profile/interests', {
    method: 'PUT',
    json: { subjects },
  })
}

export function completeOnboarding() {
  return apiRoute('/api/profile/onboarded', '/api/profile/onboarded', { method: 'POST' })
}

export function updateProfile(update: ProfileUpdate) {
  return apiRoute('/api/profile', '/api/profile', {
    method: 'PUT',
    json: update,
  })
}

export function uploadProfilePicture(file: File) {
  return uploadBinary('/api/profile/avatar', file)
}

export function deleteProfilePicture() {
  return apiRoute('/api/profile/avatar', '/api/profile/avatar', { method: 'DELETE' })
}

export type ReaderToken = components['schemas']['ReaderToken']

export function fetchReaderTokens() {
  return apiRoute('/api/profile/tokens', '/api/profile/tokens')
}

export function createReaderToken(name: string) {
  return apiRoute('/api/profile/tokens', '/api/profile/tokens', {
    method: 'POST',
    json: { name },
  })
}

export function revokeReaderToken(id: number) {
  return apiRoute('/api/profile/tokens/{id}', `/api/profile/tokens/${id}`, { method: 'DELETE' })
}

export type AgentToken = components['schemas']['AgentToken']

export function fetchAgentTokens() {
  return apiRoute('/api/profile/agent-tokens', '/api/profile/agent-tokens')
}

export function createAgentToken(
  name: string,
  scope: 'read' | 'write',
) {
  return apiRoute('/api/profile/agent-tokens', '/api/profile/agent-tokens', {
    method: 'POST',
    json: { name, scope },
  })
}

export function revokeAgentToken(id: number) {
  return apiRoute('/api/profile/agent-tokens/{id}', `/api/profile/agent-tokens/${id}`, { method: 'DELETE' })
}

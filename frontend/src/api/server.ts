import { apiRoute } from './client'
import type { components } from './generated'

export type ServerStatus = components['schemas']['ServerStatus']
export type UpdateStatus = components['schemas']['UpdateStatus']
export type Diagnostics = components['schemas']['Diagnostics']
export type RestartChange = components['schemas']['RestartChange']

export function fetchServerStatus() {
  return apiRoute('/api/admin/server', '/api/admin/server')
}

export function fetchRestartChanges() {
  return apiRoute('/api/admin/server/restart', '/api/admin/server/restart')
}

export function fetchDiagnostics() {
  return apiRoute('/api/admin/server/diagnostics', '/api/admin/server/diagnostics')
}

export function fetchUpdates() {
  return apiRoute('/api/admin/server/updates', '/api/admin/server/updates')
}

export function checkUpdates() {
  return apiRoute('/api/admin/server/updates', '/api/admin/server/updates', { method: 'POST' })
}

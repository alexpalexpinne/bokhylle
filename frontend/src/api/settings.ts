import type { components } from './generated'
import { apiRoute } from './client'

export type AdminSettings = components['schemas']['SettingsResponse']

export function fetchAdminSettings() {
  return apiRoute('/api/admin/settings', '/api/admin/settings')
}

export function updateSetting(key: string, value: unknown) {
  return apiRoute('/api/admin/settings/{key}', `/api/admin/settings/${encodeURIComponent(key)}`, {
    method: 'PUT',
    json: { value },
  })
}

export type ConnectionTest =
  | components['schemas']['ConnectionResult']
  | components['schemas']['ConnectionStatus']

export function testProwlarr() {
  return apiRoute('/api/admin/integrations/prowlarr/test', '/api/admin/integrations/prowlarr/test', { method: 'POST' })
}

export function testTorznab() {
  return apiRoute('/api/admin/integrations/torznab/test', '/api/admin/integrations/torznab/test', { method: 'POST' })
}

export function testNewznab() {
  return apiRoute('/api/admin/integrations/newznab/test', '/api/admin/integrations/newznab/test', { method: 'POST' })
}

export function testSabnzbd() {
  return apiRoute('/api/admin/integrations/sabnzbd/test', '/api/admin/integrations/sabnzbd/test', { method: 'POST' })
}

export function testQbittorrent() {
  return apiRoute('/api/admin/integrations/qbittorrent/test', '/api/admin/integrations/qbittorrent/test', { method: 'POST' })
}

export function testSmtp() {
  return apiRoute('/api/admin/integrations/smtp/test', '/api/admin/integrations/smtp/test', { method: 'POST' })
}

export type IntegrationHealth = components['schemas']['IntegrationStatus']

export function fetchWatchStatus() {
  return apiRoute('/api/admin/maintenance/watch', '/api/admin/maintenance/watch')
}

export function fetchIntegrationHealth() {
  return apiRoute('/api/admin/integrations/status', '/api/admin/integrations/status')
}

export type LibraryHealth = components['schemas']['LibraryHealth']

export function fetchLibraryHealth() {
  return apiRoute('/api/admin/library-health', '/api/admin/library-health')
}

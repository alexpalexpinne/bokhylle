import type { components } from './generated'
import { apiRoute } from './client'

export type ImageJobStatus = components['schemas']['ImageJobStatus']

export function fetchImageJob() {
  return apiRoute('/api/admin/maintenance/images', '/api/admin/maintenance/images')
}

export function startImageJob() {
  return apiRoute('/api/admin/maintenance/images', '/api/admin/maintenance/images', { method: 'POST' })
}

export type JobFailure = components['schemas']['JobFailure']

export type MetadataJobStatus = components['schemas']['MetadataJobStatus']

export function fetchMetadataJob() {
  return apiRoute('/api/admin/maintenance/metadata', '/api/admin/maintenance/metadata')
}

export function startMetadataJob(force = false) {
  return apiRoute('/api/admin/maintenance/metadata', '/api/admin/maintenance/metadata', {
    method: 'POST',
    query: force ? { force: true } : {},
  })
}

export function cancelMetadataJob() {
  return apiRoute('/api/admin/maintenance/metadata/cancel', '/api/admin/maintenance/metadata/cancel', { method: 'POST' })
}

export type BackupStatus = components['schemas']['BackupStatus']

export function fetchBackupStatus() {
  return apiRoute('/api/admin/maintenance/backups', '/api/admin/maintenance/backups')
}

export type ImportJobStatus = components['schemas']['ImportJobStatus']

export function fetchImportJob() {
  return apiRoute('/api/admin/maintenance/imports', '/api/admin/maintenance/imports')
}

export function startImportJob() {
  return apiRoute('/api/admin/maintenance/imports', '/api/admin/maintenance/imports', { method: 'POST' })
}

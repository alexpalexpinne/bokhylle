import type { components } from './generated'
import { apiRoute } from './client'

export type AcquisitionStatus = components['schemas']['AcquisitionStatus']

export type Acquisition = components['schemas']['AcquisitionView']

export type AttentionItem = components['schemas']['AttentionItem']

export function fetchAttention() {
  return apiRoute('/api/admin/attention', '/api/admin/attention')
}

export type Candidate = components['schemas']['CandidateView']

export type AcquisitionDiagnostics = components['schemas']['AcquisitionDiagnostics']

/// Acquire a book Bokhylle already knows by id (metadata-only or wanted
/// books), so no provider resolution is needed.
export function createAcquisitionForBook(
  bookId: number,
  options: { preferredFormat?: string; sendToReader?: boolean; sharing?: 'private' | 'shared'; askBeforeDownload?: boolean } = {},
) {
  return apiRoute('/api/books/{book_id}/acquisitions', `/api/books/${bookId}/acquisitions`, {
    method: 'POST',
    json: {
      preferredFormat: options.preferredFormat,
      sendToReader: options.sendToReader,
      sharing: options.sharing,
      askBeforeDownload: options.askBeforeDownload,
    },
  })
}

export function createHttpAcquisitionForBook(bookId: number, url: string, format?: string, sharing?: 'private' | 'shared') {
  return apiRoute('/api/books/{book_id}/acquisitions/http', `/api/books/${bookId}/acquisitions/http`, {
    method: 'POST',
    json: { url, format, sharing },
  })
}

export function createAcquisitionFromDiscovery(
  provider: string,
  providerKey: string,
  options: { preferredFormat?: string; sendToReader?: boolean; sharing?: 'private' | 'shared'; askBeforeDownload?: boolean } = {},
) {
  return apiRoute('/api/discover/acquisitions', '/api/discover/acquisitions', {
    method: 'POST',
    json: {
      provider,
      providerKey,
      preferredFormat: options.preferredFormat,
      sendToReader: options.sendToReader,
      sharing: options.sharing,
      askBeforeDownload: options.askBeforeDownload,
    },
  })
}

export function fetchAcquisitions(
  limit = 100,
  household = false,
) {
  return apiRoute('/api/acquisitions', '/api/acquisitions', {
    query: { limit, ...(household ? { scope: 'household' } : {}) },
  })
}

export function inspectAcquisition(id: string) {
  return apiRoute('/api/acquisitions/{id}/inspect', `/api/acquisitions/${id}/inspect`, { method: 'POST' })
}

export function setKeepLooking(id: string, enabled: boolean) {
  return apiRoute('/api/acquisitions/{id}/keep-looking', `/api/acquisitions/${id}/keep-looking`, {
    method: 'POST',
    json: { enabled },
  })
}

export function retryAcquisition(id: string) {
  return apiRoute('/api/acquisitions/{id}/retry', `/api/acquisitions/${id}/retry`, { method: 'POST' })
}

export function cancelAcquisition(id: string) {
  return apiRoute('/api/acquisitions/{id}/cancel', `/api/acquisitions/${id}/cancel`, { method: 'POST' })
}

export function fetchCandidates(id: string, technical = false) {
  return apiRoute('/api/acquisitions/{id}/candidates', `/api/acquisitions/${id}/candidates`, {
    query: technical ? { technical: true } : {},
  })
}

export function selectCandidate(id: string, index: number) {
  return apiRoute('/api/acquisitions/{id}/select', `/api/acquisitions/${id}/select`, {
    method: 'POST',
    json: { index },
  })
}

export function fetchDiagnostics(id: string) {
  return apiRoute('/api/admin/acquisitions/{id}', `/api/admin/acquisitions/${id}`)
}

export function availabilityLabel(seeders: number | null | undefined, method = 'torrent'): {
  label: string
  className: string
} {
  if (method === 'nzb') {
    return { label: 'Available from Usenet', className: 'text-success' }
  }
  if (seeders === null || seeders === undefined) {
    return { label: 'Availability unknown', className: 'text-ink-faint' }
  }
  if (seeders >= 20) {
    return { label: 'Excellent availability', className: 'text-success' }
  }
  if (seeders >= 5) {
    return { label: 'Good availability', className: 'text-success' }
  }
  if (seeders >= 1) {
    return { label: 'Limited availability', className: 'text-warning' }
  }
  return { label: 'No known seeders', className: 'text-danger' }
}

export function isActiveStatus(status: AcquisitionStatus): boolean {
  return [
    'REQUESTED',
    'SEARCHING',
    'EVALUATING',
    'QUEUED',
    'DOWNLOADING',
    'DOWNLOADED',
    'INSPECTING',
    'IDENTIFIED',
    'IMPORTING',
    'NEEDS_SELECTION',
    'NEEDS_REVIEW',
  ].includes(status)
}

export function statusLabel(acquisition: Acquisition): string {
  switch (acquisition.status) {
    case 'REQUESTED':
    case 'SEARCHING':
    case 'EVALUATING':
      return 'Finding book...'
    case 'QUEUED':
      return 'Starting download...'
    case 'DOWNLOADING':
      return `Downloading ${Math.floor(acquisition.progress)}%`
    case 'DOWNLOADED':
      return 'Downloaded'
    case 'INSPECTING':
    case 'IDENTIFIED':
    case 'IMPORTING':
      return 'Adding to library...'
    case 'READY':
      return 'In Library'
    case 'IMPORT_FAILED':
      switch (acquisition.errorCode) {
        case 'content_missing':
          return 'Downloaded files were not found'
        default:
          return 'Import failed'
      }
    case 'NEEDS_REVIEW':
      return 'Needs review'
    case 'NO_RELEASE_FOUND':
      return 'No suitable copy found'
    case 'NEEDS_SELECTION':
      return 'Choose a version'
    case 'CANCELLED':
      return 'Cancelled'
    case 'DOWNLOAD_FAILED':
      switch (acquisition.errorCode) {
        case 'integrations_not_configured':
          return 'Acquisition is not configured'
        case 'search_failed':
          return 'Could not search for releases'
        case 'download_missing':
          return 'Download disappeared'
        default:
          return 'Download failed'
      }
  }
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  const units = ['KB', 'MB', 'GB']
  let value = bytes / 1024
  let index = 0
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024
    index += 1
  }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[index]}`
}

export type DirectActivity = components['schemas']['DirectActivityItem']

export function fetchDirectActivity() {
  return apiRoute('/api/activity/direct', '/api/activity/direct')
}

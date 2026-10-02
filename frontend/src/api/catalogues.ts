import type { components } from './generated'
import { apiRoute } from './client'

export type CatalogSource = components['schemas']['CatalogSource']
export type CatalogFeed = components['schemas']['CatalogFeed']
export type CatalogEntry = components['schemas']['CatalogEntry']

export function fetchCatalogSources() {
  return apiRoute('/api/catalogues', '/api/catalogues')
}

export function addCatalogSource(name: string, url: string) {
  return apiRoute('/api/catalogues', '/api/catalogues', {
    method: 'POST',
    json: { name, url },
  })
}

export function removeCatalogSource(id: string) {
  return apiRoute('/api/catalogues/{id}', `/api/catalogues/${id}`, {
    method: 'DELETE',
  })
}

export function fetchCatalogFeed(id: string, options: { url?: string; q?: string } = {}) {
  return apiRoute('/api/catalogues/{id}/feed', `/api/catalogues/${id}/feed`, {
    query: options,
  })
}

export function acquireCatalogEntry(
  id: string,
  pageUrl: string,
  entryId: string,
  fileIndex: number,
  sharing?: 'private' | 'shared',
) {
  return apiRoute('/api/catalogues/{id}/acquisitions', `/api/catalogues/${id}/acquisitions`, {
    method: 'POST',
    json: { pageUrl, entryId, fileIndex, sharing },
  })
}

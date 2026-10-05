import { withHomeInvalidation } from '../lib/homeSnapshot'
import type { components } from './generated'
import { apiRoute } from './client'

export type DiscoveryStatus = components['schemas']['DiscoveryStatus']

export type DiscoveryResult = components['schemas']['DiscoveryResult']

export type DiscoveryDetail = components['schemas']['DiscoveryDetail']

export type ReleasePreview = components['schemas']['ReleaseView']

export type SearchType = 'any' | 'title' | 'author' | 'isbn' | 'subject'

export function localDiscoveryBookId(provider: string, providerKey: string): number | null {
  if (provider !== 'local' || !/^local:[1-9]\d*$/.test(providerKey)) return null
  const id = Number(providerKey.slice(6))
  return Number.isSafeInteger(id) ? id : null
}

export function searchDiscover(query: string, type: SearchType) {
  return apiRoute('/api/discover/search', '/api/discover/search', { query: { q: query, type } })
}

export function likeExternalBook(
  providerKey: string,
  provider?: string,
) {
  return withHomeInvalidation(apiRoute('/api/discover/like', '/api/discover/like', {
    method: 'POST',
    json: { providerKey, provider },
  }))
}

export function fetchDiscoverBook(
  providerKey: string,
  provider?: string,
) {
  return apiRoute('/api/discover/book', '/api/discover/book', {
    query: { providerKey, ...(provider ? { provider } : {}) },
  })
}

export function fetchReleases(
  providerKey: string,
  format?: string,
  provider?: string,
) {
  return apiRoute('/api/discover/releases', '/api/discover/releases', {
    query: { providerKey, ...(format ? { format } : {}), ...(provider ? { provider } : {}) },
  })
}

export type DiscoverPage = components['schemas']['DiscoveryPage']

/// The orchestrated discovery response: books and authors decided together,
/// with intent ranking already applied server-side.
export function fetchDiscoverOrchestration(params: {
  q: string
  type: string
  continuation?: string
  localOnly?: boolean
  limit?: number
}) {
  return apiRoute('/api/discover', '/api/discover', {
    query: {
      q: params.q,
      type: params.type,
      limit: params.limit ?? 24,
      ...(params.continuation ? { continuation: params.continuation } : {}),
      ...(params.localOnly ? { source: 'local' } : {}),
    },
  })
}

export function discoverCoverUrl(coverId: string, title: string, provider?: string): string {
  const params = new URLSearchParams({ title })
  if (provider) {
    params.set('provider', provider)
  }
  return `/api/discover/cover/${encodeURIComponent(coverId)}?${params.toString()}`
}

export function rejectExternalBook(providerKey: string, provider?: string) {
  return withHomeInvalidation(apiRoute('/api/discover/preference', '/api/discover/preference', {
    method: 'POST', json: { providerKey, provider, preference: 'not_for_me' },
  }))
}

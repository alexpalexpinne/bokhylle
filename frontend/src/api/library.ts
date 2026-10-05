import { withHomeInvalidation } from '../lib/homeSnapshot'
import { apiRoute } from './client'
import type { components } from './generated'

export type BookSummary = components['schemas']['BookSummary']

export type Edition = components['schemas']['EditionDetail']

export type FileInfo = components['schemas']['FileDetail']

export type SubjectRef = components['schemas']['SubjectName']

export type BookDetail = components['schemas']['BookDetail']

export type BookPage = components['schemas']['BookPage']

export type AuthorSummary = components['schemas']['AuthorSummary']

export type AuthorDetail = components['schemas']['AuthorDetail']

export type AuthorProfile = components['schemas']['AuthorProfileView']

export type BookSort = 'recent' | 'title' | 'author'

export type BookFilters = {
  mine?: boolean
  /// One member's shelf (adults browsing a child's shelf, for example).
  user?: number
  format?: string
  kind?: string
  language?: string
  series?: string
  subject?: string
  collection?: number
  letter?: string
  missing?: string
}

function filterQuery(filters: BookFilters) {
  return {
    mine: filters.mine || undefined,
    user: filters.user || undefined,
    format: filters.format || undefined,
    kind: filters.kind || undefined,
    language: filters.language || undefined,
    series: filters.series || undefined,
    subject: filters.subject || undefined,
    collection: filters.collection || undefined,
    missing: filters.missing || undefined,
  }
}

export type FacetValue = components['schemas']['FacetValue']

export type SubjectFacet = components['schemas']['SubjectFacet']

export type BookFacets = components['schemas']['BookFacets']

export function fetchBooks(
  sort: BookSort,
  page: number,
  pageSize = 24,
  filters: BookFilters = {},
) {
  return apiRoute('/api/books', '/api/books', {
    query: { sort, page, pageSize, ...filterQuery(filters), letter: filters.letter || undefined },
  })
}

export function fetchBookFacets(mine = true, user?: number) {
  return apiRoute('/api/books/facets', '/api/books/facets', {
    query: user ? { user } : mine ? {} : { scope: 'household' },
  })
}

export function searchBooks(query: string, filters: BookFilters = {}) {
  return apiRoute('/api/books/search', '/api/books/search', {
    query: { q: query, ...filterQuery(filters) },
  })
}

export function fetchRecent(limit = 12, mine = true) {
  return apiRoute('/api/books/recent', '/api/books/recent', {
    query: { limit, ...(mine ? {} : { scope: 'household' }) },
  })
}

export type ContinueReadingItem = components['schemas']['ReadingProgress']

export function fetchContinueReading() {
  return apiRoute('/api/books/continue', '/api/books/continue')
}

export type HomeRail = components['schemas']['HomeRail']

export function fetchHomeRails() {
  return apiRoute('/api/home/rails', '/api/home/rails')
}

export type SpotlightItem = components['schemas']['SpotlightItem']

export function fetchSpotlight(cachedOnly = false) {
  return apiRoute('/api/home/spotlight', '/api/home/spotlight', {
    query: { cachedOnly },
  })
}

export type Updates = components['schemas']['UpdatesResponse']

export function fetchUpdates() {
  return apiRoute('/api/home/updates', '/api/home/updates')
}

export function fetchHiddenSubjects() {
  return apiRoute('/api/home/subjects', '/api/home/subjects')
}

export function setSubjectHidden(normalized: string, hidden: boolean) {
  return withHomeInvalidation(apiRoute('/api/home/subjects/{normalized}', `/api/home/subjects/${encodeURIComponent(normalized)}`, {
    method: 'PUT',
    json: { hidden },
  }))
}

export function fetchHighlights(limit = 12, mine = true) {
  return apiRoute('/api/books/highlights', '/api/books/highlights', {
    query: { limit, ...(mine ? {} : { scope: 'household' }) },
  })
}

export type BookUpdate = components['schemas']['BookUpdateInput']
export type MetadataField = components['schemas']['MetadataField']

export function updateBookAdmin(id: number, update: BookUpdate) {
  return apiRoute('/api/admin/books/{id}', `/api/admin/books/${id}`, {
    method: 'PUT',
    json: update,
  })
}

export function deleteBookAdmin(id: number) {
  return apiRoute('/api/admin/books/{id}', `/api/admin/books/${id}`, { method: 'DELETE' })
}

export function deleteBookFileAdmin(bookId: number, fileId: number) {
  return apiRoute('/api/admin/books/{book_id}/files/{file_id}', `/api/admin/books/${bookId}/files/${fileId}`, { method: 'DELETE' })
}

export function fetchBook(id: number) {
  return apiRoute('/api/books/{id}', `/api/books/${id}`)
}

export function setBookSharing(id: number, sharing: 'private' | 'shared') {
  return withHomeInvalidation(apiRoute('/api/books/{id}/sharing', `/api/books/${id}/sharing`, { method: 'PUT', json: { sharing } }))
}

export function setBooksSharing(bookIds: number[], sharing: 'private' | 'shared') {
  return withHomeInvalidation(apiRoute('/api/books/sharing', '/api/books/sharing', { method: 'PUT', json: { bookIds, sharing } }))
}

export type SimilarBook = components['schemas']['SimilarBook']

export type RelatedBooks = components['schemas']['RelatedBooks']

export function fetchRelatedBooks(id: number) {
  return apiRoute('/api/books/{id}/related', `/api/books/${id}/related`)
}

export function fetchAuthors(
  mine = true,
  following = false,
  user?: number,
) {
  return apiRoute('/api/authors', '/api/authors', {
    query: {
      ...(user ? { user } : mine ? {} : { scope: 'household' }),
      ...(following ? { following: true } : {}),
    },
  })
}

export type AuthorFollowState = components['schemas']['FollowState']

export type AuthorHit = components['schemas']['AuthorHit']

export type AuthorSearchResult = components['schemas']['AuthorSearchResponse']

export function searchDiscoverAuthors(
  query: string,
  localOnly = false,
) {
  return apiRoute('/api/discover/authors', '/api/discover/authors', {
    query: { q: query, ...(localOnly ? { source: 'local' } : {}) },
  })
}

export function followDiscoverAuthor(input: {
  name: string
  provider: string | null
  providerKey: string | null
}) {
  return withHomeInvalidation(apiRoute('/api/discover/authors/follow', '/api/discover/authors/follow', {
    method: 'POST',
    json: input,
  }))
}

/// Materialize a transient author without following, so their page can be
/// opened before deciding to follow.
export function ensureDiscoverAuthor(input: {
  name: string
  provider: string | null
  providerKey: string | null
}) {
  return apiRoute('/api/discover/authors/ensure', '/api/discover/authors/ensure', {
    method: 'POST',
    json: input,
  })
}

export type CatalogueWork = components['schemas']['AuthorCatalogueItem']

export function fetchAuthorCatalogue(
  id: number,
  page = 1,
  sort: 'newest' | 'title' = 'newest',
) {
  return apiRoute('/api/authors/{id}/catalogue', `/api/authors/${id}/catalogue`, {
    query: { page, sort },
  })
}

export function fetchAuthorFollow(id: number) {
  return apiRoute('/api/authors/{id}/follow', `/api/authors/${id}/follow`)
}

export function setAuthorAutomation(
  id: number,
  autoAcquire: boolean,
  deliveryTargetId: number | null,
) {
  return apiRoute('/api/authors/{id}/automation', `/api/authors/${id}/automation`, {
    method: 'PUT',
    json: { autoAcquire, deliveryTargetId },
  })
}

export function setAuthorFollow(id: number, following: boolean) {
  return withHomeInvalidation(apiRoute('/api/authors/{id}/follow', `/api/authors/${id}/follow`, { method: following ? 'POST' : 'DELETE' }))
}

export function fetchAuthor(id: number) {
  return apiRoute('/api/authors/{id}', `/api/authors/${id}`)
}

export function fetchAuthorProfile(id: number) {
  return apiRoute('/api/authors/{id}/profile', `/api/authors/${id}/profile`)
}

export function coverUrl(bookId: number): string {
  return `/api/books/${bookId}/cover`
}

export function downloadUrl(bookId: number, fileId: number): string {
  return `/api/books/${bookId}/files/${fileId}/download`
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  const units = ['KB', 'MB', 'GB']
  let value = bytes / 1024
  let unitIndex = 0
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024
    unitIndex += 1
  }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[unitIndex]}`
}

export type ScanSummary = components['schemas']['ScanSummary']

export type ScanStatus = components['schemas']['ScanStatus']

export function triggerScan() {
  return apiRoute('/api/library/scan', '/api/library/scan', { method: 'POST' })
}

export function fetchScanStatus() {
  return apiRoute('/api/library/scan/status', '/api/library/scan/status')
}

export function authorList(authors: string[]): string {
  return authors.length > 0 ? authors.join(', ') : 'Unknown author'
}

export function addBookToShelf(id: number) {
  return withHomeInvalidation(apiRoute('/api/books/{id}/shelf', `/api/books/${id}/shelf`, { method: 'PUT' }))
}

export function removeBookFromShelf(id: number) {
  return withHomeInvalidation(apiRoute('/api/books/{id}/shelf', `/api/books/${id}/shelf`, { method: 'DELETE' }))
}

export function claimShelf() {
  return withHomeInvalidation(apiRoute('/api/books/shelf/claim-all', '/api/books/shelf/claim-all', { method: 'POST' }))
}

export function setBookPreference(
  id: number,
  preference: 'liked' | 'not_for_me' | null,
) {
  return withHomeInvalidation(apiRoute('/api/books/{id}/preference', `/api/books/${id}/preference`, {
    method: 'PUT',
    json: { preference },
  }))
}

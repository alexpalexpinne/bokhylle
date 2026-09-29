import type { components } from './generated'
import { apiRoute } from './client'

export type CollectionSummary = components['schemas']['CollectionSummary']

export type CollectionDetail = components['schemas']['CollectionDetail']

export function fetchCollections() {
  return apiRoute('/api/collections', '/api/collections')
}

export function fetchCollection(id: number) {
  return apiRoute('/api/collections/{id}', `/api/collections/${id}`)
}

export function createCollection(name: string) {
  return apiRoute('/api/collections', '/api/collections', {
    method: 'POST',
    json: { name },
  })
}

export function deleteCollection(id: number) {
  return apiRoute('/api/collections/{id}', `/api/collections/${id}`, { method: 'DELETE' })
}

export function addBookToCollection(collectionId: number, bookId: number) {
  return apiRoute('/api/collections/{id}/books', `/api/collections/${collectionId}/books`, {
    method: 'POST',
    json: { bookId },
  })
}

export function removeBookFromCollection(collectionId: number, bookId: number) {
  return apiRoute('/api/collections/{id}/books/{book_id}', `/api/collections/${collectionId}/books/${bookId}`, { method: 'DELETE' })
}

export function fetchBookCollections(bookId: number) {
  return apiRoute('/api/books/{book_id}/collections', `/api/books/${bookId}/collections`)
}

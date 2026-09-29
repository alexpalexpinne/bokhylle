import { notifySessionExpired } from '../auth/session'
import { ApiError, apiRoute } from './client'
import type { components } from './generated'

export type BrowserPositionState = components['schemas']['BrowserPositionState']
export type BrowserPosition = components['schemas']['BrowserPosition']
export type BookCompletionState = components['schemas']['BookCompletionState']

export function setBookCompletion(bookId: number, completed: boolean) {
  return apiRoute(
    '/api/books/{book_id}/completion',
    `/api/books/${bookId}/completion`,
    { method: 'PUT', json: { completed } },
  )
}

export function positionUrl(bookId: number, fileId: number): `/api/books/${number}/files/${number}/position` {
  return `/api/books/${bookId}/files/${fileId}/position`
}

export function contentUrl(bookId: number, fileId: number): string {
  return `/api/books/${bookId}/files/${fileId}/content`
}

export function comicPageUrl(bookId: number, fileId: number, page: number): string {
  return `/api/books/${bookId}/files/${fileId}/pages/${page}`
}

export function fetchComicManifest(bookId: number, fileId: number) {
  return apiRoute(
    '/api/books/{book_id}/files/{file_id}/pages',
    `/api/books/${bookId}/files/${fileId}/pages`,
  )
}

export function fetchBrowserPosition(bookId: number, fileId: number) {
  return apiRoute(
    '/api/books/{book_id}/files/{file_id}/position',
    positionUrl(bookId, fileId),
  )
}

export function saveBrowserPosition(
  bookId: number,
  fileId: number,
  update: components['schemas']['SaveBrowserPosition'],
) {
  return apiRoute(
    '/api/books/{book_id}/files/{file_id}/position',
    positionUrl(bookId, fileId),
    { method: 'PUT', json: update },
  )
}

export function setReadingDirection(bookId: number, fileId: number, direction: 'ltr' | 'rtl' | null) {
  return apiRoute(
    '/api/books/{book_id}/files/{file_id}/direction',
    `/api/books/${bookId}/files/${fileId}/direction`,
    { method: 'PUT', json: { direction } },
  )
}

export async function fetchEpubBytes(bookId: number, fileId: number, signal: AbortSignal) {
  const response = await fetch(contentUrl(bookId, fileId), {
    credentials: 'include',
    cache: 'no-store',
    signal,
  })
  if (!response.ok) {
    if (response.status === 401) notifySessionExpired()
    const body = await response.json().catch(() => null) as { message?: string; code?: string } | null
    throw new ApiError(body?.message ?? 'Could not open this EPUB', body?.code ?? 'reader_error', response.status)
  }
  return response.arrayBuffer()
}

import { apiRoute } from './client'
import type { components } from './generated'

export type ComicShelfPage = components['schemas']['ComicShelfPage']
export type SeriesDetail = components['schemas']['SeriesDetail']
export type SeriesRecord = components['schemas']['SeriesRecord']

export function fetchComicShelf(page = 1, mine = true, user?: number, sort: 'recent' | 'title' = 'recent') {
  return apiRoute('/api/library/comics', '/api/library/comics', {
    query: { page, pageSize: 24, mine, user, sort },
  })
}

export function fetchComicSeries(id: number, mine = true, user?: number) {
  return apiRoute('/api/series/{id}', `/api/series/${id}`, {
    query: { mine, user },
  })
}

export function fetchAdminSeries() {
  return apiRoute('/api/admin/series', '/api/admin/series')
}

export function createAdminSeries(name: string) {
  return apiRoute('/api/admin/series', '/api/admin/series', {
    method: 'POST', json: { name },
  })
}

export function updateAdminSeries(id: number, update: components['schemas']['UpdateSeriesInput']) {
  return apiRoute('/api/admin/series/{id}', `/api/admin/series/${id}`, {
    method: 'PUT', json: update,
  })
}

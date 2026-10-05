import { apiRoute } from './client'
import { withHomeInvalidation } from '../lib/homeSnapshot'
import type { components } from './generated'

export type RecommendationsPage = components['schemas']['RecommendationsPage']
export type SeriesContinuation = components['schemas']['SeriesContinuation']

export function fetchRecommendations(cachedOnly = false, subject?: string) {
  return apiRoute('/api/recommendations', '/api/recommendations', {
    query: { cachedOnly, limit: 72, ...(subject ? { subject } : {}) },
  })
}

export function recommendationFeedback(key: string, action: 'like' | 'not_for_me' | 'dismiss') {
  return withHomeInvalidation(apiRoute('/api/recommendations/{key}/feedback', `/api/recommendations/${encodeURIComponent(key)}/feedback`, {
    method: 'POST', json: { action },
  }))
}

export function undoRecommendationFeedback(key: string, token: string) {
  return withHomeInvalidation(apiRoute('/api/recommendations/{key}/feedback/{token}', `/api/recommendations/${encodeURIComponent(key)}/feedback/${encodeURIComponent(token)}`, { method: 'DELETE' }))
}

export function recordImpressions(keys: string[]) {
  return apiRoute('/api/recommendations/impressions', '/api/recommendations/impressions', { method: 'POST', json: { keys } })
}

export function fetchSeriesContinuations() {
  return apiRoute('/api/home/series', '/api/home/series')
}

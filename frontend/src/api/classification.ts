import { apiRoute } from './client'
import type { components } from './generated'

export type ClassificationReviewPage = components['schemas']['ClassificationReviewPage']
export type ClassificationReviewItem = components['schemas']['ClassificationReviewItem']
export type ClassificationDecision = components['schemas']['ClassificationDecision']

export type ReviewStatus = 'pending' | 'all'
export type SuggestedKind = 'all' | 'book' | 'comic' | 'manga'
export type ReviewAttention = 'all' | 'simple' | 'review'
export type ReviewFilters = { status: ReviewStatus; kind: SuggestedKind; attention: ReviewAttention }

export function fetchClassificationReview(page: number, filters: ReviewFilters, pageSize = 25) {
  return apiRoute('/api/admin/books/classification-review', '/api/admin/books/classification-review', {
    query: { page, pageSize, status: filters.status, kind: filters.kind, attention: filters.attention },
  })
}

export function saveClassificationReview(decisions: ClassificationDecision[]) {
  return apiRoute('/api/admin/books/classification-review', '/api/admin/books/classification-review', {
    method: 'POST', json: { decisions },
  })
}

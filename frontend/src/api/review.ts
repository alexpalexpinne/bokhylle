import type { components } from './generated'
import { ApiError } from './client'
import { apiRoute } from './client'

export type ReviewCandidate = components['schemas']['ReviewImportCandidate']

export type ReviewDetail = components['schemas']['ReviewDetail']

export function fetchReview(id: string) {
  return apiRoute('/api/acquisitions/{id}/review', `/api/acquisitions/${id}/review`)
}

export function resolveReview(
  id: string,
  action: 'choose' | 'retry' | 'ignore',
  path?: string,
) {
  if (action === 'choose' && path === undefined) {
    throw new Error('Choosing a review candidate requires its path')
  }
  return apiRoute('/api/acquisitions/{id}/review/resolve', `/api/acquisitions/${id}/review/resolve`, {
    method: 'POST',
    json: action === 'choose' ? { action, path: path! } : { action },
  })
}

export function basename(path: string): string {
  return (
    path
      .split(/[\\/]/)
      .filter(Boolean)
      .pop() ?? path
  )
}

export function errorMessage(error: unknown, fallback: string): string {
  return error instanceof ApiError ? error.message : fallback
}

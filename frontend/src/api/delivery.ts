import type { components } from './generated'
import { apiRoute } from './client'

export type DeliveryTarget = components['schemas']['DeliveryTarget']

export type Delivery = components['schemas']['Delivery']

export type DefaultReader = components['schemas']['DefaultTarget']

export function fetchDefaultReader() {
  return apiRoute('/api/delivery-targets/default', '/api/delivery-targets/default')
}

export function fetchTargets() {
  return apiRoute('/api/delivery-targets', '/api/delivery-targets')
}

export function createTarget(
  address: string,
  deviceType: string = 'kindle',
  name?: string,
) {
  return apiRoute('/api/delivery-targets', '/api/delivery-targets', {
    method: 'POST',
    json: {
      address,
      connector: 'email',
      deviceType,
      ...(name ? { name } : {}),
    },
  })
}

export function updateTarget(
  id: number,
  update: { name?: string; address?: string; enabled?: boolean },
) {
  return apiRoute('/api/delivery-targets/{id}', `/api/delivery-targets/${id}`, {
    method: 'PUT',
    json: update,
  })
}

export function setTargetDefault(id: number) {
  return apiRoute('/api/delivery-targets/{id}/default', `/api/delivery-targets/${id}/default`, { method: 'POST' })
}

export function deleteTarget(id: number) {
  return apiRoute('/api/delivery-targets/{id}', `/api/delivery-targets/${id}`, { method: 'DELETE' })
}

export function deliverBook(
  bookId: number,
  fileId: number,
  targetId?: number,
) {
  return apiRoute('/api/books/{book_id}/files/{file_id}/deliver', `/api/books/${bookId}/files/${fileId}/deliver`, {
    method: 'POST',
    json: targetId ? { targetId } : {},
  })
}

export function fetchDeliveries(bookId: number) {
  return apiRoute('/api/deliveries', '/api/deliveries', { query: { bookId } })
}

export function retryDelivery(id: number) {
  return apiRoute('/api/deliveries/{id}/retry', `/api/deliveries/${id}/retry`, { method: 'POST' })
}

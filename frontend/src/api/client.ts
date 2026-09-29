import { notifySessionExpired } from '../auth/session'
import type { components, paths } from './generated'

export class ApiError extends Error {
  readonly code: string
  readonly status: number

  constructor(message: string, code: string, status: number) {
    super(message)
    this.name = 'ApiError'
    this.code = code
    this.status = status
  }
}

type ErrorEnvelope = Partial<components['schemas']['ErrorBody']>

type HttpMethod = 'GET' | 'POST' | 'PUT' | 'DELETE' | 'PATCH'
type RouteMethod<P extends keyof paths> = Uppercase<Extract<keyof paths[P], Lowercase<HttpMethod>>>
type Operation<P extends keyof paths, M extends RouteMethod<P>> =
  NonNullable<paths[P][Lowercase<M> & keyof paths[P]]>
type JsonBody<O> = O extends { requestBody: { content: { 'application/json': infer B } } }
  ? B
  : never
type Query<O> = O extends { parameters: { query?: infer Q } } ? NonNullable<Q> : never
type SuccessResponse<O> = O extends { responses: infer R }
  ? R[Extract<keyof R, 200 | 201 | 202 | 203 | 204 | 206>]
  : never
type JsonResult<R> = R extends { content: { 'application/json': infer T } } ? T : void
type RoutePath<P extends string> = P extends `${infer Head}{${string}}${infer Tail}`
  ? `${Head}${string}${RoutePath<Tail>}`
  : P
type RouteOptions<O, M extends HttpMethod> = Omit<RequestInit, 'body' | 'method'> & {
  method?: M
} & ([JsonBody<O>] extends [never] ? { json?: never } : { json: JsonBody<O> }) &
  ([Query<O>] extends [never]
    ? { query?: never }
    : {} extends Query<O>
      ? { query?: Query<O> }
      : { query: Query<O> })
type NeedsOptions<O> = [JsonBody<O>] extends [never]
  ? [Query<O>] extends [never]
    ? false
    : {} extends Query<O>
      ? false
      : true
  : true

/// Auth endpoints answer 401 for ordinary reasons (wrong password, signed out,
/// no session yet); those must not be treated as an expired session or the
/// login flow would loop.
function isAuthEndpoint(path: string): boolean {
  return (
    path === '/api/auth/me' ||
    path === '/api/auth/login' ||
    path === '/api/auth/logout' ||
    path === '/api/auth/logout-all' ||
    path.startsWith('/api/auth/users')
  )
}

async function api<T>(path: string, options: RequestInit = {}): Promise<T> {
  const headers = new Headers(options.headers)
  if (options.body !== undefined && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json')
  }

  const response = await fetch(path, { credentials: 'include', ...options, headers })

  if (response.status === 204) {
    return undefined as T
  }

  const payload: unknown = await response.json().catch(() => null)

  if (!response.ok) {
    if (response.status === 401 && !isAuthEndpoint(path)) {
      notifySessionExpired()
    }
    const envelope = (payload ?? {}) as ErrorEnvelope
    throw new ApiError(
      envelope.message ?? response.statusText,
      envelope.code ?? 'unknown',
      response.status,
    )
  }

  return payload as T
}

export function uploadBinary(path: '/api/profile/avatar', file: File): Promise<void> {
  return api<void>(path, {
    method: 'PUT',
    headers: { 'Content-Type': file.type },
    body: file,
  })
}

/** Bind a frontend request to the generated operation and its JSON contract. */
export function apiRoute<
  P extends keyof paths,
  M extends RouteMethod<P> = Extract<'GET', RouteMethod<P>>,
>(
  route: P,
  url: RoutePath<P>,
  ...[options]: NeedsOptions<Operation<P, M>> extends true
    ? [options: RouteOptions<Operation<P, M>, M>]
    : [options?: RouteOptions<Operation<P, M>, M>]
): Promise<JsonResult<SuccessResponse<Operation<P, M>>>> {
  // The route is a compile-time contract; url contains the encoded path values.
  void route
  const { json, query, ...requestOptions } = options ?? {}
  const search = new URLSearchParams()
  for (const [key, value] of Object.entries(query ?? {})) {
    if (value !== undefined && value !== null) search.set(key, String(value))
  }
  const target = search.size ? `${url}?${search.toString()}` : url
  return api<JsonResult<SuccessResponse<Operation<P, M>>>>(target, {
    ...requestOptions,
    ...(json === undefined ? {} : { body: JSON.stringify(json) }),
  })
}

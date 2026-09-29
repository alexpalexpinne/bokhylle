// A single session-expiry signal: the API client notifies, the auth provider
// reacts. Kept tiny so any 401 from an authenticated endpoint invalidates the
// frontend session without page-by-page handlers.

type Listener = () => void

const listeners = new Set<Listener>()

export function onSessionExpired(listener: Listener): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

export function notifySessionExpired(): void {
  for (const listener of listeners) {
    listener()
  }
}

import { useCallback, useState } from 'react'
import { ApiError } from '../api/client'

/// One small pattern for user-visible mutations: busy while in flight, a
/// readable error on failure, local state updated only on success.
export function useMutation() {
  const [busyKey, setBusyKey] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  const run = useCallback(
    async <T>(
      key: string,
      action: () => Promise<T>,
      fallback: string,
      onSuccess?: (result: T) => void,
    ): Promise<boolean> => {
      setBusyKey(key)
      setError(null)
      try {
        const result = await action()
        onSuccess?.(result)
        return true
      } catch (caught) {
        setError(caught instanceof ApiError ? caught.message : fallback)
        return false
      } finally {
        setBusyKey(null)
      }
    },
    [],
  )

  return { busyKey, error, run, clearError: () => setError(null) }
}

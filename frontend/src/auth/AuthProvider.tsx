import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react'
import { ApiError, apiRoute } from '../api/client'
import { AuthContext, type User } from './context'
import { onSessionExpired } from './session'

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(null)
  const [loading, setLoading] = useState(true)
  const [demo, setDemo] = useState<boolean | null>(null)

  const refresh = useCallback(async () => {
    try {
      const data = await apiRoute('/api/auth/me', '/api/auth/me')
      setUser(data.user)
    } catch (error) {
      if (!(error instanceof ApiError) || error.status !== 401) {
        console.error(error)
      }
      setUser(null)
    }
  }, [])

  // A 401 from any authenticated endpoint (revoked/expired session) drops
  // the user; RequireAuth then sends them to the login flow.
  useEffect(() => onSessionExpired(() => setUser(null)), [])

  useEffect(() => {
    let cancelled = false

    apiRoute('/api/demo', '/api/demo')
      .then((data) => { if (!cancelled) setDemo(data.enabled) })
      .catch(() => { if (!cancelled) setDemo(false) })

    apiRoute('/api/auth/me', '/api/auth/me')
      .then((data) => {
        if (!cancelled) {
          setUser(data.user)
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          if (!(error instanceof ApiError) || error.status !== 401) {
            console.error(error)
          }
          setUser(null)
        }
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false)
        }
      })

    return () => {
      cancelled = true
    }
  }, [])

  const login = useCallback(
    async (username: string, password: string, remember = true) => {
      const data = await apiRoute('/api/auth/login', '/api/auth/login', {
        method: 'POST',
        json: { username, password, remember },
      })
      setUser(data.user)
    },
    [],
  )

  const logout = useCallback(async () => {
    await apiRoute('/api/auth/logout', '/api/auth/logout', { method: 'POST' })
    setUser(null)
  }, [])

  const logoutAll = useCallback(async () => {
    await apiRoute('/api/auth/logout-all', '/api/auth/logout-all', { method: 'POST' })
    setUser(null)
  }, [])

  const enterDemo = useCallback(async (profile: 'adult' | 'child') => {
    const data = await apiRoute('/api/demo/enter', '/api/demo/enter', {
      method: 'POST',
      json: { profile },
    })
    setUser(data.user)
  }, [])

  const switchDemo = useCallback(async () => {
    const data = await apiRoute('/api/demo/switch', '/api/demo/switch', { method: 'POST' })
    setUser(data.user)
  }, [])

  const value = useMemo(
    () => ({ user, loading, demo, login, logout, logoutAll, refresh, enterDemo, switchDemo }),
    [user, loading, demo, login, logout, logoutAll, refresh, enterDemo, switchDemo],
  )

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

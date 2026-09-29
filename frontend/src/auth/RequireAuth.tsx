import { Navigate, Outlet, useLocation } from 'react-router-dom'
import { useAuth } from './useAuth'

function Loading() {
  return (
    <div className="flex min-h-screen items-center justify-center text-sm text-ink-muted">
      Loading…
    </div>
  )
}

export function RequireAuth() {
  const { user, loading } = useAuth()
  const location = useLocation()

  if (loading) {
    return <Loading />
  }

  if (!user) {
    return <Navigate to="/login" replace state={{ from: location }} />
  }

  return <Outlet />
}

/// Children are curated by a parent; discovery, authors, activity and reader
/// management are adult surfaces even though the backend refuses the API too.
export function RequireAdult() {
  const { user, loading } = useAuth()

  if (loading) {
    return <Loading />
  }

  if (user?.profileType === 'child') {
    return <Navigate to="/library" replace />
  }

  return <Outlet />
}

export function RequireDiscover() {
  const { user, loading } = useAuth()

  if (loading) return <Loading />
  if (user?.profileType === 'child' && !user.canDiscover) {
    return <Navigate to="/library" replace />
  }
  return <Outlet />
}

export function RequireAdmin() {
  const { user, loading } = useAuth()

  if (loading) {
    return <Loading />
  }

  if (user?.role !== 'admin') {
    return <Navigate to="/" replace />
  }

  return <Outlet />
}

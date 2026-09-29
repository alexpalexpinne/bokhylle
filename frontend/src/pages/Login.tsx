import { useEffect, useRef, useState, type FormEvent } from 'react'
import { Navigate, useLocation, useNavigate } from 'react-router-dom'
import { ApiError } from '../api/client'
import { type AuthMode, type LoginUser, fetchLoginUsers } from '../api/auth'
import { BrandLockup } from '../components/BrandLockup'
import { Button } from '../components/ui/Button'
import { useAuth } from '../auth/useAuth'

export function Login() {
  const { user, loading, login, demo, enterDemo } = useAuth()
  const navigate = useNavigate()
  const location = useLocation()

  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [submitting, setSubmitting] = useState(false)
  const [users, setUsers] = useState<LoginUser[]>([])
  const [picking, setPicking] = useState(true)
  const [mode, setMode] = useState<AuthMode>('legacy')
  const [remember, setRemember] = useState(true)
  const passwordRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (demo !== false) return
    fetchLoginUsers()
      .then((data) => {
        setUsers(data.users)
        setPicking(data.users.length > 0)
      })
      .catch(() => {
        setUsers([])
        setPicking(false)
      })
  }, [demo])

  useEffect(() => {
    if (!picking) {
      passwordRef.current?.focus()
    }
  }, [picking])

  function choose(user: LoginUser) {
    setUsername(user.username)
    setMode(user.authMode === 'pin' || user.authMode === 'password' ? user.authMode : 'legacy')
    setPassword('')
    setError(null)
    setPicking(false)
  }

  if (!loading && user) {
    return <Navigate to="/" replace />
  }

  if (demo === null) {
    return <main className="flex min-h-screen items-center justify-center text-sm text-ink-muted" role="status">Opening Bokhylle…</main>
  }

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setSubmitting(true)
    setError(null)

    try {
      await login(username, password, remember)
      const state = location.state as { from?: { pathname?: string } } | null
      navigate(state?.from?.pathname ?? '/', { replace: true })
    } catch (caught) {
      const fallback =
        mode === 'pin'
          ? 'Incorrect PIN'
          : mode === 'password'
            ? 'Incorrect password'
            : 'Incorrect credential'
      setError(caught instanceof ApiError && caught.status !== 401 ? caught.message : fallback)
    } finally {
      setSubmitting(false)
    }
  }

  async function enter(profile: 'adult' | 'child') {
    setSubmitting(true)
    setError(null)
    try {
      await enterDemo(profile)
      navigate('/', { replace: true })
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not enter the demo')
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <main className="flex min-h-screen flex-col items-center justify-center px-6 py-12">
      <div className="w-full max-w-sm">
        <div className="flex flex-col items-center text-center">
          <BrandLockup large stacked />
          <p className="mt-5 font-sans text-[11px] font-medium uppercase tracking-[0.24em] text-ink-muted">
            {demo ? 'Public demo' : 'Private library'}
          </p>
          <h1 className="mt-3 font-display text-display text-ink">Come in and browse</h1>
          <p className="mt-3 max-w-sm text-sm text-ink-muted">
            Find a book, keep it on your shelf, then download it or send it to your reader.
          </p>
        </div>

        {demo ? (
          <div className="mt-9 border-t border-line pt-5">
            <p className="mb-4 text-sm text-ink-muted">Explore a sample household library. Enter as an adult to see a child book request in Notifications. Changes may reset at any time.</p>
            <div className="grid gap-3 sm:grid-cols-2">
              <Button variant="primary" disabled={submitting} onClick={() => void enter('adult')}>Enter as adult</Button>
              <Button variant="secondary" disabled={submitting} onClick={() => void enter('child')}>Enter as child</Button>
            </div>
            {error && <p role="alert" className="mt-4 text-sm text-danger">{error}</p>}
          </div>
        ) : picking ? (
          <div className="mt-9">
            <p className="font-sans text-[11px] font-medium uppercase tracking-[0.2em] text-ink-faint">
              Who is reading?
            </p>
            <div className="mt-3 divide-y divide-line">
              {users.map((user) => (
                <button
                  key={user.username}
                  type="button"
                  onClick={() => choose(user)}
                  className="group flex w-full items-center gap-4 py-3.5 text-left"
                >
                  <span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-[3px] bg-surface-2 font-display text-base text-ink">
                    {(user.displayName ?? user.username).slice(0, 1).toUpperCase()}
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate font-display text-base text-ink transition-colors group-hover:text-accent">
                      {user.displayName ?? user.username}
                    </span>
                    <span className="mt-0.5 block font-sans text-[10px] uppercase tracking-[0.16em] text-ink-faint">
                      {user.role === 'admin'
                        ? 'Administrator'
                        : user.profileType === 'child'
                          ? 'Child profile'
                          : 'Household member'}
                    </span>
                  </span>
                  <span className="text-ink-faint transition-colors group-hover:text-accent">
                    &#8594;
                  </span>
                </button>
              ))}
            </div>
            <button
              type="button"
              onClick={() => {
                setUsername('')
                setPassword('')
                setMode('legacy')
                setPicking(false)
              }}
              className="mt-5 text-xs text-ink-muted transition-colors hover:text-ink"
            >
              Sign in with a username instead
            </button>
          </div>
        ) : (
        <form onSubmit={(event) => void handleSubmit(event)} className="mt-9">
          <label className="mb-5 block">
            <span className="mb-1.5 block font-sans text-[11px] uppercase tracking-[0.16em] text-ink-faint">
              Username
            </span>
            <input
              value={username}
              onChange={(event) => setUsername(event.target.value)}
              autoComplete="username"
              required
              className="w-full border-b border-line bg-transparent pb-2 font-display text-lg text-ink outline-none placeholder:text-ink-faint focus-visible:border-accent focus-visible:outline-none"
            />
          </label>

          <label className="mb-5 block">
            <span className="mb-1.5 block font-sans text-[11px] uppercase tracking-[0.16em] text-ink-faint">
              {mode === 'pin' ? 'PIN' : mode === 'password' ? 'Password' : 'PIN or password'}
            </span>
            <input
              ref={passwordRef}
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              autoComplete="current-password"
              required
              inputMode={mode === 'pin' ? 'numeric' : undefined}
              maxLength={mode === 'pin' ? 6 : undefined}
              className="w-full border-b border-line bg-transparent pb-2 font-display text-lg text-ink outline-none placeholder:text-ink-faint focus-visible:border-accent focus-visible:outline-none"
            />
          </label>

          <label className="mb-5 flex items-center gap-2">
            <input
              type="checkbox"
              checked={remember}
              onChange={(event) => setRemember(event.target.checked)}
              className="h-4 w-4 accent-[var(--color-accent)]"
            />
            <span className="text-xs text-ink-muted">Remember this device</span>
          </label>

          {error && <p className="mb-4 border-l-2 border-danger pl-3 text-sm text-danger">{error}</p>}

          <Button type="submit" variant="primary" size="lg" className="w-full" disabled={submitting}>
            {submitting ? 'Signing in…' : 'Sign in'}
          </Button>

          {users.length > 0 && (
            <button
              type="button"
              onClick={() => {
                setPicking(true)
                setPassword('')
              }}
              className="mt-4 block text-xs text-ink-muted transition-colors hover:text-ink"
            >
              Choose another account
            </button>
          )}
        </form>
        )}

        {!demo && <p className="mt-6 text-center text-xs text-ink-faint">Private by design — your library stays yours.</p>}
      </div>
    </main>
  )
}

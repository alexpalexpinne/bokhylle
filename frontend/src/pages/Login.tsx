import { useEffect, useRef, useState, type FormEvent } from 'react'
import { Navigate, useLocation, useNavigate } from 'react-router-dom'
import { ApiError } from '../api/client'
import { type AuthMode, type LoginUser, fetchLoginUsers } from '../api/auth'
import { BrandLockup } from '../components/BrandLockup'
import { ProfileAvatar } from '../components/ProfileAvatar'
import { Button } from '../components/ui/Button'
import { useAuth } from '../auth/useAuth'
import { loginMotion, useLoginMotion } from './login/useLoginMotion'

export function Login() {
  const { user, loading, login, demo, enterDemo } = useAuth()
  const navigate = useNavigate()
  const location = useLocation()
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [submitting, setSubmitting] = useState(false)
  const [users, setUsers] = useState<LoginUser[]>([])
  const [profilesLoading, setProfilesLoading] = useState(true)
  const [selected, setSelected] = useState<LoginUser | null>(null)
  const [manual, setManual] = useState(false)
  const [remember, setRemember] = useState(true)
  const usernameRef = useRef<HTMLInputElement>(null)
  const passwordRef = useRef<HTMLInputElement>(null)
  const selectedButtonRef = useRef<HTMLButtonElement | null>(null)
  const profilesRef = useRef<HTMLDivElement>(null)
  const mainRef = useRef<HTMLElement>(null)
  const contentRef = useRef<HTMLDivElement>(null)
  const panelRef = useRef<HTMLDivElement>(null)
  const controlsRef = useRef<HTMLDivElement>(null)
  const formOpen = selected !== null || manual
  const motion = useLoginMotion({
    formOpen, mainRef, contentRef, profilesRef, panelRef, controlsRef, usernameRef, passwordRef,
  })
  const panelExpanded = formOpen && (manual || motion.stage === 'opening' || motion.stage === 'ready')
  const mode: AuthMode = selected?.authMode === 'pin' ? 'pin' : selected?.authMode === 'password' ? 'password' : 'legacy'
  const credentialLabel = mode === 'pin' ? 'PIN' : mode === 'password' ? 'Password' : 'PIN or password'

  useEffect(() => {
    if (demo !== false) return
    let active = true
    fetchLoginUsers()
      .then((data) => {
        if (!active) return
        setUsers(data.users)
        setManual(data.users.length === 0)
      })
      .catch(() => {
        if (active) setManual(true)
      })
      .finally(() => {
        if (active) setProfilesLoading(false)
      })
    return () => { active = false }
  }, [demo])

  useEffect(() => {
    if (manual && users.length === 0) usernameRef.current?.focus()
  }, [manual, users.length])

  function choose(profile: LoginUser, button: HTMLButtonElement) {
    if (selected === profile) {
      passwordRef.current?.focus({ preventScroll: true })
      motion.alignCredential()
      return
    }
    selectedButtonRef.current = button
    motion.selectProfile(button, () => {
      setSelected(profile)
      setManual(false)
      setUsername(profile.username)
      setPassword('')
      setError(null)
    })
  }

  function openManualForm() {
    motion.showUsernameForm(() => {
      setSelected(null)
      setManual(true)
      setUsername('')
      setPassword('')
      setError(null)
    })
  }

  function closeForm() {
    motion.close(() => {
      setSelected(null)
      setManual(false)
      setUsername('')
      setPassword('')
      setError(null)
    })
    const profileButton = selectedButtonRef.current ?? profilesRef.current?.querySelector('button')
    profileButton?.focus({ preventScroll: true })
  }

  if (!loading && user) return <Navigate to="/" replace />
  if (demo === null) return <main className="flex min-h-screen items-center justify-center text-sm text-ink-muted" role="status">Opening Bokhylle…</main>

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (submitting) return
    setSubmitting(true)
    setError(null)
    try {
      await login(username, password, remember)
      const state = location.state as { from?: { pathname?: string } } | null
      navigate(state?.from?.pathname ?? '/', { replace: true })
    } catch (caught) {
      const fallback = mode === 'pin' ? 'Incorrect PIN' : mode === 'password' ? 'Incorrect password' : 'Incorrect PIN or password'
      setError(caught instanceof ApiError && caught.status !== 401 ? caught.message : fallback)
      passwordRef.current?.focus({ preventScroll: true })
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
    <main ref={mainRef} className="flex min-h-dvh flex-col items-center justify-center px-6 py-12" style={motion.layoutStyle}>
      <div ref={contentRef} className={`w-full ${demo ? 'max-w-sm' : 'max-w-xl'}`}>
        <div className="flex flex-col items-center text-center">
          <BrandLockup large stacked />
          {demo && <p className="mt-5 font-sans text-[11px] font-medium uppercase tracking-[0.24em] text-ink-muted">Public demo</p>}
          <h1 className="mt-8 font-display text-hero text-ink">{demo ? 'Come in and browse' : formOpen ? 'Sign in' : 'Who’s reading?'}</h1>
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
        ) : (
          <>
            {profilesLoading ? (
              <p role="status" className="mt-8 text-center text-sm text-ink-muted">Loading profiles…</p>
            ) : users.length > 0 && (
              <div
                ref={profilesRef}
                role="group"
                aria-label={selected ? 'Selected profile' : 'Household profiles'}
                hidden={manual}
                className={manual ? 'hidden' : 'mt-8 flex flex-wrap justify-center gap-x-6 gap-y-5 transition-[height] ease-out'}
                style={{ height: selected ? motion.profileHeight : undefined, transitionDuration: `${loginMotion.slideMs}ms` }}
              >
                {users.map((profile) => (
                  <button
                    key={profile.username}
                    type="button"
                    hidden={selected !== null && selected !== profile && motion.stage !== 'fading'}
                    aria-hidden={selected !== null && selected !== profile ? true : undefined}
                    inert={selected !== null && selected !== profile}
                    data-profile-username={profile.username}
                    aria-pressed={selected === profile}
                    aria-expanded={selected === profile}
                    aria-controls="sign-in-panel"
                    disabled={submitting}
                    onClick={(event) => choose(profile, event.currentTarget)}
                    className={selected !== null && selected !== profile && motion.stage !== 'fading' ? 'hidden' : 'group flex w-24 flex-col items-center gap-3 rounded-[3px] py-1 text-center disabled:opacity-60 sm:w-28'}
                  >
                    <ProfileAvatar
                      user={profile}
                      src={profile.avatarUrl}
                      className={`h-20 w-20 text-3xl ring-2 ring-offset-4 ring-offset-canvas transition-[box-shadow] ${selected === profile ? 'ring-accent' : 'ring-transparent group-hover:ring-line'}`}
                    />
                    <span className={`w-full break-words font-display text-lg leading-snug transition-colors ${selected === profile ? 'text-accent' : 'text-ink group-hover:text-accent'}`}>
                      {profile.displayName || profile.username}
                    </span>
                  </button>
                ))}
              </div>
            )}
            <div
              ref={panelRef}
              id="sign-in-panel"
              aria-hidden={!formOpen}
              aria-busy={motion.transitioning}
              inert={!formOpen}
              className={`grid transition-[grid-template-rows,opacity] ease-in-out ${panelExpanded ? 'grid-rows-[1fr] opacity-100' : 'grid-rows-[0fr] opacity-0'}`}
              style={{ transitionDuration: `${loginMotion.expandMs}ms` }}
            >
              <div className="min-h-0 overflow-hidden">
                <form onSubmit={(event) => void handleSubmit(event)} className="mx-auto mt-8 max-w-xs border-t border-line pt-6" aria-labelledby="sign-in-heading">
                  <h2 id="sign-in-heading" className={selected ? 'sr-only' : 'mb-5 text-center text-sm text-ink-muted'}>
                    {selected ? `Sign in as ${selected.displayName || selected.username}` : 'Sign in with your username'}
                  </h2>
                  <div ref={controlsRef}>
                    {manual ? (
                      <label className="mb-5 block">
                        <span className="mb-1.5 block text-xs text-ink-muted">Username</span>
                        <input
                          ref={usernameRef}
                          name="username"
                          value={username}
                          onChange={(event) => setUsername(event.target.value)}
                          autoComplete="username"
                          enterKeyHint="next"
                          required
                          disabled={submitting}
                          className="h-11 w-full border-b border-line bg-transparent font-display text-lg text-ink outline-none focus-visible:border-accent focus-visible:outline-none"
                        />
                      </label>
                    ) : <input type="hidden" name="username" autoComplete="username" value={username} />}
                    <label className="mb-5 block">
                      <span className="mb-1.5 block text-xs text-ink-muted">{credentialLabel}</span>
                      <input
                        ref={passwordRef}
                        name="password"
                        type="password"
                        value={password}
                        onChange={(event) => setPassword(mode === 'pin' ? event.target.value.replace(/\D/g, '').slice(0, 6) : event.target.value)}
                        autoComplete="current-password"
                        enterKeyHint="go"
                        required
                        readOnly={submitting}
                        inputMode={mode === 'pin' ? 'numeric' : undefined}
                        minLength={mode === 'pin' ? 6 : undefined}
                        pattern={mode === 'pin' ? '[0-9]{6}' : undefined}
                        aria-invalid={error ? true : undefined}
                        aria-describedby={error ? 'sign-in-error' : undefined}
                        className="h-11 w-full border-b border-line bg-transparent font-display text-lg text-ink outline-none focus-visible:border-accent focus-visible:outline-none"
                      />
                    </label>
                    <label className="mb-5 flex min-h-6 items-center gap-2">
                      <input type="checkbox" checked={remember} disabled={submitting} onChange={(event) => setRemember(event.target.checked)} className="h-4 w-4 accent-[var(--color-accent)]" />
                      <span className="text-xs text-ink-muted">Remember this device</span>
                    </label>
                    {error && <p id="sign-in-error" role="alert" className="mb-4 border-l-2 border-danger pl-3 text-sm text-danger">{error}</p>}
                    <Button type="submit" variant="primary" size="lg" className="w-full" disabled={submitting}>
                      {submitting ? 'Signing in…' : 'Sign in'}
                    </Button>
                  </div>
                  {users.length > 0 && <button type="button" disabled={submitting} onClick={closeForm} className="mx-auto mt-3 block min-h-11 text-xs text-ink-muted hover:text-ink">Back to profiles</button>}
                </form>
              </div>
            </div>
            {!profilesLoading && users.length > 0 && !manual && (
              <button
                type="button"
                disabled={submitting}
                onClick={openManualForm}
                className="mx-auto mt-6 block min-h-11 text-xs text-ink-muted hover:text-ink"
              >Sign in another way</button>
            )}
          </>
        )}
      </div>
    </main>
  )
}

import { useEffect, useRef, useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { ChevronDown, CircleHelp, LogOut, Monitor, Moon, RefreshCw, Settings, Sun, UserRound } from 'lucide-react'
import { ApiError } from '../api/client'
import { useAuth } from '../auth/useAuth'
import { type ThemeChoice, useThemeChoice } from '../theme'
import { ProfileAvatar } from './ProfileAvatar'

export function AccountMenu() {
  const { user, logout, demo, switchDemo } = useAuth()
  const navigate = useNavigate()
  const [open, setOpen] = useState(false)
  const [busy, setBusy] = useState<'switch' | 'signout' | null>(null)
  const [error, setError] = useState<string | null>(null)
  const containerRef = useRef<HTMLDivElement>(null)
  const { choice, choose } = useThemeChoice()

  useEffect(() => {
    if (!open) {
      return
    }

    function onPointerDown(event: PointerEvent) {
      if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
        setOpen(false)
      }
    }

    function onKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        setOpen(false)
      }
    }

    document.addEventListener('pointerdown', onPointerDown)
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('pointerdown', onPointerDown)
      document.removeEventListener('keydown', onKeyDown)
    }
  }, [open])

  if (!user) {
    return null
  }

  const isAdmin = user.role === 'admin'
  const isChild = user.profileType === 'child'

  // A failed logout leaves the server session alive: keep the current profile
  // and show the failure instead of pretending the sign-out succeeded.
  async function leave(mode: 'switch' | 'signout') {
    setBusy(mode)
    setError(null)
    try {
      await logout()
      if (mode === 'switch') {
        navigate('/login')
      } else {
        setOpen(false)
      }
    } catch (caught) {
      setError(
        caught instanceof ApiError
          ? caught.message
          : 'Could not sign out. The session may still be active — try again.',
      )
    } finally {
      setBusy(null)
    }
  }

  async function switchProfile() {
    setBusy('switch')
    setError(null)
    try {
      await switchDemo()
      setOpen(false)
      navigate('/')
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not switch profile')
    } finally {
      setBusy(null)
    }
  }

  return (
    <div ref={containerRef} className="relative">
      <button
        type="button"
        onClick={() => setOpen((current) => !current)}
        aria-expanded={open}
        aria-label="Account menu"
        className="flex items-center gap-1.5 rounded-full p-1 pr-2 text-ink-soft transition-colors hover:bg-surface-2 hover:text-ink"
      >
        <ProfileAvatar user={user} className="h-8 w-8 text-xs font-semibold" />
        <ChevronDown size={14} className={open ? 'rotate-180 transition-transform' : 'transition-transform'} />
      </button>

      {open && (
        <div
          className="absolute right-0 top-full z-50 mt-2 w-56 overflow-hidden rounded-panel bg-surface py-1.5 shadow-modal"
        >
          <div className="border-b border-line px-4 py-3">
            <p className="text-sm font-medium text-ink">
              {user.displayName ?? user.username}
            </p>
            <p className="text-xs text-ink-faint">
              {user.displayName ? `@${user.username} · ` : ''}
              {isAdmin
                ? 'Administrator'
                : user.profileType === 'child'
                  ? 'Child profile'
                  : 'Household member'}
            </p>
          </div>

          {!isChild && <div className="border-b border-line px-4 py-3">
            <p className="text-[11px] uppercase tracking-[0.18em] text-ink-faint">Theme</p>
            <div className="mt-2 flex items-center gap-1">
              {(
                [
                  { value: 'system', label: 'System', icon: Monitor },
                  { value: 'paper', label: 'Paper', icon: Sun },
                  { value: 'ink', label: 'Ink', icon: Moon },
                ] as { value: ThemeChoice; label: string; icon: typeof Sun }[]
              ).map((option) => (
                <button
                  key={option.value}
                  type="button"
                  aria-pressed={choice === option.value}
                  onClick={() => choose(option.value)}
                  className={`flex flex-1 items-center justify-center gap-1.5 rounded-[3px] px-2 py-1.5 text-xs transition-colors ${
                    choice === option.value
                      ? 'bg-accent text-accent-ink'
                      : 'text-ink-muted hover:bg-surface-2 hover:text-ink'
                  }`}
                >
                  <option.icon size={12} aria-hidden />
                  {option.label}
                </button>
              ))}
            </div>
          </div>}

          {(!demo || isChild) && (
            <Link
              to="/profile"
              onClick={() => setOpen(false)}
              className="flex items-center gap-2.5 px-4 py-2.5 text-sm text-ink-soft transition-colors hover:bg-surface-2 hover:text-ink"
            >
              <UserRound size={15} />
              {isChild ? 'My settings' : 'Profile'}
            </Link>
          )}

          {isAdmin && (
            <Link
              to="/settings"
              onClick={() => setOpen(false)}
              className="flex items-center gap-2.5 px-4 py-2.5 text-sm text-ink-soft transition-colors hover:bg-surface-2 hover:text-ink"
            >
              <Settings size={15} />
              Administration
            </Link>
          )}

          <Link
            to="/help"
            onClick={() => setOpen(false)}
            className="flex items-center gap-2.5 px-4 py-2.5 text-sm text-ink-soft transition-colors hover:bg-surface-2 hover:text-ink"
          >
            <CircleHelp size={15} />
            Help
          </Link>

          <button
            type="button"
            disabled={busy !== null}
            onClick={() => void (demo ? switchProfile() : leave('switch'))}
            className="flex w-full items-center gap-2.5 px-4 py-2.5 text-left text-sm text-ink-soft transition-colors hover:bg-surface-2 hover:text-ink disabled:opacity-50"
          >
            <RefreshCw size={15} />
            {busy === 'switch' ? 'Switching…' : demo ? 'Switch adult / child' : 'Switch profile'}
          </button>

          <button
            type="button"
            disabled={busy !== null}
            onClick={() => void leave('signout')}
            className="flex w-full items-center gap-2.5 px-4 py-2.5 text-left text-sm text-ink-soft transition-colors hover:bg-surface-2 hover:text-ink disabled:opacity-50"
          >
            <LogOut size={15} />
            {busy === 'signout' ? 'Signing out…' : 'Sign out'}
          </button>

          {error && (
            <p role="alert" className="border-t border-line px-4 py-2.5 text-xs text-danger">
              {error}
            </p>
          )}
        </div>
      )}
    </div>
  )
}

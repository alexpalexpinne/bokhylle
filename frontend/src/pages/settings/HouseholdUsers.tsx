import { useEffect, useState } from 'react'
import { Pencil, Plus } from 'lucide-react'
import { Link } from 'react-router-dom'
import { ApiError } from '../../api/client'
import {
  type AdminUser,
  createUser,
  fetchUserProfiles,
  fetchUsers,
  restartOnboarding,
  setUserProfileType,
  updateUser,
} from '../../api/users'
import { LANGUAGES, languageLabel } from '../../lib/languages'
import { Button } from '../../components/ui/Button'
import { Modal } from '../../components/ui/Modal'

type UserEditor = {
  mode: 'create' | 'edit'
  preferredLanguages: string[]
  id?: number
  username: string
  displayName: string
  credential: string
  credentialType: 'pin' | 'password'
  role: 'admin' | 'user'
  profileType: 'adult' | 'child'
  disabled: boolean
  canRequest: boolean
  canDiscover: boolean
  canAcquire: boolean
}

export function HouseholdUsers() {
  const [users, setUsers] = useState<AdminUser[]>([])
  const [profiles, setProfiles] = useState<Record<number, string>>({})
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [editor, setEditor] = useState<UserEditor | null>(null)
  const [saving, setSaving] = useState(false)
  const [convertArmed, setConvertArmed] = useState(false)

  function load() {
    fetchUsers()
      .then((items) => {
        setUsers(items)
        setError(null)
      })
      .catch((caught: unknown) => {
        setError(caught instanceof ApiError ? caught.message : 'Could not load users')
      })
  }

  function loadProfiles() {
    return fetchUserProfiles()
      .then((data) =>
        setProfiles(
          Object.fromEntries(data.users.map((entry) => [entry.userId, entry.profileType])),
        ),
      )
      .catch((caught: unknown) => console.warn('settings.user_profiles.load_failed', caught))
  }

  useEffect(() => {
    load()
    void loadProfiles()
  }, [])

  async function restartSetup(userId: number) {
    try {
      await restartOnboarding(userId)
      setNotice('Setup will run again the next time they sign in.')
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not restart setup')
    }
  }

  const currentProfileType =
    editor?.mode === 'edit' && editor.id !== undefined
      ? (profiles[editor.id] ?? 'adult')
      : null
  const converting = currentProfileType !== null && editor?.profileType !== currentProfileType

  async function save() {
    if (!editor) {
      return
    }
    setSaving(true)
    setError(null)

    try {
      if (editor.mode === 'create') {
        await createUser({
          username: editor.username.trim(),
          credential: editor.credential,
          credentialType: editor.credentialType,
          role: editor.role,
          profileType: editor.profileType,
          displayName: editor.displayName.trim() || undefined,
          preferredLanguages: editor.preferredLanguages,
          canRequest: editor.canRequest,
          canDiscover: editor.canDiscover,
          canAcquire: editor.canAcquire,
        })
        setNotice(
          editor.profileType === 'child'
            ? `${editor.username.trim()} added as a child profile. Assign books from their shelf.`
            : `${editor.username.trim()} added.`,
        )
      } else if (editor.id !== undefined) {
        await updateUser(editor.id, {
          displayName: editor.displayName.trim(),
          role: editor.role,
          credential: editor.credential || undefined,
          credentialType: editor.credential ? editor.credentialType : undefined,
          disabled: editor.disabled,
          preferredLanguages: editor.preferredLanguages,
          canRequest: editor.canRequest,
          canDiscover: editor.canDiscover,
          canAcquire: editor.canAcquire,
        })
        if (converting) {
          await setUserProfileType(editor.id, editor.profileType)
          setProfiles((map) => ({ ...map, [editor.id as number]: editor.profileType }))
          setNotice(
            editor.profileType === 'child'
              ? `${editor.username} is now a child profile. Assign books from Manage access on a book page.`
              : `${editor.username} is now an adult profile.`,
          )
        } else {
          setNotice('User updated.')
        }
      }
      setEditor(null)
      setConvertArmed(false)
      load()
    } catch (caught) {
      // User updates and profile-type changes are separate writes; reload so
      // the list and the editor reflect whatever actually persisted.
      load()
      void loadProfiles()
      setError(
        `${
          caught instanceof ApiError ? caught.message : 'Could not save the user'
        }. Some changes may already have been saved; the current values have been reloaded.`,
      )
    } finally {
      setSaving(false)
    }
  }

  const inputClass =
    'w-full rounded-card bg-surface-2 px-3.5 py-2.5 text-sm text-ink outline-none placeholder:text-ink-faint focus-visible:outline-2 focus-visible:outline-focus'

  return (
    <section className="rounded-panel bg-surface p-5 sm:p-6">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="text-base font-semibold text-ink">Household users</h2>
          <p className="mt-0.5 text-xs text-ink-faint">
            Create accounts for the people you share the library with.
          </p>
        </div>
        <Button
          variant="secondary"
          size="sm"
          onClick={() => {
            setConvertArmed(false)
            setEditor({
              mode: 'create',
              username: '',
              displayName: '',
              credential: '',
              credentialType: 'pin',
              role: 'user',
              profileType: 'adult',
              disabled: false,
              canRequest: true,
              canDiscover: false,
              canAcquire: true,
              preferredLanguages: [],
            })
          }}
        >
          <Plus size={14} aria-hidden />
          Add user
        </Button>
      </div>

      {error && (
        <p className="mt-4 rounded-card bg-surface-2 px-3.5 py-2.5 text-sm text-danger">{error}</p>
      )}
      {notice && (
        <p className="mt-4 rounded-card bg-surface-2 px-3.5 py-2.5 text-sm text-ink-soft">
          {notice}
        </p>
      )}

      <div className="mt-4 space-y-1">
        {users.map((user) => (
          <div
            key={user.id}
            className="flex flex-wrap items-center gap-3 rounded-card px-3 py-3 transition-colors hover:bg-surface-2"
          >
            <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-surface-3 font-display text-sm text-ink">
              {(user.displayName ?? user.username).slice(0, 1).toUpperCase()}
            </span>
            <div className="min-w-0 flex-1">
              <div className="flex flex-wrap items-center gap-2">
                <p className="text-sm font-medium text-ink">
                  {user.displayName ?? user.username}
                </p>
                <span className="text-xs text-ink-muted">
                  {user.displayName ? `@${user.username} · ` : ''}
                  {user.role === 'admin'
                    ? 'Administrator'
                    : profiles[user.id] === 'child'
                      ? 'Child profile'
                      : 'Household member'}
                </span>
                {user.credentialType === 'pin' && (
                  <span className="rounded-[3px] bg-surface-3 px-2 py-0.5 font-sans text-[10px] font-medium uppercase tracking-[0.14em] text-ink-soft">
                    PIN
                  </span>
                )}
                {user.disabled && (
                  <span className="rounded-[3px] bg-danger/10 px-2 py-0.5 font-sans text-[10px] font-medium uppercase tracking-[0.14em] text-danger">
                    Disabled
                  </span>
                )}
              </div>
              <p className="mt-0.5 text-xs text-ink-muted">
                {user.readerCount} {user.readerCount === 1 ? 'reader' : 'readers'}
              </p>
            </div>
            <Button variant="ghost" size="sm" onClick={() => void restartSetup(user.id)}>
              Restart setup
            </Button>
            {profiles[user.id] === 'child' && <Link to={`/settings/children/${user.id}/readers`} className="inline-flex h-8 items-center rounded-[3px] px-3.5 text-sm font-medium text-ink-soft hover:bg-surface-2 hover:text-ink">Readers</Link>}
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                setConvertArmed(false)
                setEditor({
                  mode: 'edit',
                  id: user.id,
                  username: user.username,
                  displayName: user.displayName ?? '',
                  credential: '',
                  credentialType: user.role === 'admin' ? 'password' : 'pin',
                  role: user.role,
                  profileType: profiles[user.id] === 'child' ? 'child' : 'adult',
                  disabled: user.disabled,
                  canRequest: user.canRequest !== false,
                  canDiscover: user.canDiscover === true,
                  canAcquire: user.canAcquire !== false,
                  preferredLanguages: user.preferredLanguages ?? [],
                })
              }}
            >
              <Pencil size={13} aria-hidden />
              Edit
            </Button>
          </div>
        ))}
        {users.length === 0 && !error && (
          <p className="px-3 py-2 text-sm text-ink-muted">No users yet.</p>
        )}
      </div>

      {editor && (
        <Modal
          title={editor.mode === 'create' ? 'Add user' : `Edit ${editor.username}`}
          onClose={() => setEditor(null)}
          footer={
            <>
              <Button variant="ghost" onClick={() => setEditor(null)}>
                Cancel
              </Button>
              <Button
                variant="primary"
                disabled={
                  saving ||
                  (editor.mode === 'create' &&
                    (!editor.username.trim() || !editor.credential.trim()))
                }
                onClick={() => {
                  if (converting && !convertArmed) {
                    setConvertArmed(true)
                    return
                  }
                  void save()
                }}
              >
                {saving
                  ? 'Saving…'
                  : editor.mode === 'create'
                    ? 'Add user'
                    : converting
                      ? convertArmed
                        ? 'Convert profile'
                        : 'Review profile change'
                      : 'Save'}
              </Button>
            </>
          }
        >
          <div className="space-y-4">
            {converting && (
              <p
                className={`rounded-card px-3.5 py-2.5 text-sm ${
                  convertArmed ? 'bg-danger/10 text-danger' : 'bg-surface-2 text-ink-soft'
                }`}
              >
                {editor.profileType === 'child' ? (
                  <>
                    {editor.username} becomes a child profile: no discovery, downloads or
                    acquisitions, and their shelf is cleared until you assign books. Likes and
                    interests are kept, and setup runs again the next time they sign in.
                  </>
                ) : (
                  <>
                    {editor.username} becomes an adult profile and can use Discover, downloads and
                    readers again. Setup runs again the next time they sign in.
                  </>
                )}
              </p>
            )}
            <label className="block">
              <span className="mb-1.5 block text-xs text-ink-muted">Username</span>
              <input
                value={editor.username}
                disabled={editor.mode === 'edit'}
                onChange={(event) =>
                  setEditor((current) =>
                    current ? { ...current, username: event.target.value } : current,
                  )
                }
                className={`${inputClass} disabled:opacity-60`}
              />
            </label>
            <label className="block">
              <span className="mb-1.5 block text-xs text-ink-muted">Display name</span>
              <input
                value={editor.displayName}
                onChange={(event) =>
                  setEditor((current) =>
                    current ? { ...current, displayName: event.target.value } : current,
                  )
                }
                placeholder={editor.username || 'optional'}
                className={inputClass}
              />
            </label>
            <label className="block">
              <span className="mb-1.5 block text-xs text-ink-muted">
                {editor.mode === 'create' ? 'Credential' : 'New credential (leave empty to keep)'}
              </span>
              <div className="mb-2 flex gap-2">
                {(['pin', 'password'] as const).map((type) => (
                  <button
                    key={type}
                    type="button"
                    disabled={editor.role === 'admin' && type === 'pin'}
                    aria-pressed={editor.credentialType === type}
                    onClick={() =>
                      setEditor((current) =>
                        current ? { ...current, credentialType: type } : current,
                      )
                    }
                    className={`rounded-[3px] px-3.5 py-1.5 font-sans text-[11px] font-medium uppercase tracking-[0.14em] transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${
                      editor.credentialType === type
                        ? 'bg-accent text-accent-ink'
                        : 'bg-surface-2 text-ink-soft hover:bg-surface-3 hover:text-ink'
                    }`}
                  >
                    {type === 'pin' ? '6-digit PIN' : 'Password'}
                  </button>
                ))}
              </div>
              <input
                type="password"
                value={editor.credential}
                onChange={(event) =>
                  setEditor((current) =>
                    current ? { ...current, credential: event.target.value } : current,
                  )
                }
                inputMode={editor.credentialType === 'pin' ? 'numeric' : undefined}
                maxLength={editor.credentialType === 'pin' ? 6 : undefined}
                placeholder={
                  editor.mode === 'create'
                    ? editor.credentialType === 'pin'
                      ? '6 digits'
                      : 'at least 8 characters'
                    : undefined
                }
                className={inputClass}
              />
              {editor.mode === 'create' && (
                <span className="mt-1.5 block text-xs text-ink-faint">
                  {editor.credentialType === 'pin'
                    ? 'Six digits; easy PINs like 123456 are rejected.'
                    : 'At least 8 characters. Administrators always use a password.'}
                </span>
              )}
            </label>
            <label className="block">
              <span className="mb-1.5 block text-xs text-ink-muted">Profile type</span>
              <select
                value={editor.profileType}
                onChange={(event) => {
                  const profileType = event.target.value as 'adult' | 'child'
                  setConvertArmed(false)
                  setEditor((current) =>
                    current
                      ? {
                          ...current,
                          profileType,
                          role: profileType === 'child' ? 'user' : current.role,
                        }
                      : current,
                  )
                }}
                className={inputClass}
              >
                <option value="adult">Adult</option>
                <option value="child">Child</option>
              </select>
              {editor.profileType === 'child' && (
                <span className="mt-1.5 block text-xs text-ink-faint">
                  Children can read books assigned to their shelf. Catalogue browsing and
                  requests are controlled below; downloads and delivery remain adult actions.
                </span>
              )}
            </label>
            {editor.profileType === 'child' && (
              <div>
                <span className="mb-1.5 block text-xs text-ink-muted">
                  Reading languages (in order)
                </span>
                {editor.preferredLanguages.length > 0 ? (
                  <ul className="mb-2 flex flex-wrap gap-2">
                    {editor.preferredLanguages.map((language, index) => (
                      <li key={language}>
                        <button
                          type="button"
                          aria-label={`Remove ${languageLabel(language)}`}
                          onClick={() =>
                            setEditor((current) =>
                              current
                                ? {
                                    ...current,
                                    preferredLanguages: current.preferredLanguages.filter(
                                      (item) => item !== language,
                                    ),
                                  }
                                : current,
                            )
                          }
                          className="inline-flex items-center gap-2 rounded-[3px] bg-surface-2 px-3 py-1.5 text-xs text-ink-soft transition-colors hover:bg-surface-3"
                        >
                          {languageLabel(language)}
                          {index === 0 && (
                            <span className="font-sans text-[10px] uppercase tracking-[0.14em] text-ink-faint">
                              Preferred
                            </span>
                          )}
                        </button>
                      </li>
                    ))}
                  </ul>
                ) : (
                  <p className="mb-2 text-xs text-ink-faint">No languages chosen yet.</p>
                )}
                <select
                  value=""
                  onChange={(event) => {
                    const value = event.target.value
                    if (!value) {
                      return
                    }
                    setEditor((current) =>
                      current
                        ? {
                            ...current,
                            preferredLanguages: [...current.preferredLanguages, value],
                          }
                        : current,
                    )
                  }}
                  className={inputClass}
                >
                  <option value="">Add a language…</option>
                  {LANGUAGES.filter(
                    ([code]) => !editor.preferredLanguages.includes(code),
                  ).map(([code, label]) => (
                    <option key={code} value={code}>
                      {label}
                    </option>
                  ))}
                </select>
                <span className="mt-1.5 block text-xs text-ink-faint">
                  Children read in these languages; the first is preferred.
                </span>
              </div>
            )}
            {editor.profileType === 'child' && (
              <label className="flex items-start gap-3">
                <input
                  type="checkbox"
                  checked={editor.canDiscover}
                  onChange={(event) =>
                    setEditor((current) =>
                      current ? { ...current, canDiscover: event.target.checked } : current,
                    )
                  }
                  className="mt-0.5 h-4 w-4 accent-accent"
                />
                <span>
                  <span className="block text-xs text-ink-muted">Allow Discover</span>
                  <span className="mt-1 block text-xs text-ink-faint">
                    Show public catalogue books and suggestions based on this child&apos;s shelf and interests.
                    Catalogue descriptions are not age filtered. The household library stays private.
                  </span>
                </span>
              </label>
            )}
            {editor.profileType === 'child' && (
              <label className="flex items-start gap-3">
                <input
                  type="checkbox"
                  checked={editor.canRequest}
                  onChange={(event) =>
                    setEditor((current) =>
                      current ? { ...current, canRequest: event.target.checked } : current,
                    )
                  }
                  className="mt-0.5 h-4 w-4 accent-accent"
                />
                <span>
                  <span className="block text-xs text-ink-muted">Can ask for books</span>
                  <span className="mt-1 block text-xs text-ink-faint">
                    Lets {editor.username || 'this child'} submit titles for administrator approval and search
                    the basic request catalogue. When off, Discover can still be browsed if allowed above.
                  </span>
                </span>
              </label>
            )}
            {editor.profileType === 'adult' && editor.role !== 'admin' && (
              <label className="flex items-start gap-3">
                <input
                  type="checkbox"
                  checked={editor.canAcquire}
                  onChange={(event) => setEditor((current) =>
                    current ? { ...current, canAcquire: event.target.checked } : current,
                  )}
                  className="mt-0.5 h-4 w-4 accent-accent"
                />
                <span>
                  <span className="block text-xs text-ink-muted">Can add books to the shared library</span>
                  <span className="mt-1 block text-xs text-ink-faint">
                    When off, this reader can use existing books and ask an administrator to approve new ones.
                  </span>
                </span>
              </label>
            )}
            <label className="block">
              <span className="mb-1.5 block text-xs text-ink-muted">Role</span>
              <select
                value={editor.role}
                onChange={(event) =>
                  setEditor((current) => {
                    if (!current) {
                      return current
                    }
                    const role = event.target.value as 'admin' | 'user'
                    return {
                      ...current,
                      role,
                      credentialType: role === 'admin' ? 'password' : current.credentialType,
                    }
                  })
                }
                className={inputClass}
              >
                <option value="user">Household member</option>
                <option value="admin" disabled={editor.profileType === 'child'}>
                  Administrator
                </option>
              </select>
            </label>
            {editor.mode === 'edit' && (
              <label className="flex items-center gap-2">
                <input
                  type="checkbox"
                  checked={editor.disabled}
                  onChange={(event) =>
                    setEditor((current) =>
                      current ? { ...current, disabled: event.target.checked } : current,
                    )
                  }
                  className="h-4 w-4 accent-[var(--color-accent)]"
                />
                <span className="text-sm text-ink-soft">
                  Disabled (cannot sign in; sessions are revoked)
                </span>
              </label>
            )}
          </div>
        </Modal>
      )}
    </section>
  )
}

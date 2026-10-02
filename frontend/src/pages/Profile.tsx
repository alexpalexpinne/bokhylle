import { useEffect, useState } from 'react'
import { Pencil } from 'lucide-react'
import { ApiError } from '../api/client'
import {
  type DeliveryTarget,
  createTarget,
  deleteTarget,
  fetchTargets,
  setTargetDefault,
  updateTarget,
} from '../api/delivery'
import { ExternalLink } from 'lucide-react'
import { changeCredential } from '../api/auth'
import { fetchHiddenSubjects, setBookPreference, setSubjectHidden } from '../api/library'
import { LANGUAGES } from '../lib/languages'
import { fetchDefaultReader } from '../api/delivery'
import {
  type AgentToken,
  type ReaderToken,
  fetchAgentTokens,
  fetchReaderTokens,
  updateProfile,
  type LikedBook,
  type ProfileStats,
  fetchLikedBooks,
  fetchProfileStats,
} from '../api/profile'
import { Button } from '../components/ui/Button'
import { Input } from '../components/ui/Field'
import { Modal } from '../components/ui/Modal'
import { PageHeader } from '../components/ui/PageHeader'
import { ProfileAvatar } from '../components/ProfileAvatar'
import { ProfilePhotoDialog } from '../components/ProfilePhotoDialog'
import { useAuth } from '../auth/useAuth'
import { useMutation } from '../lib/useMutation'
import { Navigate, NavLink, useNavigate, useParams } from 'react-router-dom'
import {
  ProfileAccount,
  ProfileOverview,
  ProfilePreferences,
  ProfileTaste,
} from './profile/sections'
import { ReadersSection } from './profile/ReadersSection'
import { ReaderTokensSection } from './profile/ReaderTokensSection'
import { IntegrationsSection } from './profile/IntegrationsSection'
import { AppearanceSection } from './profile/AppearanceSection'
import { SharingSection } from './profile/SharingSection'

const profileSections = [
  { id: 'overview', label: 'Overview', description: 'Your account at a glance.' },
  { id: 'preferences', label: 'Reading preferences', description: 'Formats, languages, requests, and notifications.' },
  { id: 'appearance', label: 'Appearance', description: 'Your shelves, decorations, and Spotlight rotation.' },
  { id: 'readers', label: 'Send to readers', description: 'Email destinations and reader app access.' },
  { id: 'taste', label: 'Your taste', description: 'Liked books and hidden categories.' },
  { id: 'integrations', label: 'Integrations', description: 'Connect an assistant to your profile.' },
  { id: 'account', label: 'Account security', description: 'Your sign-in credential.' },
] as const

type Editor = {
  mode: 'create' | 'edit'
  target?: DeliveryTarget
  name: string
  address: string
  deviceType: string
}

const READER_TYPES: [string, string][] = [
  ['kindle', 'Kindle'],
  ['pocketbook', 'PocketBook'],
  ['other', 'Other'],
]

function defaultReaderName(deviceType: string): string {
  return READER_TYPES.find(([value]) => value === deviceType)?.[1] ?? 'Reader'
}

export function Profile() {
  const { section } = useParams()
  const navigate = useNavigate()
  const activeSection = section ?? 'overview'
  const active = profileSections.find((item) => item.id === activeSection)
  const { user, refresh, demo } = useAuth()
  const [photoOpen, setPhotoOpen] = useState(false)
  const [targets, setTargets] = useState<DeliveryTarget[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<{ section: string; message: string } | null>(null)
  const [editor, setEditor] = useState<Editor | null>(null)
  const [saving, setSaving] = useState(false)
  const [confirmRemove, setConfirmRemove] = useState<DeliveryTarget | null>(null)
  const [busy, setBusy] = useState<number | null>(null)
  const [editingName, setEditingName] = useState<string | null>(null)
  const [savingName, setSavingName] = useState(false)
  const [prefFormat, setPrefFormat] = useState<'epub' | 'any'>(
    user?.preferredFormat === 'pdf' || user?.preferredFormat === 'any'
      ? 'any'
      : 'epub',
  )
  const [prefLanguages, setPrefLanguages] = useState<string[]>(
    user?.preferredLanguages?.length
      ? user.preferredLanguages
      : user?.preferredLanguage
        ? [user.preferredLanguage]
        : [],
  )
  const [languagesOpen, setLanguagesOpen] = useState(false)
  const [languagePickerOpen, setLanguagePickerOpen] = useState(false)
  const [languageQuery, setLanguageQuery] = useState('')
  const availableLanguages = LANGUAGES.filter(
    ([code, name]) =>
      !prefLanguages.includes(code) &&
      (languageQuery.trim() === '' ||
        name.toLowerCase().includes(languageQuery.trim().toLowerCase()) ||
        code.startsWith(languageQuery.trim().toLowerCase())),
  )
  const [prefMode, setPrefMode] = useState<'automatic' | 'ask'>(
    user?.acquisitionMode === 'ask' ? 'ask' : 'automatic',
  )
  const [notifyEmail, setNotifyEmail] = useState(user?.notificationEmail ?? '')
  const [notifyEnabled, setNotifyEnabled] = useState(user?.emailNotifications ?? false)
  const [savingPrefs, setSavingPrefs] = useState(false)
  const [currentSecret, setCurrentSecret] = useState('')
  const [newType, setNewType] = useState<'pin' | 'password'>('pin')
  const [newSecret, setNewSecret] = useState('')
  const [confirmSecret, setConfirmSecret] = useState('')
  const [savingCredential, setSavingCredential] = useState(false)
  const [hiddenSubjects, setHiddenSubjects] = useState<string[]>([])
  const [likedBooks, setLikedBooks] = useState<LikedBook[]>([])
  const [overviewStats, setOverviewStats] = useState<ProfileStats | null>(null)
  const [readerTokens, setReaderTokens] = useState<ReaderToken[]>([])
  const [newTokenName, setNewTokenName] = useState('')
  const [freshToken, setFreshToken] = useState<string | null>(null)
  const [agentTokens, setAgentTokens] = useState<AgentToken[]>([])
  const [newAgentName, setNewAgentName] = useState('')
  const [newAgentScope, setNewAgentScope] = useState<'read' | 'write'>('read')
  const [freshAgentToken, setFreshAgentToken] = useState<string | null>(null)
  const tokenMutation = useMutation()
  const [readerListWarning, setReaderListWarning] = useState<string | null>(null)
  const [agentListWarning, setAgentListWarning] = useState<string | null>(null)
  const [deliveryInfo, setDeliveryInfo] = useState<{
    senderAddress: string | null
    amazonUrl: string
  } | null>(null)

  function showNotice(message: string) {
    setNotice({ section: activeSection, message })
  }

  useEffect(() => {
    fetchLikedBooks()
      .then((data) => setLikedBooks(data.items))
      .catch((caught: unknown) => console.warn('profile.liked_books.load_failed', caught))
  }, [])

  useEffect(() => {
    if (activeSection !== 'overview') return
    let cancelled = false
    fetchProfileStats()
      .then((stats) => {
        if (!cancelled) setOverviewStats(stats)
      })
      .catch((caught: unknown) => console.warn('profile.stats.load_failed', caught))
    return () => { cancelled = true }
  }, [activeSection])

  async function removeLike(bookId: number) {
    try {
      await setBookPreference(bookId, null)
      setLikedBooks((current) => current.filter((book) => book.bookId !== bookId))
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not update that like')
    }
  }

  useEffect(() => {
    fetchHiddenSubjects()
      .then((data) => setHiddenSubjects(data.hidden))
      .catch((caught: unknown) => console.warn('profile.hidden_subjects.load_failed', caught))
    fetchReaderTokens()
      .then((data) => setReaderTokens(data.tokens))
      .catch((caught: unknown) => console.warn('profile.reader_tokens.load_failed', caught))
    fetchAgentTokens()
      .then((data) => setAgentTokens(data.tokens))
      .catch((caught: unknown) => console.warn('profile.agent_tokens.load_failed', caught))
  }, [])

  async function restoreSubject(normalized: string) {
    try {
      await setSubjectHidden(normalized, false)
      setHiddenSubjects((current) => current.filter((item) => item !== normalized))
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not restore the category')
    }
  }

  function load() {
    fetchTargets()
      .then((items) => {
        setTargets(items)
        setError(null)
      })
      .catch((caught: unknown) => {
        setError(caught instanceof ApiError ? caught.message : 'Could not load your readers')
      })
      .finally(() => setLoading(false))
  }

  useEffect(() => {
    load()
    fetchDefaultReader()
      .then((reader) =>
        setDeliveryInfo({
          senderAddress: reader.senderAddress,
          amazonUrl: reader.amazonUrl,
        }),
      )
      .catch((caught: unknown) => console.warn('profile.default_reader.load_failed', caught))
  }, [])

  async function saveCredential() {
    setError(null)
    if (newSecret !== confirmSecret) {
      setError('The new credential and its confirmation do not match.')
      return
    }
    setSavingCredential(true)
    try {
      await changeCredential({
        current: currentSecret,
        credentialType: user?.role === 'admin' ? 'password' : newType,
        credential: newSecret,
      })
      await refresh()
      setCurrentSecret('')
      setNewSecret('')
      setConfirmSecret('')
      showNotice('Credential updated. Your other devices were signed out.')
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not update your credential')
    } finally {
      setSavingCredential(false)
    }
  }

  async function savePreferences() {
    setSavingPrefs(true)
    setError(null)
    try {
      await updateProfile({
        preferredFormat: prefFormat,
        preferredLanguage: prefLanguages[0] ?? null,
        preferredLanguages: prefLanguages,
        acquisitionMode: prefMode,
        notificationEmail: notifyEmail,
        emailNotifications: notifyEnabled,
      })
      await refresh()
      showNotice('Preferences saved.')
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not save your preferences')
    } finally {
      setSavingPrefs(false)
    }
  }

  async function save() {
    if (!editor) {
      return
    }
    setSaving(true)
    setError(null)

    try {
      if (editor.mode === 'create') {
        await createTarget(editor.address.trim(), editor.deviceType, editor.name.trim() || undefined)
        showNotice('Reader added.')
      } else if (editor.target) {
        await updateTarget(editor.target.id, {
          name: editor.name.trim() || defaultReaderName(editor.deviceType),
          address: editor.address.trim(),
        })
        showNotice('Reader updated.')
      }
      setEditor(null)
      load()
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not save the reader')
    } finally {
      setSaving(false)
    }
  }

  async function saveName() {
    if (editingName === null) {
      return
    }
    setSavingName(true)
    setError(null)
    try {
      await updateProfile({ displayName: editingName.trim() })
      await refresh()
      showNotice('Profile updated.')
      setEditingName(null)
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not update your profile')
    } finally {
      setSavingName(false)
    }
  }

  async function makeDefault(target: DeliveryTarget) {
    setBusy(target.id)
    setNotice(null)
    try {
      const items = await setTargetDefault(target.id)
      setTargets(items)
      showNotice(`${target.name} is now your default reader.`)
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not set the default reader')
    } finally {
      setBusy(null)
    }
  }

  async function toggleEnabled(target: DeliveryTarget) {
    setBusy(target.id)
    setNotice(null)
    try {
      await updateTarget(target.id, { enabled: !target.enabled })
      showNotice(target.enabled ? `${target.name} disabled.` : `${target.name} enabled.`)
      load()
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not update the reader')
    } finally {
      setBusy(null)
    }
  }

  async function remove(target: DeliveryTarget) {
    setBusy(target.id)
    setError(null)
    try {
      await deleteTarget(target.id)
      setConfirmRemove(null)
      showNotice(`${target.name} removed.`)
      load()
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not remove the reader')
    } finally {
      setBusy(null)
    }
  }

  const enabledTargets = targets.filter((target) => target.enabled)

  if (!active) return <Navigate to="/profile" replace />

  return (
    <section>
      <PageHeader
        eyebrow="Profile"
        title={active.id === 'overview' ? 'My profile' : active.label}
        description={active.description}
      />

      {activeSection !== 'readers' && notice?.section === activeSection && (
        <p role="status" className="mt-5 border-l-2 border-success pl-4 text-sm text-ink-soft">
          {notice.message}
        </p>
      )}
      {activeSection !== 'readers' && error && (
        <p role="alert" className="mt-5 border-l-2 border-danger pl-4 text-sm text-danger">{error}</p>
      )}

      <div className="mt-8 flex flex-wrap items-center gap-4 rounded-panel bg-surface px-5 py-4">
        {user && <ProfileAvatar user={user} className="h-11 w-11 text-lg" />}
        <div className="min-w-0 flex-1">
          <p className="text-sm font-medium text-ink">
            {user?.displayName ?? user?.username}
          </p>
          <p className="text-xs text-ink-faint">
            {user?.displayName ? `@${user.username} · ` : ''}
            {user?.role === 'admin' ? 'Administrator' : 'Household member'}
          </p>
        </div>
        {!demo && <Button variant="ghost" size="sm" onClick={() => setPhotoOpen(true)}>
          Change picture
        </Button>}
        <Button
          variant="ghost"
          size="sm"
          onClick={() => setEditingName(user?.displayName ?? '')}
        >
          <Pencil size={13} aria-hidden />
          Edit name
        </Button>
      </div>

      {photoOpen && <ProfilePhotoDialog onClose={() => setPhotoOpen(false)} />}

      <div className="mt-8 grid gap-8 lg:grid-cols-[12rem_minmax(0,1fr)] lg:gap-12">
        <div className="lg:hidden">
          <label htmlFor="profile-section" className="mb-2 block text-xs font-medium text-ink-muted">Profile section</label>
          <select id="profile-section" value={activeSection} onChange={(event) => navigate(event.target.value === 'overview' ? '/profile' : `/profile/${event.target.value}`)} className="w-full rounded-card border border-line bg-surface px-4 py-3 text-sm text-ink">
            {profileSections.map((item) => <option key={item.id} value={item.id}>{item.label}</option>)}
          </select>
        </div>
        <nav aria-label="Profile sections" className="hidden lg:block">
          {profileSections.map((item) => <NavLink key={item.id} to={item.id === 'overview' ? '/profile' : `/profile/${item.id}`} end={item.id === 'overview'} className={({ isActive }) => `block border-l-2 px-4 py-3 text-sm transition-colors ${isActive ? 'border-accent font-medium text-ink' : 'border-transparent text-ink-muted hover:text-ink'}`}>{item.label}</NavLink>)}
        </nav>
        <div className="min-w-0">
      {activeSection === 'overview' && <ProfileOverview stats={overviewStats} />}
      {activeSection === 'appearance' && user && <AppearanceSection key={user.id} user={user} />}

      {activeSection === 'readers' && (
        <ReadersSection
          targets={targets}
          enabledTargets={enabledTargets}
          loading={loading}
          error={error}
          notice={notice?.section === 'readers' ? notice.message : null}
          busy={busy}
          openEditor={setEditor}
          openRemove={setConfirmRemove}
          makeDefault={(target) => void makeDefault(target)}
          toggleEnabled={(target) => void toggleEnabled(target)}
        />
      )}

      {activeSection === 'preferences' && (<>
        <ProfilePreferences
          prefFormat={prefFormat}
          setPrefFormat={setPrefFormat}
          prefLanguages={prefLanguages}
          setPrefLanguages={setPrefLanguages}
          languagesOpen={languagesOpen}
          setLanguagesOpen={setLanguagesOpen}
          languagePickerOpen={languagePickerOpen}
          setLanguagePickerOpen={setLanguagePickerOpen}
          languageQuery={languageQuery}
          setLanguageQuery={setLanguageQuery}
          availableLanguages={availableLanguages}
          prefMode={prefMode}
          setPrefMode={setPrefMode}
          notifyEmail={notifyEmail}
          setNotifyEmail={setNotifyEmail}
          notifyEnabled={notifyEnabled}
          setNotifyEnabled={setNotifyEnabled}
          savingPrefs={savingPrefs}
          savePreferences={() => void savePreferences()}
        />
        {user?.profileType !== 'child' && <SharingSection />}
      </>)}

      {activeSection === 'taste' && (
        <ProfileTaste
          likedBooks={likedBooks}
          hiddenSubjects={hiddenSubjects}
          removeLike={(bookId) => void removeLike(bookId)}
          restoreSubject={(subject) => void restoreSubject(subject)}
        />
      )}

      {activeSection === 'readers' && (
        <ReaderTokensSection
          tokens={readerTokens}
          setTokens={setReaderTokens}
          name={newTokenName}
          setName={setNewTokenName}
          freshToken={freshToken}
          setFreshToken={setFreshToken}
          listWarning={readerListWarning}
          setListWarning={setReaderListWarning}
          tokenMutation={tokenMutation}
        />
      )}

      {activeSection === 'integrations' && (
        <IntegrationsSection
          tokens={agentTokens}
          setTokens={setAgentTokens}
          name={newAgentName}
          setName={setNewAgentName}
          scope={newAgentScope}
          setScope={setNewAgentScope}
          freshToken={freshAgentToken}
          setFreshToken={setFreshAgentToken}
          listWarning={agentListWarning}
          setListWarning={setAgentListWarning}
          tokenMutation={tokenMutation}
        />
      )}

      {activeSection === 'account' && (
        <ProfileAccount
          role={user?.role}
          currentSecret={currentSecret}
          setCurrentSecret={setCurrentSecret}
          newType={newType}
          setNewType={setNewType}
          newSecret={newSecret}
          setNewSecret={setNewSecret}
          confirmSecret={confirmSecret}
          setConfirmSecret={setConfirmSecret}
          savingCredential={savingCredential}
          saveCredential={() => void saveCredential()}
        />
      )}

        </div>
      </div>

      {editor && (
        <Modal
          title={editor.mode === 'create' ? 'Add a reader' : 'Edit reader'}
          description={
            editor.deviceType === 'kindle'
              ? 'Kindle addresses usually end in @kindle.com.'
              : editor.deviceType === 'pocketbook'
                ? 'Find your Send-to-PocketBook address in the PocketBook app; no sender approval is needed.'
                : 'Use the delivery address your reader app gives you.'
          }
          onClose={() => setEditor(null)}
          footer={
            <>
              <Button variant="ghost" onClick={() => setEditor(null)}>
                Cancel
              </Button>
              <Button
                variant="primary"
                disabled={saving || !editor.address.trim()}
                onClick={() => void save()}
              >
                {saving ? 'Saving…' : editor.mode === 'create' ? 'Add reader' : 'Save'}
              </Button>
            </>
          }
        >
          <div className="space-y-4">
            <label className="block">
              <span className="mb-1.5 block text-xs text-ink-muted">Name</span>
              <Input
                value={editor.name}
                onChange={(event) =>
                  setEditor((current) =>
                    current ? { ...current, name: event.target.value } : current,
                  )
                }
              />
            </label>
            <label className="block">
              <span className="mb-1.5 block text-xs text-ink-muted">Reader type</span>
              <select
                value={editor.deviceType}
                onChange={(event) => {
                  const deviceType = event.target.value
                  setEditor((current) => {
                    if (!current) {
                      return current
                    }
                    const automatic =
                      current.name.trim() === '' ||
                      current.name === defaultReaderName(current.deviceType)
                    return {
                      ...current,
                      deviceType,
                      name: automatic ? defaultReaderName(deviceType) : current.name,
                    }
                  })
                }}
                disabled={editor.mode === 'edit'}
                className="w-full rounded-card bg-surface-2 px-4 py-2.5 text-sm text-ink outline-none focus-visible:outline-2 focus-visible:outline-focus"
              >
                {READER_TYPES.map(([value, label]) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
              <span className="mt-1.5 block text-xs text-ink-faint">
                Kobo and BOOX readers use OPDS under Reader apps.
              </span>
            </label>
            <label className="block">
              <span className="mb-1.5 block text-xs text-ink-muted">Delivery address</span>
              <Input
                value={editor.address}
                onChange={(event) =>
                  setEditor((current) =>
                    current ? { ...current, address: event.target.value } : current,
                  )
                }
                placeholder={
                  editor.deviceType === 'kindle'
                    ? 'name@kindle.com'
                    : editor.deviceType === 'pocketbook'
                      ? 'name@pbsync.com'
                      : 'reader@example.com'
                }
              />
              {editor.deviceType === 'kindle' && deliveryInfo && (
                <div className="mt-2 rounded-card bg-surface-2 px-3.5 py-2.5 text-xs text-ink-muted">
                  <p>
                    Amazon only accepts documents from approved senders — add{' '}
                    <strong>{deliveryInfo.senderAddress ?? 'your Bokhylle sender'}</strong> to your
                    approved personal document list, or Kindle silently rejects the file.
                  </p>
                  <a
                    href={deliveryInfo.amazonUrl}
                    target="_blank"
                    rel="noreferrer"
                    className="mt-1.5 inline-flex items-center gap-1.5 text-accent transition-colors hover:text-accent-strong"
                  >
                    Open Amazon personal document settings
                    <ExternalLink size={12} aria-hidden />
                  </a>
                </div>
              )}
              {editor.deviceType === 'pocketbook' && (
                <div className="mt-2 rounded-card bg-surface-2 px-3.5 py-2.5 text-xs text-ink-muted">
                  <p>
                    Find your Send-to-PocketBook address in the PocketBook app or on the device.
                    No sender approval is needed; books appear when it syncs over Wi-Fi.
                  </p>
                </div>
              )}
            </label>
          </div>
        </Modal>
      )}

      {editingName !== null && (
        <Modal
          title="Your name"
          description="Shown across Bokhylle instead of your username. Leave empty to use the username."
          onClose={() => setEditingName(null)}
          footer={
            <>
              <Button variant="ghost" onClick={() => setEditingName(null)}>
                Cancel
              </Button>
              <Button variant="primary" disabled={savingName} onClick={() => void saveName()}>
                {savingName ? 'Saving…' : 'Save'}
              </Button>
            </>
          }
        >
          <Input
            value={editingName}
            onChange={(event) => setEditingName(event.target.value)}
            placeholder={user?.username}
          />
        </Modal>
      )}

      {confirmRemove && (
        <Modal
          title={`Remove ${confirmRemove.name}?`}
          description="Books will no longer be sent to this address. You can add it again later."
          onClose={() => setConfirmRemove(null)}
          footer={
            <>
              <Button variant="ghost" onClick={() => setConfirmRemove(null)}>
                Cancel
              </Button>
              <Button
                variant="danger"
                disabled={busy === confirmRemove.id}
                onClick={() => void remove(confirmRemove)}
              >
                Remove
              </Button>
            </>
          }
        >
          <p className="text-sm text-ink-soft">{confirmRemove.address}</p>
        </Modal>
      )}
    </section>
  )
}

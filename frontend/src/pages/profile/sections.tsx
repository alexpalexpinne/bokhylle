import { ArrowUp, Plus, Trash2 } from 'lucide-react'
import type { Dispatch, SetStateAction } from 'react'
import { Link } from 'react-router-dom'
import type { LikedBook, ProfileStats } from '../../api/profile'
import { LANGUAGES, languageLabel } from '../../lib/languages'
import { Button } from '../../components/ui/Button'
import { Input } from '../../components/ui/Field'

export function ProfileOverview({ stats }: { stats: ProfileStats | null }) {
  const cards = [
    { label: 'On my shelf', count: stats?.shelf, to: '/library' },
    { label: 'Authors followed', count: stats?.authors, to: '/library?mode=authors&following=1' },
    { label: 'Books I like', count: stats?.liked, to: '/profile/taste' },
    { label: 'Books sent to readers', count: stats?.booksSent, to: '/activity' },
  ]

  return (
    <section aria-label="Your reading at a glance" className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
      {cards.map((item) => (
        <Link key={item.label} to={item.to} className="rounded-panel bg-surface p-5 transition-colors hover:bg-surface-2">
          <span className="block font-display text-3xl text-ink">{item.count ?? '—'}</span>
          <span className="mt-2 block text-sm text-ink-muted">{item.label}</span>
        </Link>
      ))}
    </section>
  )
}

type PreferencesProps = {
  prefFormat: 'epub' | 'any'
  setPrefFormat: (value: 'epub' | 'any') => void
  prefLanguages: string[]
  setPrefLanguages: Dispatch<SetStateAction<string[]>>
  languagesOpen: boolean
  setLanguagesOpen: Dispatch<SetStateAction<boolean>>
  languagePickerOpen: boolean
  setLanguagePickerOpen: Dispatch<SetStateAction<boolean>>
  languageQuery: string
  setLanguageQuery: (value: string) => void
  availableLanguages: typeof LANGUAGES
  prefMode: 'automatic' | 'ask'
  setPrefMode: (value: 'automatic' | 'ask') => void
  notifyEmail: string
  setNotifyEmail: (value: string) => void
  notifyEnabled: boolean
  setNotifyEnabled: (value: boolean) => void
  savingPrefs: boolean
  savePreferences: () => void
}

export function ProfilePreferences(props: PreferencesProps) {
  const {
    prefFormat,
    setPrefFormat,
    prefLanguages,
    setPrefLanguages,
    languagesOpen,
    setLanguagesOpen,
    languagePickerOpen,
    setLanguagePickerOpen,
    languageQuery,
    setLanguageQuery,
    availableLanguages,
    prefMode,
    setPrefMode,
    notifyEmail,
    setNotifyEmail,
    notifyEnabled,
    setNotifyEnabled,
    savingPrefs,
    savePreferences,
  } = props

  return (
    <section className="mt-6 rounded-panel bg-surface p-5">
      <h2 className="text-base font-semibold text-ink">Preferences</h2>
      <p className="mt-0.5 text-xs text-ink-faint">Used for new requests unless you choose otherwise.</p>

      <div className="mt-4 grid gap-5 sm:grid-cols-2">
        <div>
          <p className="text-xs font-medium text-ink-muted">Preferred format</p>
          <div className="mt-2 flex gap-2">
            {([
              ['epub', 'EPUB'],
              ['any', 'Any compatible'],
            ] as const).map(([value, label]) => (
              <button
                key={value}
                type="button"
                aria-pressed={prefFormat === value}
                onClick={() => setPrefFormat(value)}
                className={`rounded-[3px] px-3.5 py-1.5 font-sans text-[11px] font-medium uppercase tracking-[0.14em] transition-colors ${
                  prefFormat === value
                    ? 'bg-accent text-accent-ink'
                    : 'bg-surface-2 text-ink-soft hover:bg-surface-3 hover:text-ink'
                }`}
              >
                {label}
              </button>
            ))}
          </div>
        </div>

        <div className="sm:col-span-2">
          <div className="flex items-center justify-between gap-3">
            <p className="text-xs font-medium text-ink-muted">Preferred languages</p>
            <button
              type="button"
              onClick={() => {
                setLanguagesOpen((open) => !open)
                setLanguagePickerOpen(false)
                setLanguageQuery('')
              }}
              className="font-sans text-[10px] font-medium uppercase tracking-[0.16em] text-accent transition-colors hover:text-accent-strong"
            >
              {languagesOpen ? 'Done' : 'Edit'}
            </button>
          </div>

          {!languagesOpen ? (
            <p className="mt-1.5 text-sm text-ink">
              {prefLanguages.length > 0 ? prefLanguages.map(languageLabel).join(', ') : 'Any language'}
            </p>
          ) : (
            <div className="mt-1.5 max-w-md">
              {prefLanguages.length === 0 && (
                <p className="py-2 text-sm text-ink-faint">No languages chosen — any language is accepted.</p>
              )}
              <ul className="divide-y divide-line">
                {prefLanguages.map((language, index) => (
                  <li key={language} className="flex items-center gap-3 py-2">
                    <span className="flex-1 text-sm text-ink">{languageLabel(language)}</span>
                    {index === 0 ? (
                      <span className="font-sans text-[10px] uppercase tracking-[0.16em] text-ink-faint">Preferred</span>
                    ) : (
                      <button
                        type="button"
                        aria-label={`Make ${languageLabel(language)} preferred`}
                        onClick={() =>
                          setPrefLanguages((current) => [language, ...current.filter((item) => item !== language)])
                        }
                        className="text-ink-faint transition-colors hover:text-ink"
                      >
                        <ArrowUp size={14} aria-hidden />
                      </button>
                    )}
                    <button
                      type="button"
                      aria-label={`Remove ${languageLabel(language)}`}
                      onClick={() => setPrefLanguages((current) => current.filter((item) => item !== language))}
                      className="text-ink-faint transition-colors hover:text-danger"
                    >
                      <Trash2 size={14} aria-hidden />
                    </button>
                  </li>
                ))}
              </ul>

              {!languagePickerOpen ? (
                <button
                  type="button"
                  onClick={() => setLanguagePickerOpen(true)}
                  className="mt-2 inline-flex items-center gap-1.5 font-sans text-[11px] font-medium uppercase tracking-[0.16em] text-accent transition-colors hover:text-accent-strong"
                >
                  <Plus size={12} aria-hidden />
                  Add language
                </button>
              ) : (
                <div className="mt-2 rounded-[3px] border border-line bg-surface-2 p-2">
                  <input
                    autoFocus
                    value={languageQuery}
                    onChange={(event) => setLanguageQuery(event.target.value)}
                    placeholder="Search languages…"
                    className="w-full rounded-[3px] border border-line bg-surface px-2.5 py-1.5 text-sm text-ink outline-none placeholder:text-ink-faint focus:border-accent"
                  />
                  <ul className="mt-1.5 max-h-44 overflow-y-auto">
                    {availableLanguages.map(([code, name]) => (
                      <li key={code}>
                        <button
                          type="button"
                          onClick={() => {
                            setPrefLanguages((current) => (current.includes(code) ? current : [...current, code]))
                            setLanguagePickerOpen(false)
                            setLanguageQuery('')
                          }}
                          className="flex w-full items-center justify-between gap-3 px-2 py-1.5 text-left text-sm text-ink-soft transition-colors hover:bg-surface-3 hover:text-ink"
                        >
                          <span>{name}</span>
                          <span className="font-sans text-[10px] uppercase tracking-[0.14em] text-ink-faint">{code}</span>
                        </button>
                      </li>
                    ))}
                    {availableLanguages.length === 0 && <li className="px-2 py-1.5 text-xs text-ink-faint">No match.</li>}
                  </ul>
                </div>
              )}
            </div>
          )}

          <p className="mt-1.5 text-xs text-ink-faint">
            Books in any of these are accepted when Bokhylle picks a release; the first is preferred and used as the default for search.
          </p>
        </div>
      </div>

      <div className="mt-5">
        <p className="text-xs font-medium text-ink-muted">Download selection</p>
        <div className="mt-2 grid gap-2 sm:grid-cols-2">
          {([
            ['automatic', 'Let Bokhylle choose', 'Get picks the best suitable file using your language and format preferences. Bokhylle asks when it is unsure.'],
            ['ask', 'Show available versions', 'Opening a book shows the available torrents and files. Choose a version, then Get to download it.'],
          ] as const).map(([value, label, description]) => (
            <button
              key={value}
              type="button"
              aria-pressed={prefMode === value}
              onClick={() => setPrefMode(value)}
              className={`rounded-card px-4 py-3 text-left transition-colors ${prefMode === value ? 'bg-surface-2' : 'hover:bg-surface-2/60'}`}
            >
              <span className="block text-sm font-medium text-ink">{label}</span>
              <span className="mt-0.5 block text-xs text-ink-muted">{description}</span>
            </button>
          ))}
        </div>
      </div>

      <div className="mt-5 rounded-card bg-surface-2 px-4 py-3">
        <label className="block">
          <span className="text-xs text-ink-muted">Notification email</span>
          <input
            type="email"
            value={notifyEmail}
            onChange={(event) => setNotifyEmail(event.target.value)}
            placeholder="you@example.com"
            className="mt-1.5 w-full rounded-card bg-surface-3 px-3 py-2 text-sm text-ink outline-none placeholder:text-ink-faint focus-visible:outline-2 focus-visible:outline-focus"
          />
        </label>
        <label className="mt-3 flex items-center gap-2 text-sm text-ink-soft">
          <input type="checkbox" checked={notifyEnabled} onChange={(event) => setNotifyEnabled(event.target.checked)} className="h-4 w-4 accent-[var(--color-accent)]" />
          Email me when a request is ready, needs a choice, or fails
        </label>
      </div>

      <div className="mt-5">
        <Button variant="primary" size="sm" disabled={savingPrefs} onClick={savePreferences}>
          {savingPrefs ? 'Saving…' : 'Save preferences'}
        </Button>
      </div>
    </section>
  )
}

type TasteProps = {
  likedBooks: LikedBook[]
  hiddenSubjects: string[]
  removeLike: (bookId: number) => void
  restoreSubject: (subject: string) => void
}

export function ProfileTaste({ likedBooks, hiddenSubjects, removeLike, restoreSubject }: TasteProps) {
  return (
    <>
      {likedBooks.length > 0 && (
        <section className="mt-6 rounded-panel bg-surface p-5">
          <h2 className="text-base font-semibold text-ink">Books you like</h2>
          <p className="mt-0.5 text-xs text-ink-faint">Taste signals from setup and book pages. Removing one never changes your shelf.</p>
          <ul className="mt-4 divide-y divide-line">
            {likedBooks.map((book) => (
              <li key={book.bookId} className="flex items-center justify-between gap-3 py-3">
                <span className="min-w-0">
                  <span className="block truncate text-sm text-ink">{book.title}</span>
                  <span className="block truncate text-xs text-ink-faint">{book.authors.join(', ')}{book.readable ? ' · in your household library' : ''}</span>
                </span>
                <Button variant="ghost" size="sm" className="shrink-0" onClick={() => removeLike(book.bookId)}>Remove</Button>
              </li>
            ))}
          </ul>
        </section>
      )}

      {hiddenSubjects.length > 0 && (
        <section className="mt-6 rounded-panel bg-surface p-5">
          <h2 className="text-base font-semibold text-ink">Hidden categories</h2>
          <p className="mt-0.5 text-xs text-ink-faint">Categories you hid from Home. They stay hidden for you only.</p>
          <ul className="mt-4 flex flex-wrap gap-2">
            {hiddenSubjects.map((subject) => (
              <li key={subject}>
                <button type="button" onClick={() => restoreSubject(subject)} className="inline-flex items-center gap-2 rounded-full bg-surface-2 px-3.5 py-1.5 text-xs text-ink-soft transition-colors hover:bg-surface-3 hover:text-ink">
                  {subject}
                  <span className="text-ink-faint">Show again</span>
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
      {likedBooks.length === 0 && hiddenSubjects.length === 0 && <p className="rounded-panel bg-surface p-6 text-sm text-ink-muted">Books you like and categories you hide will appear here.</p>}
    </>
  )
}

type AccountProps = {
  role: string | undefined
  currentSecret: string
  setCurrentSecret: (value: string) => void
  newType: 'pin' | 'password'
  setNewType: (value: 'pin' | 'password') => void
  newSecret: string
  setNewSecret: (value: string) => void
  confirmSecret: string
  setConfirmSecret: (value: string) => void
  savingCredential: boolean
  saveCredential: () => void
}

export function ProfileAccount(props: AccountProps) {
  const { role, currentSecret, setCurrentSecret, newType, setNewType, newSecret, setNewSecret, confirmSecret, setConfirmSecret, savingCredential, saveCredential } = props
  const isAdmin = role === 'admin'
  return (
    <section className="mt-6 rounded-panel bg-surface p-5">
      <h2 className="text-base font-semibold text-ink">Sign-in credential</h2>
      <p className="mt-0.5 text-xs text-ink-faint">{isAdmin ? 'Administrators always sign in with a password.' : 'Choose a 6-digit PIN or a password. Changing it signs out your other devices.'}</p>

      <div className="mt-4 max-w-md">
        <label className="block">
          <span className="mb-1.5 block text-xs text-ink-muted">Current credential</span>
          <Input type="password" value={currentSecret} onChange={(event) => setCurrentSecret(event.target.value)} />
        </label>
      </div>

      {!isAdmin && (
        <div className="mt-4">
          <span className="mb-1.5 block text-xs text-ink-muted">New credential type</span>
          <div className="flex gap-2">
            {(['pin', 'password'] as const).map((type) => (
              <button key={type} type="button" aria-pressed={newType === type} onClick={() => setNewType(type)} className={`rounded-[3px] px-3.5 py-2 font-sans text-[11px] font-medium uppercase tracking-[0.14em] transition-colors ${newType === type ? 'bg-accent text-accent-ink' : 'bg-surface-2 text-ink-soft hover:bg-surface-3 hover:text-ink'}`}>
                {type === 'pin' ? 'PIN' : 'Password'}
              </button>
            ))}
          </div>
        </div>
      )}

      <div className="mt-4 grid max-w-2xl gap-4 sm:grid-cols-2">
        <label className="block">
          <span className="mb-1.5 block text-xs text-ink-muted">{newType === 'pin' && !isAdmin ? 'New PIN' : 'New password'}</span>
          <Input type="password" value={newSecret} onChange={(event) => setNewSecret(event.target.value)} inputMode={newType === 'pin' && !isAdmin ? 'numeric' : undefined} maxLength={newType === 'pin' && !isAdmin ? 6 : undefined} />
        </label>
        <label className="block">
          <span className="mb-1.5 block text-xs text-ink-muted">{newType === 'pin' && !isAdmin ? 'Confirm new PIN' : 'Confirm new password'}</span>
          <Input type="password" value={confirmSecret} onChange={(event) => setConfirmSecret(event.target.value)} />
        </label>
      </div>

      <div className="mt-4">
        <Button variant="primary" size="sm" disabled={savingCredential || !currentSecret || !newSecret} onClick={saveCredential}>
          {savingCredential ? 'Saving…' : 'Update credential'}
        </Button>
      </div>
    </section>
  )
}

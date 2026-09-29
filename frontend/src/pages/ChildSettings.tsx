import { useState } from 'react'
import { Monitor, Moon, Sun } from 'lucide-react'
import { useAuth } from '../auth/useAuth'
import { ProfileAvatar } from '../components/ProfileAvatar'
import { ProfilePhotoDialog } from '../components/ProfilePhotoDialog'
import { Button } from '../components/ui/Button'
import { PageHeader } from '../components/ui/PageHeader'
import { readReaderAppearance, storeReaderAppearance } from '../lib/readerAppearance'
import { useThemeChoice } from '../theme'

const themes = [
  { value: 'system', label: 'System', icon: Monitor },
  { value: 'paper', label: 'Paper', icon: Sun },
  { value: 'ink', label: 'Ink', icon: Moon },
] as const

export function ChildSettings() {
  const { user, demo } = useAuth()
  const { choice, choose } = useThemeChoice()
  const [photoOpen, setPhotoOpen] = useState(false)
  const [readerAppearance, setReaderAppearance] = useState(() => readReaderAppearance(user?.id))

  if (!user) return null

  return (
    <section>
      <PageHeader eyebrow="Profile" title="My settings" description="Make your reading space feel right for you." />

      <section className="mt-8 rounded-panel bg-surface p-5 sm:p-6" aria-labelledby="child-profile-heading">
        <h2 id="child-profile-heading" className="font-display text-title text-ink">My profile</h2>
        <div className="mt-5 flex flex-wrap items-center gap-4">
          <ProfileAvatar user={user} className="h-14 w-14 text-lg" />
          <p className="min-w-0 flex-1 text-sm font-medium text-ink">{user.displayName ?? user.username}</p>
          {!demo && <Button variant="ghost" size="sm" onClick={() => setPhotoOpen(true)}>Change picture</Button>}
        </div>
      </section>

      <section className="mt-5 rounded-panel bg-surface p-5 sm:p-6" aria-labelledby="child-appearance-heading">
        <h2 id="child-appearance-heading" className="font-display text-title text-ink">Appearance</h2>
        <p className="mt-2 text-sm text-ink-muted">These choices are saved in this browser.</p>

        <div className="mt-6">
          <p className="text-sm font-medium text-ink">Theme</p>
          <div role="group" aria-label="Theme" className="mt-3 flex flex-wrap gap-2">
            {themes.map((option) => (
              <button
                key={option.value}
                type="button"
                aria-pressed={choice === option.value}
                onClick={() => choose(option.value)}
                className={`inline-flex min-h-10 items-center gap-2 rounded-[3px] px-4 text-sm transition-colors ${choice === option.value ? 'bg-accent text-accent-ink' : 'bg-surface-2 text-ink-soft hover:bg-surface-3 hover:text-ink'}`}
              >
                <option.icon size={16} aria-hidden />
                {option.label}
              </button>
            ))}
          </div>
        </div>

        <div className="mt-7 max-w-md border-t border-line pt-6">
          <label htmlFor="child-reader-font" className="text-sm font-medium text-ink">Book text size: {readerAppearance.textScale}%</label>
          <input
            id="child-reader-font"
            type="range"
            min="80"
            max="200"
            step="10"
            value={readerAppearance.textScale}
            onChange={(event) => {
              const value = Number(event.target.value)
              const next = { ...readerAppearance, textScale: value }
              setReaderAppearance(next)
              storeReaderAppearance(user.id, next)
            }}
            className="mt-4 w-full accent-accent"
          />
          <p className="mt-2 text-xs text-ink-muted">Used when you open an EPUB in Bokhylle.</p>
        </div>
      </section>

      <p className="mt-6 text-sm text-ink-muted">An administrator sets up your external reader. You can open assigned EPUBs directly in Bokhylle without one.</p>

      {photoOpen && <ProfilePhotoDialog onClose={() => setPhotoOpen(false)} />}
    </section>
  )
}

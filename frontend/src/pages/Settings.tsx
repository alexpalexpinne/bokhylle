import { Link, Navigate, NavLink, useNavigate, useParams } from 'react-router-dom'
import { PageHeader } from '../components/ui/PageHeader'
import { SettingsFields } from './settings/SettingsFields'
import { groups, type SettingsSection } from './settings/groups'
import { HouseholdUsers } from './settings/HouseholdUsers'
import {
  GettingBooksConnections,
  LibraryHealthPanel,
  SettingsOverview,
  WatchFolderStatus,
} from './settings/HealthPanels'
import { MaintenanceTools } from './settings/MaintenanceTools'

const sections: { id: SettingsSection; label: string; description: string }[] = [
  { id: 'overview', label: 'Overview', description: 'The state of your library and connected services.' },
  { id: 'household', label: 'Household', description: 'Manage the people who use this library.' },
  { id: 'library', label: 'Library', description: 'Storage, scanning, and file health.' },
  { id: 'getting-books', label: 'Getting books', description: 'Sources, download clients, imports, and retries.' },
  { id: 'metadata', label: 'Metadata', description: 'Book information, ratings, and enrichment.' },
  { id: 'delivery', label: 'Delivery', description: 'Email books to readers.' },
  { id: 'server', label: 'Server', description: 'Security, backups, and diagnostics.' },
]

export function Settings() {
  const navigate = useNavigate()
  const { section, connection } = useParams()
  const active = sections.find((item) => item.id === (section ?? 'overview'))
  if (!active || (connection && active.id !== 'getting-books')) {
    return <Navigate to="/settings" replace />
  }

  const connectionTitles: Record<string, string> = {
    prowlarr: 'Prowlarr', torznab: 'Torznab', newznab: 'Newznab',
    qbittorrent: 'qBittorrent', sabnzbd: 'SABnzbd',
  }
  const connectionGroup = connection ? groups.find((group) => group.title === connectionTitles[connection]) : null
  if (connection && !connectionGroup) {
    return <Navigate to="/settings/getting-books" replace />
  }

  const title = connectionGroup?.title ?? (active.id === 'overview' ? 'Settings' : active.label)
  const sectionGroups = groups.filter((group) => group.section === active.id)

  return (
    <section>
      <PageHeader
        eyebrow="Administration"
        title={title}
        description={connectionGroup ? 'Configure and test this connection.' : active.description}
        actions={
          connectionGroup ? (
            <Link
              to="/settings/getting-books"
              className="text-sm font-medium text-accent hover:text-accent-strong"
            >
              Back to Getting books
            </Link>
          ) : undefined
        }
      />
      <div className="mt-8 grid gap-8 lg:grid-cols-[12rem_minmax(0,1fr)] lg:gap-12">
        <div className="lg:hidden">
          <label htmlFor="settings-section" className="mb-2 block text-xs font-medium text-ink-muted">Administration section</label>
          <select id="settings-section" value={active.id} onChange={(event) => navigate(event.target.value === 'overview' ? '/settings' : `/settings/${event.target.value}`)} className="w-full rounded-card border border-line bg-surface px-4 py-3 text-sm text-ink">
            {sections.map((item) => <option key={item.id} value={item.id}>{item.label}</option>)}
          </select>
        </div>
        <nav aria-label="Settings sections" className="hidden lg:block">
          {sections.map((item) => (
            <NavLink
              key={item.id}
              to={item.id === 'overview' ? '/settings' : `/settings/${item.id}`}
              end={item.id === 'overview'}
              className={({ isActive }) =>
                `block border-b border-line px-1 py-3 text-sm transition-colors lg:border-b-0 lg:border-l-2 lg:px-4 ${
                  isActive
                    ? 'border-accent font-medium text-ink lg:border-accent'
                    : 'text-ink-muted hover:text-ink lg:border-transparent'
                }`
              }
            >
              {item.label}
            </NavLink>
          ))}
        </nav>
        <div className="min-w-0 space-y-10">
          {active.id === 'overview' && <SettingsOverview />}
          {active.id === 'household' && <HouseholdUsers />}
          {active.id === 'library' && (
            <>
              <LibraryHealthPanel />
              <SettingsFields key="library" groups={sectionGroups} />
              <MaintenanceTools kind="library" />
            </>
          )}
          {active.id === 'getting-books' && (
            connectionGroup ? (
              <SettingsFields key={connection} groups={[connectionGroup]} />
            ) : (
              <>
                <GettingBooksConnections />
                <SettingsFields
                  key="getting-books"
                  groups={sectionGroups.filter(
                    (group) => !Object.values(connectionTitles).includes(group.title),
                  )}
                />
                <WatchFolderStatus />
                <MaintenanceTools kind="getting-books" />
              </>
            )
          )}
          {active.id === 'metadata' && (
            <>
              <SettingsFields key="metadata" groups={sectionGroups} />
              <MaintenanceTools kind="metadata" />
            </>
          )}
          {active.id === 'delivery' && (
            <>
              <p className="text-sm text-ink-muted">
                Adults can add a Kindle, PocketBook, or another email destination in{' '}
                <Link to="/profile" className="text-accent underline hover:text-accent-strong">Profile</Link>.
                Set up a child&apos;s reader from <Link to="/settings/household" className="text-accent underline hover:text-accent-strong">Household</Link>.
                The Kindle address below is a household fallback.
              </p>
              <SettingsFields key="delivery" groups={sectionGroups} />
            </>
          )}
          {active.id === 'server' && (
            <>
              <SettingsFields key="server" groups={sectionGroups} />
              <MaintenanceTools kind="server" />
            </>
          )}
        </div>
      </div>
    </section>
  )
}

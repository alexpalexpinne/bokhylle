import type { ReactNode } from 'react'
import type { ServerStatus } from '../../api/server'
import type { BackupStatus } from '../../api/maintenance'
import { ButtonAnchor } from '../../components/ui/Button'
import { timestamp } from './serverFormat'

function bytes(value: number) {
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB']
  const index = Math.min(Math.max(0, Math.floor(Math.log2(Math.max(value, 1)) / 10)), units.length - 1)
  return `${(value / 1024 ** index).toLocaleString(undefined, { maximumFractionDigits: 1 })} ${units[index]}`
}

function uptime(seconds: number) {
  const days = Math.floor(seconds / 86400)
  const hours = Math.floor(seconds / 3600) % 24
  const minutes = Math.floor(seconds / 60) % 60
  return days ? `${days}d ${hours}h ${minutes}m` : hours ? `${hours}h ${minutes}m` : `${minutes}m`
}

export function Facts({ items }: { items: [string, ReactNode][] }) {
  return <dl className="grid gap-x-6 gap-y-3 text-sm sm:grid-cols-[10rem_minmax(0,1fr)]">
    {items.map(([label, value]) => <div key={label} className="contents">
      <dt className="text-ink-muted">{label}</dt>
      <dd className="-mt-2 min-w-0 break-words text-ink sm:mt-0">{value}</dd>
    </div>)}
  </dl>
}

export function IdentityPanel({ status }: { status: ServerStatus }) {
  const { build } = status
  return <section aria-labelledby="server-identity">
    <h2 id="server-identity" className="mb-5 font-display text-2xl text-ink">Bokhylle</h2>
    <Facts items={[
      ['Version', build.version],
      ['Build', <span key="build">{build.commit ? <a className="text-accent underline" href={`https://github.com/alexpalexpinne/bokhylle/commit/${build.commit}`}>{build.commit.slice(0, 7)}</a> : 'Commit unavailable'}{build.dirty === true ? ' · local changes' : build.dirty === null ? ' · working tree unknown' : ''}</span>],
      ['Built', build.builtAt == null ? 'Build date unavailable' : timestamp(build.builtAt)],
      ['Started', timestamp(status.startedAt)],
      ['Uptime', uptime(status.uptimeSeconds)],
      ['Installation', build.installation === 'docker' ? 'Docker' : 'Source build'],
      ['Database', status.databaseOk ? 'Responding' : 'Needs attention'],
    ]} />
  </section>
}

export function StoragePanel({ status }: { status: ServerStatus }) {
  return <section aria-labelledby="server-storage" className="border-t border-line pt-6">
    <h2 id="server-storage" className="font-display text-2xl text-ink">Storage</h2>
    <p className="mt-2 text-sm text-ink-muted">Paths on the same filesystem share the space shown below.</p>
    <div className="mt-5 space-y-6">
      {status.storage.map((group, index) => <div key={index}>
        <div className="flex flex-wrap justify-between gap-2 text-sm">
          <h3 className="font-medium text-ink">{group.locations.map((location) => location.label).join(' · ')}</h3>
          <p className="text-ink-soft">{group.availableBytes != null && group.totalBytes != null ? `${bytes(group.availableBytes)} free / ${bytes(group.totalBytes)}` : 'Capacity unavailable'}</p>
        </div>
        {group.lowSpace && <p role="status" className="mt-2 text-sm text-danger">Storage is running low. Free space before adding more books.</p>}
        {group.error && <p role="status" className="mt-2 text-sm text-ink-muted">{group.error}</p>}
        <ul className="mt-3 space-y-2 border-l border-line pl-4 text-xs text-ink-muted">
          {group.locations.map((location) => <li key={location.label}>
            <span className="text-ink-soft">{location.label}</span>{' '}<code className="break-all">{location.path}</code>
            <span className={`ml-2 ${location.writable ? '' : 'text-danger'}`}>{location.writable ? 'Writable' : location.error ?? 'Not writable'}</span>
          </li>)}
        </ul>
      </div>)}
    </div>
  </section>
}

export function BackupPanel({ status }: { status: BackupStatus }) {
  const attention = status.outcome === 'failed' || status.inventoryError
  return <div className="space-y-5">
    {attention && <p role="status" className="text-sm font-medium text-danger">Backups need attention</p>}
    <Facts items={[
      ['Automatic', status.schedulerEnabled ? `Every ${status.intervalHours} hours · keep ${status.keep}` : 'Disabled'],
      ['Last successful backup', status.lastSuccess ? `${timestamp(status.lastSuccess.createdAt)} · ${bytes(status.lastSuccess.size)}` : 'No successful backup recorded'],
      ['Last attempt', status.lastAttempt ? `${timestamp(status.lastAttempt.at)} · ${status.outcome}` : 'No attempt recorded'],
      ['Next scheduled attempt', status.nextScheduledAt != null ? timestamp(status.nextScheduledAt) : 'Not scheduled'],
      ['Last failure', status.lastFailure ? <span key="failure">{timestamp(status.lastFailure.at)}<span className="mt-1 block text-ink-muted">{status.lastFailure.summary}</span></span> : 'None recorded'],
    ]} />
    {status.inventoryError && <p className="text-sm text-danger">{status.inventoryError}</p>}
    {!status.latest && status.lastSuccess && <p className="text-sm text-ink-muted">The last successful backup is recorded in history, but no completed backup is currently available in the backup directory.</p>}
    <div className="border-l-2 border-line pl-4 text-sm text-ink-soft">
      <p>Database backups protect Bokhylle state. Back up your <strong className="font-medium text-ink">config and library directories</strong> together for complete recovery, including books and saved artwork.</p>
      <p className="mt-2 text-xs text-ink-muted">Scheduled backups keep integration credentials. This download removes secret settings; re-enter those credentials after restoring it.</p>
    </div>
    <ButtonAnchor href="/api/admin/backup" download>Download database backup</ButtonAnchor>
  </div>
}

import { useEffect, useState, type ReactNode } from 'react'
import { Link } from 'react-router-dom'
import { ApiError } from '../../api/client'
import { fetchScanStatus, triggerScan } from '../../api/library'
import {
  type IntegrationHealth,
  type LibraryHealth,
  fetchIntegrationHealth,
  fetchLibraryHealth,
  testProwlarr,
  testTorznab,
  testNewznab,
  testSabnzbd,
  testQbittorrent,
  fetchWatchStatus,
} from '../../api/settings'
import { Button } from '../../components/ui/Button'

async function runLibraryScan(): Promise<string> {
  await triggerScan()

  for (let attempt = 0; attempt < 600; attempt += 1) {
    const status = await fetchScanStatus()
    if (!status.running) {
      if (status.error) {
        throw new Error(`Scan failed: ${status.error}`)
      }
      const summary = status.summary
      if (summary) {
        return `Scan finished: ${summary.filesFound} files, ${summary.indexed} added, ${summary.updated} updated, ${summary.duplicates} duplicates, ${summary.errors} errors`
      }
      return 'Scan finished'
    }
    await new Promise((resolve) => setTimeout(resolve, 1000))
  }

  return 'Scan is still running'
}

function HealthRow({
  label,
  ok,
  detail,
  children,
}: {
  label: string
  ok: boolean | null
  detail: string
  children?: ReactNode
}) {
  const mark = ok === null ? '—' : ok ? '✓' : '✗'
  const tone = ok === null ? 'text-ink-faint' : ok ? 'text-success' : 'text-danger'
  return (
    <div className="border-b border-line py-3 last:border-b-0">
      <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <span className="font-sans text-[11px] uppercase tracking-[0.14em] text-ink-muted">
          {label}
        </span>
        <span className={`font-sans text-xs ${tone}`}>
          {mark} {detail}
        </span>
      </div>
      {children}
    </div>
  )
}

function connectionState(connection: { configured: boolean; ok: boolean; version: string | null; error: string | null }) {
  if (!connection.configured) {
    return 'Not configured'
  }
  if (!connection.ok) {
    return `Needs attention${connection.error ? ` · ${connection.error}` : ''}`
  }
  return connection.version ? `Connected · ${connection.version}` : 'Connected'
}

export function SettingsOverview() {
  const [integrations, setIntegrations] = useState<IntegrationHealth | null>(null)
  const [library, setLibrary] = useState<LibraryHealth | null>(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    let mounted = true
    void Promise.allSettled([fetchIntegrationHealth(), fetchLibraryHealth()]).then(
      ([integrationResult, libraryResult]) => {
        if (!mounted) {
          return
        }
        if (integrationResult.status === 'fulfilled') {
          setIntegrations(integrationResult.value)
        }
        if (libraryResult.status === 'fulfilled') {
          setLibrary(libraryResult.value)
        }
        setError(integrationResult.status === 'rejected' || libraryResult.status === 'rejected')
      },
    )
    return () => {
      mounted = false
    }
  }, [])

  const rows = [
    {
      title: 'Library',
      to: '/settings/library',
      detail: library
        ? `${library.books} books · ${library.missingFiles} files missing from disk`
        : 'Checking library…',
    },
    {
      title: 'Getting books',
      to: '/settings/getting-books',
      detail: integrations
        ? `Prowlarr: ${connectionState(integrations.prowlarr)} · Torznab: ${connectionState(integrations.torznab)} · Newznab: ${connectionState(integrations.newznab)} · qBittorrent: ${connectionState(integrations.qbittorrent)} · SABnzbd: ${connectionState(integrations.sabnzbd)}`
        : 'Checking connections…',
    },
    {
      title: 'Delivery',
      to: '/settings/delivery',
      detail: integrations
        ? integrations.smtp.configured
          ? 'Email delivery configured'
          : 'Email delivery not configured'
        : 'Checking delivery…',
    },
    { title: 'Household', to: '/settings/household', detail: 'Manage members and child profiles' },
  ]

  return (
    <section>
      <h2 className="font-display text-2xl text-ink">At a glance</h2>
      <p className="mt-2 text-sm text-ink-muted">Open a section to change its settings or investigate a problem.</p>
      {error && <p className="mt-4 text-sm text-danger">Some status checks are unavailable. Open a section to retry.</p>}
      <ul className="mt-6 border-t border-line">
        {rows.map((row) => (
          <li key={row.title} className="border-b border-line py-5">
            <Link
              to={row.to}
              className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1 text-ink hover:text-accent"
            >
              <span className="text-base font-medium">{row.title}</span>
              <span className="text-sm text-ink-muted">{row.detail}</span>
            </Link>
          </li>
        ))}
      </ul>
    </section>
  )
}

export function GettingBooksConnections() {
  const [health, setHealth] = useState<IntegrationHealth | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [testing, setTesting] = useState<'prowlarr' | 'torznab' | 'newznab' | 'qbittorrent' | 'sabnzbd' | null>(null)
  const [testResult, setTestResult] = useState<Record<string, string>>({})

  async function check() {
    setBusy(true)
    setError(null)
    try {
      setHealth(await fetchIntegrationHealth())
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not check integrations')
    } finally {
      setBusy(false)
    }
  }

  useEffect(() => {
    void check()
  }, [])

  async function test(kind: 'prowlarr' | 'torznab' | 'newznab' | 'qbittorrent' | 'sabnzbd') {
    setTesting(kind)
    setTestResult((current) => ({ ...current, [kind]: '' }))
    try {
      const result = await ({ prowlarr: testProwlarr, torznab: testTorznab, newznab: testNewznab, qbittorrent: testQbittorrent, sabnzbd: testSabnzbd }[kind]())
      setTestResult((current) => ({
        ...current,
        [kind]: result.version ? `Connected · ${result.version}` : 'Connected',
      }))
      await check()
    } catch (caught) {
      setTestResult((current) => ({
        ...current,
        [kind]: caught instanceof ApiError ? caught.message : 'Connection test failed',
      }))
    } finally {
      setTesting(null)
    }
  }

  const connections = [
    {
      id: 'prowlarr' as const,
      name: 'Prowlarr',
      purpose: 'Find available books',
      status: health?.prowlarr,
    },
    {
      id: 'torznab' as const,
      name: 'Torznab',
      purpose: 'Find torrents through Jackett or a compatible indexer',
      status: health?.torznab,
    },
    {
      id: 'qbittorrent' as const,
      name: 'qBittorrent',
      purpose: 'Download torrents',
      status: health?.qbittorrent,
    },
    {
      id: 'newznab' as const,
      name: 'Newznab',
      purpose: 'Find Usenet releases',
      status: health?.newznab,
    },
    {
      id: 'sabnzbd' as const,
      name: 'SABnzbd',
      purpose: 'Retrieve and unpack Usenet releases',
      status: health?.sabnzbd,
    },
  ]

  return (
    <section>
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="font-display text-2xl text-ink">Connections</h2>
          <p className="mt-1 text-sm text-ink-muted">A search source finds books; a download client retrieves them.</p>
        </div>
        <Button variant="secondary" size="sm" disabled={busy} onClick={() => void check()}>
          {busy ? 'Checking…' : 'Check now'}
        </Button>
      </div>

      {error && <p className="mt-4 text-sm text-danger">{error}</p>}

      <div className="mt-5 border-t border-line">
        {connections.map((connection) => (
          <div key={connection.id} className="border-b border-line py-5">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div>
                <h3 className="text-base font-medium text-ink">{connection.name}</h3>
                <p className="text-xs text-ink-muted">{connection.purpose}</p>
                <p
                  role="status"
                  className={`mt-1 text-sm ${connection.status?.configured && !connection.status.ok ? 'text-danger' : 'text-ink-soft'}`}
                >
                  {connection.status ? connectionState(connection.status) : 'Checking…'}
                </p>
                {testResult[connection.id] && (
                  <p role="status" className="mt-1 text-xs text-ink-muted">Test: {testResult[connection.id]}</p>
                )}
              </div>
              <div className="flex items-center gap-2">
                <Link
                  to={`/settings/getting-books/${connection.id}`}
                  className="inline-flex h-8 items-center rounded-[3px] bg-surface-2 px-3.5 text-sm font-medium text-ink hover:bg-surface-3"
                >
                  Configure
                </Link>
                <Button
                  size="sm"
                  disabled={!connection.status?.configured || testing !== null}
                  onClick={() => void test(connection.id)}
                >
                  {testing === connection.id ? 'Testing…' : 'Test'}
                </Button>
              </div>
            </div>
          </div>
        ))}
      </div>

      {health && (
        <div className="mt-6" role="status" aria-live="polite">
          {health.qbittorrent.configured && health.qbittorrent.ok && (
            <HealthRow
              label="Download path"
              ok={
                health.qbittorrent.pathChecked === false
                  ? null
                  : (health.qbittorrent.missingPaths ?? 0) === 0
              }
              detail={
                health.qbittorrent.pathChecked === false
                  ? 'Could not list downloads'
                  : (health.qbittorrent.missingPaths ?? 0) === 0
                    ? `${health.qbittorrent.completed ?? 0} completed downloads reachable`
                    : `${health.qbittorrent.missingPaths} of ${health.qbittorrent.completed} paths unreachable`
              }
            >
              {health.qbittorrent.pathMessage && (
                <p
                  className={`mt-1.5 text-xs ${
                    (health.qbittorrent.missingPaths ?? 0) > 0 || health.qbittorrent.pathChecked === false
                      ? 'text-danger'
                      : 'text-ink-faint'
                  }`}
                >
                  {health.qbittorrent.pathMessage}
                </p>
              )}
              {(health.qbittorrent.pathExamples ?? []).map((example) => (
                <p key={example.path} className="mt-0.5 truncate text-xs text-ink-faint">
                  {example.name} · {example.path}
                </p>
              ))}
            </HealthRow>
          )}
          <HealthRow
            label="Library"
            ok={health.library.writable}
            detail={health.library.writable ? 'Writable' : 'Not writable'}
          >
            <p className="mt-1 truncate text-xs text-ink-faint">{health.library.path}</p>
          </HealthRow>
          <HealthRow
            label="Hardlinks"
            ok={health.hardlinks.supported}
            detail={health.hardlinks.supported ? 'Supported' : 'Falls back to copy'}
          >
            <p className="mt-1 text-xs text-ink-faint">{health.hardlinks.message}</p>
          </HealthRow>
        </div>
      )}
    </section>
  )
}

export function LibraryHealthPanel() {
  const [health, setHealth] = useState<LibraryHealth | null>(null)
  const [busy, setBusy] = useState(false)
  const [scanning, setScanning] = useState(false)
  const [notice, setNotice] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  async function check() {
    setBusy(true)
    setError(null)
    try {
      setHealth(await fetchLibraryHealth())
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not check the library')
    } finally {
      setBusy(false)
    }
  }

  useEffect(() => {
    void check()
  }, [])

  async function scan() {
    setScanning(true)
    setNotice(null)
    try {
      setNotice(await runLibraryScan())
      await check()
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not start the scan')
    } finally {
      setScanning(false)
    }
  }

  const gaps = health
    ? [
        { key: 'cover', label: 'Missing covers', count: health.missingCovers },
        { key: 'description', label: 'Missing descriptions', count: health.missingDescriptions },
        { key: 'language', label: 'Missing language', count: health.missingLanguages },
      ]
    : []

  return (
    <section className="rounded-panel bg-surface p-5 sm:p-6">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="text-base font-semibold text-ink">Library health</h2>
          <p className="mt-0.5 text-xs text-ink-faint">
            {health
              ? `${health.books} books and ${health.files} files indexed. Counts are actionable; nothing here is scored.`
              : 'Metadata and files an admin can fix. Administrators only.'}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button variant="secondary" size="sm" disabled={busy} onClick={() => void check()}>
            {busy ? 'Checking…' : 'Check now'}
          </Button>
          <Button variant="secondary" size="sm" disabled={scanning} onClick={() => void scan()}>
            {scanning ? 'Scanning…' : 'Scan library'}
          </Button>
        </div>
      </div>

      {error && (
        <p className="mt-4 rounded-card bg-surface-2 px-3.5 py-2.5 text-sm text-danger">{error}</p>
      )}
      {notice && <p className="mt-4 text-sm text-ink-soft">{notice}</p>}

      {health && (
        <div className="mt-4" role="status" aria-live="polite">
          {gaps.map((gap) => (
            <div
              key={gap.key}
              className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1 border-b border-line py-3"
            >
              <span className="text-sm text-ink-soft">{gap.label}</span>
              <span className="flex items-baseline gap-4">
                <span
                  className={`font-sans text-lg tabular-nums ${
                    gap.count > 0 ? 'text-ink' : 'text-ink-faint'
                  }`}
                >
                  {gap.count}
                </span>
                {gap.count > 0 && (
                  <Link
                    to={`/library?missing=${gap.key}`}
                    className="text-xs font-medium text-accent transition-colors hover:text-accent-strong"
                  >
                    Show books
                  </Link>
                )}
              </span>
            </div>
          ))}
          <div className="border-b border-line py-3 last:border-b-0">
            <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
              <span className="text-sm text-ink-soft">Files missing from disk</span>
              <span className="flex items-baseline gap-4">
                <span
                  className={`font-sans text-lg tabular-nums ${
                    health.missingFiles > 0 ? 'text-danger' : 'text-ink-faint'
                  }`}
                >
                  {health.missingFiles}
                </span>
              </span>
            </div>
            {health.missingFiles > 0 ? (
              health.missingFileSamples.map((sample) => (
                <p key={sample.path} className="mt-1.5 text-xs text-ink-faint">
                  <Link
                    to={`/library/${sample.bookId}`}
                    className="text-ink-soft transition-colors hover:text-accent"
                  >
                    {sample.title}
                  </Link>
                  {' · '}
                  <span className="break-all">{sample.path}</span>
                </p>
              ))
            ) : (
              <p className="mt-1.5 text-xs text-ink-faint">Every indexed file is present.</p>
            )}
          </div>
        </div>
      )}
    </section>
  )
}

export function WatchFolderStatus() {
  const [status, setStatus] = useState<Awaited<ReturnType<typeof fetchWatchStatus>> | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  async function check() {
    setBusy(true)
    try {
      setStatus(await fetchWatchStatus())
      setError(null)
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not check the import folder')
    } finally {
      setBusy(false)
    }
  }
  useEffect(() => { void check() }, [])
  return <section className="border-t border-line pt-5">
    <div className="flex flex-wrap items-center justify-between gap-3">
      <h2 className="font-display text-2xl text-ink">Import folder</h2>
      <Button variant="secondary" size="sm" disabled={busy} onClick={() => void check()}>{busy ? 'Checking…' : 'Check import folder'}</Button>
    </div>
    {status && <div className="mt-3 space-y-2 text-sm text-ink-muted" role="status">
      <p>{status.enabled ? 'Watching' : 'Watcher off'} · <span className="break-all">{status.path}</span></p>
      <p>{status.pending} pending imports · {status.cleanupPending} awaiting cleanup · {status.reviewFiles} files in review</p>
      {status.reviewFiles > 0 && <p>Check the <span className="break-all">{status.path}/review</span> folder for files that could not be imported.</p>}
      {status.lastError && <p className="text-danger">{status.lastError}</p>}
    </div>}
    {error && <p role="alert" className="mt-3 text-sm text-danger">{error}</p>}
  </section>
}

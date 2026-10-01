import { useCallback, useEffect, useState } from 'react'
import { fetchBackupStatus } from '../../api/maintenance'
import { fetchServerStatus, fetchUpdates } from '../../api/server'
import { Button } from '../../components/ui/Button'
import { SettingsFields } from './SettingsFields'
import { BackupPanel, IdentityPanel, StoragePanel } from './ServerPanels'
import { ServerUpdates } from './ServerUpdates'
import { ServerDiagnostics } from './ServerDiagnostics'
import type { Group } from './groups'

function useResource<T>(fetcher: () => Promise<T>) {
  const [data, setData] = useState<T | null>(null)
  const [error, setError] = useState(false)
  const [loading, setLoading] = useState(true)
  const load = useCallback(async () => {
    setLoading(true)
    try { setData(await fetcher()); setError(false) }
    catch { setError(true) }
    finally { setLoading(false) }
  }, [fetcher])
  useEffect(() => {
    let active = true
    void fetcher().then((result) => { if (active) { setData(result); setError(false) } })
      .catch(() => { if (active) setError(true) })
      .finally(() => { if (active) setLoading(false) })
    return () => { active = false }
  }, [fetcher])
  return { data, setData, error, loading, load }
}

export function ServerSettings({ groups, onSaved }: { groups: Group[]; onSaved: () => void }) {
  const server = useResource(fetchServerStatus)
  const backups = useResource(fetchBackupStatus)
  const updates = useResource(fetchUpdates)

  function reload() {
    void server.load()
    void backups.load()
    void updates.load()
    onSaved()
  }

  return <div className="space-y-8">
    <div className="flex flex-wrap items-center justify-between gap-3 text-xs text-ink-muted">
      <p>Status is checked when this page loads or you refresh it.</p>
      <Button size="sm" onClick={reload} disabled={server.loading || backups.loading || updates.loading}>Refresh status</Button>
    </div>
    {server.error && <p role="alert" className="text-sm text-danger">Could not load server status. Refresh to try again.</p>}
    {!server.data && !server.error && <p className="text-sm text-ink-muted">Checking server…</p>}
    {server.data && <><IdentityPanel status={server.data} /><StoragePanel status={server.data} /></>}
    <SettingsFields groups={groups} onSaved={reload} details={{
      Backups: <>
        {backups.error && <p role="alert" className="mb-4 text-sm text-danger">Could not load backup status. Refresh to try again.</p>}
        {backups.data ? <BackupPanel status={backups.data} /> : !backups.error && <p className="text-sm text-ink-muted">Checking backups…</p>}
      </>,
      Updates: <>
        {updates.error && <p role="alert" className="mb-4 text-sm text-danger">Could not load update information. Refresh to try again.</p>}
        {updates.data ? <ServerUpdates status={updates.data} installation={server.data?.build.installation} onChecked={updates.setData} /> : !updates.error && <p className="text-sm text-ink-muted">Loading update information…</p>}
      </>,
    }} />
    <ServerDiagnostics />
  </div>
}

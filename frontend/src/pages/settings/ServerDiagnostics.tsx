import { useState } from 'react'
import { fetchDiagnostics } from '../../api/server'
import { fetchLogs } from '../../api/users'
import { Button } from '../../components/ui/Button'

export function ServerDiagnostics() {
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [exportText, setExportText] = useState<string | null>(null)
  const [logs, setLogs] = useState<string | null>(null)
  const [loadingLogs, setLoadingLogs] = useState(false)

  async function copy() {
    setBusy(true)
    setMessage(null)
    setError(null)
    try {
      const value = JSON.stringify(await fetchDiagnostics(), null, 2)
      try {
        await navigator.clipboard.writeText(value)
        setExportText(null)
        setMessage('Diagnostics copied.')
      } catch {
        setExportText(value)
        setMessage('Select and copy the diagnostics below. Clipboard access is unavailable in this browser.')
      }
    } catch { setError('Could not generate diagnostics. Try again.') }
    finally { setBusy(false) }
  }

  async function toggleLogs() {
    if (logs !== null) { setLogs(null); return }
    setLoadingLogs(true)
    setError(null)
    try { setLogs((await fetchLogs()).lines.join('\n')) }
    catch { setError('Could not load server logs. Try again.') }
    finally { setLoadingLogs(false) }
  }

  return <section aria-labelledby="server-diagnostics" className="border-t border-line pt-6">
    <h2 id="server-diagnostics" className="font-display text-2xl text-ink">Diagnostics</h2>
    <p className="mt-2 max-w-2xl text-sm text-ink-muted">Copy a structured report with build details, storage and backup health, integration configuration, library counts, and safe error summaries. It excludes credentials, paths, book titles, profile details, and raw logs.</p>
    <div className="mt-5 flex flex-wrap gap-2">
      <Button onClick={() => void copy()} disabled={busy}>{busy ? 'Preparing diagnostics…' : 'Copy diagnostics'}</Button>
      <Button onClick={() => void toggleLogs()} disabled={loadingLogs} aria-expanded={logs !== null} aria-controls="server-log-output">{loadingLogs ? 'Loading logs…' : logs !== null ? 'Hide logs' : 'Show logs'}</Button>
    </div>
    {message && <p role="status" className="mt-3 text-sm text-ink-soft">{message}</p>}
    {error && <p role="alert" className="mt-3 text-sm text-danger">{error}</p>}
    {exportText && <div className="mt-4 text-sm text-ink-muted"><label htmlFor="diagnostics-report">Diagnostics report</label><textarea id="diagnostics-report" readOnly value={exportText} onFocus={(event) => event.currentTarget.select()} rows={12} className="mt-2 w-full rounded-card border border-line bg-surface-2 p-3 font-mono text-xs text-ink" /></div>}
    <div id="server-log-output">
      {logs !== null && <div className="mt-5">
        <p className="text-xs text-ink-muted">Recent raw logs may contain private paths or URLs. Review them before sharing.</p>
        <pre tabIndex={0} aria-label="Recent server logs" className="mt-3 max-h-96 overflow-y-auto whitespace-pre-wrap break-all border-l border-line pl-4 text-xs leading-relaxed text-ink-soft">{logs || 'No recent logs.'}</pre>
      </div>}
    </div>
  </section>
}

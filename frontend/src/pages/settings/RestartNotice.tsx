import { useEffect, useState } from 'react'
import { fetchRestartChanges, type RestartChange } from '../../api/server'

export function RestartNotice({ revision }: { revision: string }) {
  const [changes, setChanges] = useState<RestartChange[]>([])
  const [failed, setFailed] = useState(false)
  useEffect(() => {
    let active = true
    void fetchRestartChanges().then((result) => {
      if (active) { setChanges(result); setFailed(false) }
    }).catch(() => { if (active) setFailed(true) })
    return () => { active = false }
  }, [revision])

  if (failed) return <p role="status" className="text-sm text-ink-muted">Could not check whether a restart is required. Reload Settings to try again.</p>
  if (!changes.length) return null
  return (
    <section aria-labelledby="restart-required" className="border-l-2 border-accent pl-5">
      <h2 id="restart-required" className="font-display text-xl text-ink">Restart required</h2>
      <p className="mt-2 text-sm text-ink-soft">These saved settings differ from the running configuration:</p>
      <ul className="mt-2 list-disc space-y-1 pl-5 text-sm text-ink">
        {changes.map((change) => <li key={change.key}>{change.label}</li>)}
      </ul>
      <details className="mt-3 text-sm text-ink-soft">
        <summary className="cursor-pointer text-accent">View restart instructions</summary>
        <p className="mt-3">For Docker Compose, run <code className="break-words">docker compose restart bokhylle</code> from your installation directory, using the same Compose files as usual. For a source installation, restart the server process.</p>
        <p className="mt-2">Changes to Compose environment variables or mounts require recreating the container with <code>docker compose up -d bokhylle</code>. The notice clears after a restart or if you restore the running values.</p>
      </details>
    </section>
  )
}

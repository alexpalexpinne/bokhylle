import { useState } from 'react'
import { checkUpdates, type UpdateStatus } from '../../api/server'
import { Button, ButtonAnchor } from '../../components/ui/Button'
import { Facts } from './ServerPanels'
import { timestamp } from './serverFormat'

const descriptions: Record<UpdateStatus['state'], string> = {
  not_checked: 'Releases have not been checked yet.',
  update_available: 'A newer stable release is available.',
  up_to_date: 'You are running the latest stable release version.',
  newer_build: 'Your version is newer than the latest stable release.',
  unavailable: 'Update information is unavailable.',
  no_release: 'No stable GitHub release is available yet.',
}

export function ServerUpdates({ status, installation, onChecked }: {
  status: UpdateStatus
  installation?: string
  onChecked: (status: UpdateStatus) => void
}) {
  const [checking, setChecking] = useState(false)
  const [error, setError] = useState<string | null>(null)
  async function check() {
    setChecking(true)
    setError(null)
    try { onChecked(await checkUpdates()) }
    catch { setError('Could not check for updates. Try again later.') }
    finally { setChecking(false) }
  }
  return <div className="space-y-4">
    <p role="status" className="text-sm text-ink">{descriptions[status.state]}</p>
    <Facts items={[
      ['Current version', status.currentVersion],
      ['Automatic checks', status.automaticChecks ? 'Enabled' : 'Disabled'],
      ['Latest stable', status.latestVersion ? `${status.latestVersion}${status.state === 'unavailable' ? ' · last known release' : ''}` : 'Unavailable'],
      ['Last check', timestamp(status.checkedAt)],
      ...(status.state === 'unavailable' ? [['Last successful check', timestamp(status.lastSuccessAt)] as [string, string]] : []),
    ]} />
    {(status.error || error) && <p role="status" className="text-sm text-ink-muted">{error ?? status.error}</p>}
    <div className="flex flex-wrap gap-2">
      <Button onClick={() => void check()} disabled={checking}>{checking ? 'Checking…' : 'Check for updates'}</Button>
      {status.releaseUrl && <ButtonAnchor href={status.releaseUrl}>View release notes</ButtonAnchor>}
    </div>
    <p className="text-xs text-ink-muted">Checks are cached. Manual checks can run once a minute.</p>
    <details className="text-sm text-ink-soft">
      <summary className="cursor-pointer text-accent">How to update</summary>
      <div className="mt-4 space-y-4">
        <p>Back up the config and library directories together, then read the release notes. Keep the previous backup and version until you have checked the updated installation.</p>
        {installation === 'source' ? <p>For a source installation, check out the release you intend to run, build the frontend and Rust server, then restart the server process using your usual service manager.</p> : <>
          <div>
            <h3 className="font-medium text-ink">Published Docker image</h3>
            <p className="mt-1">Set <code>BOKHYLLE_IMAGE</code> in your installation&apos;s <code>.env</code> to the desired published version tag or digest, then run:</p>
            <pre className="mt-3 whitespace-pre-wrap break-all border-l border-line pl-4 text-xs leading-relaxed">{'docker compose -f compose.yaml -f compose.image.yaml pull bokhylle\ndocker compose -f compose.yaml -f compose.image.yaml up -d --no-build bokhylle'}</pre>
          </div>
          <div>
            <h3 className="font-medium text-ink">Docker built from source</h3>
            <p className="mt-1">Check out the intended release in your installation directory, then rebuild:</p>
            <pre className="mt-3 whitespace-pre-wrap break-all border-l border-line pl-4 text-xs">docker compose up -d --build bokhylle</pre>
          </div>
        </>}
        <p>{installation !== 'source' && 'Use the same Compose files, environment, and volumes as your installation. '}Check the server health and sign in afterward. Database migrations run forward only; restoring an older version may also require restoring its matching backup.</p>
        <a href="https://github.com/alexpalexpinne/bokhylle/blob/main/docs/operations.md#update" className="inline-block text-accent underline">Read the update guide</a>
      </div>
    </details>
  </div>
}

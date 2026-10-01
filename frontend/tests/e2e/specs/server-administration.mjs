import { expect } from '../support.mjs'
import { check } from './accessibility.mjs'
import { serverPreview } from '../../../scripts/server-preview.mjs'

export default async function serverAdministration(page, { base }) {
  const writes = []
  const fixture = serverPreview()
  let settings = {}
  let pendingRestart = []
  let updatePosts = 0
  let diagnosticsGets = 0
  let logsGets = 0
  let updateUnavailable = false
  let statusUnavailable = false
  await page.addInitScript(() => {
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: async (value) => {
      if (window.__denyClipboard) throw new Error('Clipboard denied')
      window.__copiedDiagnostics = value
    } } })
  })
  await page.route('**/api/**', async (route) => {
    const request = route.request()
    const path = new URL(request.url()).pathname
    const json = (body, status = 200) => route.fulfill({ status, json: body })
    if (path === '/api/auth/me') return json({ user: { id: 1, username: 'test_admin', displayName: 'Test Admin', role: 'admin', profileType: 'adult', preferredLanguages: ['en'] } })
    if (path === '/api/demo') return json({ enabled: false })
    if (path === '/api/notifications') return json({ items: [], unread: 0, pendingRequests: 0, pendingRequestItems: [] })
    if (path === '/api/profile/onboarding') return json({ onboarded: true, interests: [] })
    if (path === '/api/admin/server/restart') return json(pendingRestart)
    if (path === '/api/admin/server') return statusUnavailable ? json({ message: 'Storage check unavailable', code: 'unavailable' }, 503) : json(fixture.server)
    if (path === '/api/admin/maintenance/backups') return json(fixture.backups)
    if (path === '/api/admin/maintenance/images' || path === '/api/admin/maintenance/metadata') return json({ running: false, startedAt: null, finishedAt: null, failures: [], error: null })
    if (path === '/api/admin/server/updates') {
      if (request.method() === 'POST') {
        updatePosts += 1
        await new Promise((resolve) => setTimeout(resolve, 200))
      }
      return json(updateUnavailable ? { ...fixture.updates, state: 'unavailable', error: 'Could not check GitHub releases. The library remains available.' } : fixture.updates)
    }
    if (path === '/api/admin/server/diagnostics') { diagnosticsGets += 1; return json(fixture.diagnostics) }
    if (path === '/api/admin/logs') { logsGets += 1; return json({ lines: ['WARN private path /private/example and secret-url.example.test'] }) }
    if (path === '/api/admin/settings') return json({ settings, envOverrides: {}, secretsConfigured: {} })
    if (path.startsWith('/api/admin/settings/')) {
      const key = decodeURIComponent(path.slice('/api/admin/settings/'.length))
      const value = request.postDataJSON().value
      writes.push({ key, value })
      settings = { ...settings, [key]: value }
      if (key === 'backups.interval_hours') {
        fixture.backups.intervalHours = value
        fixture.backups.schedulerEnabled = value > 0
        fixture.backups.nextScheduledAt = value > 0 ? fixture.backups.lastSuccess.createdAt + value * 3600 : null
      }
      if (key === 'backups.keep') fixture.backups.keep = value
      if (key === 'metadata.provider') pendingRestart = value === 'automatic' ? [] : [{ key, label: 'Metadata provider' }]
      return json({ ok: true })
    }
    return json([])
  })

  for (const [width, theme] of [[1440, 'paper'], [390, 'paper'], [1440, 'ink'], [390, 'ink'], [320, 'paper']]) {
    await page.setViewportSize({ width, height: 1000 })
    await page.addInitScript((theme) => localStorage.setItem('bokhylle.theme', theme), theme)
    await page.goto(`${base}/settings/server`, { waitUntil: 'networkidle' })
    await page.getByRole('heading', { name: 'Diagnostics', exact: true }).waitFor()
    expect(await page.getByText('f2fe990', { exact: true }).count() === 1, 'server shows a distinct commit identity')
    expect(await page.getByRole('heading', { name: 'Config · Library · Downloads' }).count() === 1, 'shared filesystem has one capacity pool')
    expect(await page.getByLabel('Automatic backup interval (hours)').inputValue() === '24', 'backup interval displays its effective default')
    expect(await page.getByLabel('Backups to keep').inputValue() === '7', 'retention displays its effective default')
    expect(await page.getByLabel('Check GitHub releases automatically').isChecked(), 'automatic release checks default to enabled')
    await page.getByText('How to update', { exact: true }).click()
    await page.getByText('Published Docker image', { exact: true }).waitFor()
    await check(page, `server ${width} ${theme}`)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), `server has no horizontal overflow at ${width}`)
  }

  await page.setViewportSize({ width: 1440, height: 1000 })
  await page.goto(`${base}/settings/server`, { waitUntil: 'networkidle' })
  await page.getByLabel('Automatic backup interval (hours)').fill('0')
  await page.getByLabel('Backups to keep').fill('3')
  await page.getByRole('button', { name: 'Save 2 changes', exact: true }).click()
  await page.getByText('Settings saved.', { exact: true }).waitFor()
  await page.getByText('Not scheduled', { exact: true }).waitFor()
  expect(writes.some((write) => write.key === 'backups.interval_hours' && write.value === 0), 'disabled backup schedule is saved as a number')
  expect(writes.some((write) => write.key === 'backups.keep' && write.value === 3), 'retention is saved as a number')
  expect(await page.getByRole('heading', { name: 'Restart required' }).count() === 0, 'live backup settings do not require a restart')

  // A startup-bound setting must show its notice where it was changed and clear on revert.
  await page.goto(`${base}/settings/metadata`, { waitUntil: 'networkidle' })
  const metadata = page.getByLabel(/^Metadata provider/)
  await metadata.selectOption('google_books')
  await page.getByRole('button', { name: 'Save 1 change', exact: true }).click()
  await page.getByRole('heading', { name: 'Restart required' }).waitFor()
  await metadata.selectOption('automatic')
  await page.getByRole('button', { name: 'Save 1 change', exact: true }).click()
  await page.waitForFunction(() => !document.getElementById('restart-required'))

  fixture.backups.outcome = 'failed'
  fixture.backups.lastFailure = { at: fixture.backups.lastAttempt.at, summary: 'Could not complete the database backup. Check config storage and permissions.' }
  fixture.backups.latest = null
  fixture.backups.inventoryError = 'Backup directory is unavailable; check config storage and permissions'
  fixture.server.storage[0].lowSpace = true
  fixture.server.build.commit = null
  fixture.server.build.builtAt = null
  fixture.server.build.dirty = null
  updateUnavailable = true
  await page.goto(`${base}/settings/server`, { waitUntil: 'networkidle' })
  await page.getByText('Backups need attention', { exact: true }).waitFor()
  await page.getByText('Commit unavailable · working tree unknown', { exact: true }).waitFor()
  await page.getByText('Build date unavailable', { exact: true }).waitFor()
  await page.getByText('0.1.1 · last known release', { exact: true }).waitFor()
  await page.getByRole('button', { name: 'Check for updates', exact: true }).click()
  expect(await page.getByRole('button', { name: 'Checking…', exact: true }).isDisabled(), 'duplicate release checks are disabled')
  await page.getByRole('button', { name: 'Check for updates', exact: true }).waitFor()
  expect(updatePosts === 1, 'offline release awareness performs one manual request')
  expect(await page.getByText('You are running the latest stable release version.', { exact: true }).count() === 0, 'offline state never claims up to date')

  await page.getByRole('button', { name: 'Copy diagnostics', exact: true }).click()
  await page.getByText('Diagnostics copied.', { exact: true }).waitFor()
  const copied = await page.evaluate(() => window.__copiedDiagnostics)
  expect(JSON.parse(copied).formatVersion === 1 && !copied.includes('/private/'), 'clipboard contains the structured server report')
  expect(diagnosticsGets === 1 && logsGets === 0, 'diagnostics copy never fetches raw logs')
  await page.evaluate(() => { window.__denyClipboard = true })
  await page.getByRole('button', { name: 'Copy diagnostics', exact: true }).click()
  await page.getByLabel('Diagnostics report', { exact: true }).waitFor()
  expect(JSON.parse(await page.getByLabel('Diagnostics report').inputValue()).formatVersion === 1, 'HTTP or denied clipboard has a usable manual copy fallback')
  await page.getByRole('button', { name: 'Show logs', exact: true }).click()
  await page.getByLabel('Recent server logs', { exact: true }).waitFor()
  expect(logsGets === 1, 'raw logs are requested separately')
  await page.getByRole('button', { name: 'Hide logs', exact: true }).click()
  await check(page, 'server failed backups and clipboard fallback')

  statusUnavailable = true
  await page.getByRole('button', { name: 'Refresh status', exact: true }).click()
  await page.getByText('Could not load server status. Refresh to try again.', { exact: true }).waitFor()
  expect(await page.getByRole('button', { name: 'Copy diagnostics', exact: true }).isEnabled(), 'one failed status request does not disable independent tools')
}

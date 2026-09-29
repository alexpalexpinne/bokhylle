import { expect } from '../support.mjs'

/** Settings navigation preserves edits until the administrator saves or discards them. */
export default async function settings(page, { base }) {
  const saved = {}
  const writes = []
  await page.route('**/api/admin/settings', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        settings: saved,
        envOverrides: {},
        secretsConfigured: {},
      }),
    }),
  )
  await page.route('**/api/admin/settings/**', (route) => {
    const key = decodeURIComponent(new URL(route.request().url()).pathname.split('/').at(-1))
    const { value } = JSON.parse(route.request().postData() ?? '{}')
    writes.push(key)
    saved[key] = value
    return route.fulfill({ status: 204, body: '' })
  })
  await page.route('**/api/admin/integrations/status', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        prowlarr: { configured: false, ok: false, version: null, error: null },
        torznab: { configured: false, ok: false, version: null, error: null },
        newznab: { configured: false, ok: false, version: null, error: null },
        sabnzbd: { configured: false, ok: false, version: null, error: null },
        qbittorrent: { configured: false, ok: false, version: null, error: null },
        smtp: { configured: false },
        library: { path: '/tmp/books', exists: true, writable: true },
        hardlinks: { supported: true, message: 'Supported' },
      }),
    }),
  )
  await page.route('**/api/admin/library-health', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        books: 2,
        files: 2,
        missingCovers: 0,
        missingDescriptions: 0,
        missingLanguages: 0,
        missingFiles: 0,
        missingFileSamples: [],
      }),
    }),
  )

  await page.route('**/api/admin/maintenance/watch', (route) => route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ enabled: true, path: '/config/ingest', pending: 1, cleanupPending: 0, reviewFiles: 2, lastError: null }) }))

  await page.goto(`${base}/settings`, { waitUntil: 'networkidle' })
  await page.getByRole('heading', { name: 'At a glance' }).waitFor()
  const sections = page.getByRole('navigation', { name: 'Settings sections' })
  await sections.getByRole('link', { name: 'Getting books' }).click()
  await page.waitForURL(`${base}/settings/getting-books`)
  await page.getByRole('heading', { name: 'Connections' }).waitFor()
  await page.getByText('2 files in review', { exact: false }).waitFor()
  for (const [name, route] of [['Newznab', 'newznab'], ['SABnzbd', 'sabnzbd']]) {
    const card = page.getByRole('heading', { name, exact: true }).locator('..').locator('..')
    await card.getByRole('link', { name: 'Configure' }).click()
    await page.waitForURL(`${base}/settings/getting-books/${route}`)
    await page.getByRole('textbox', { name: /API endpoint|URL/ }).waitFor()
    await page.getByRole('link', { name: 'Back to Getting books' }).click()
  }
  await page.getByText('Not configured').first().waitFor()
  expect(
    (await page.getByText('Not configured').count()) >= 2,
    'unconfigured services should not be labelled broken',
  )
  const prowlarr = page.getByRole('heading', { name: 'Prowlarr' }).locator('..').locator('..')
  await prowlarr.getByRole('link', { name: 'Configure' }).click()
  await page.waitForURL(`${base}/settings/getting-books/prowlarr`)
  const urlField = page.getByRole('textbox', { name: 'URL' })
  await urlField.fill('http://example.test:9696')
  await sections.getByRole('link', { name: 'Library' }).click()
  await page.getByRole('dialog', { name: 'Discard unsaved changes?' }).waitFor()
  await page.getByRole('button', { name: 'Keep editing' }).click()
  expect(page.url().endsWith('/settings/getting-books/prowlarr'), 'Keep editing should stay in the editor')
  expect((await urlField.inputValue()) === 'http://example.test:9696', 'the edit should remain')
  await sections.getByRole('link', { name: 'Library' }).click()
  await page.getByRole('button', { name: 'Discard changes' }).click()
  await page.waitForURL(`${base}/settings/library`)

  await page.getByRole('textbox', { name: 'Library root' }).fill('/tmp/test-library')
  await page.getByRole('button', { name: 'Save 1 change' }).click()
  await page.getByText('Settings saved.').waitFor()
  expect(writes.length === 1 && writes[0] === 'library.root', `only the current section should save: ${writes}`)
  await sections.getByRole('link', { name: 'Server' }).click()
  await page.waitForURL(`${base}/settings/server`)
  await sections.getByRole('link', { name: 'Getting books' }).click()
  for (const [name, route, key] of [
    ['Newznab', 'newznab', 'integrations.newznab.url'],
    ['SABnzbd', 'sabnzbd', 'integrations.sabnzbd.url'],
  ]) {
    const card = page.getByRole('heading', { name, exact: true }).locator('..').locator('..')
    await card.getByRole('link', { name: 'Configure' }).click()
    await page.getByRole('textbox', { name: /API endpoint|URL/ }).fill(`http://${route}.example.test/api`)
    await page.getByRole('button', { name: 'Save 1 change' }).click()
    await page.getByText('Settings saved.').waitFor()
    expect(saved[key] === `http://${route}.example.test/api`, `${name} configuration should be saved`)
    await page.getByRole('link', { name: 'Back to Getting books' }).click()
  }
  await page.getByRole('checkbox', { name: /Watch an import folder/ }).check()
  await page.getByRole('textbox', { name: /Watch folder/ }).fill('/config/ingest')
  await page.getByRole('button', { name: 'Save 2 changes' }).click()
  await page.getByText('Settings saved.').waitFor()
  expect(saved['imports.watch_enabled'] === true && saved['imports.watch_folder'] === '/config/ingest', 'watch-folder configuration should be saved together')
}

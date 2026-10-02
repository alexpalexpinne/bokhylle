import axe from 'axe-core'
import { expect } from '../support.mjs'

// Fictional data shared with the documentation captures.
export async function mockVersionChoices(page) {
  const state = {
    user: { id: 88, username: 'mira', displayName: 'Mira', role: 'user', profileType: 'adult', canAcquire: true,
      preferredLanguages: ['en'], preferredFormat: 'epub', defaultBookSharing: 'private', acquisitionMode: 'automatic' },
    writes: [], acquisitions: [], failSelection: false, duplicate: false, initialStatus: 'NEEDS_SELECTION',
  }
  const book = { id: 77, title: 'Where Maps End', authors: ['Nora Vale'], authorRefs: [], language: 'en', availableLanguages: ['en'],
    description: 'A fictional journey through forgotten coastlines.', publicationYear: 2024, subjects: [], editions: [], metadataSources: [],
    series: null, seriesNumber: null, rating: null, ratingCount: null, hasCover: false, onShelf: true, preference: null,
    sharing: 'private', sharedInHousehold: false, files: [{ id: 10, format: 'epub', size: 420000, filename: 'Where Maps End.epub' }] }
  const candidates = [
    { index: 0, method: 'torrent', format: 'epub', language: 'en', sizeBytes: 4200000, seeders: 30, leechers: 2, indexer: 'Reading Room', releaseName: 'Nora.Vale.Where.Maps.End.2024.Retail.EPUB', rejected: false },
    { index: 1, method: 'nzb', format: 'pdf', language: 'en', sizeBytes: 8400000, seeders: null, indexer: 'Paper Archive', releaseName: 'Where Maps End — illustrated PDF edition', rejected: false },
    { index: 2, method: 'torrent', format: 'epub', language: 'fr', sizeBytes: 2000000, seeders: 4, indexer: 'Reading Room', releaseName: 'Where.Maps.End.French.EPUB', rejected: true },
  ]
  state.newAcquisition = (overrides = {}) => ({ id: 'maps-version', bookId: 77, bookTitle: book.title, bookAuthors: book.authors,
    status: 'NEEDS_SELECTION', askBeforeDownload: true, requestedByUserId: 88, requestedBy: 'Mira', managedByMe: false,
    progress: 0, deliveryStatus: 'NONE', errorMessage: null, keepLooking: false, retryAttempts: 0,
    createdAt: 1768471200, updatedAt: 1768471200, ...overrides })
  await page.route('**/api/**', (route) => {
    const request = route.request()
    const path = new URL(request.url()).pathname
    const json = (body, status = 200) => route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) })
    if (path === '/api/auth/me') return json({ user: state.user })
    if (path === '/api/profile' && request.method() === 'PUT') {
      Object.assign(state.user, request.postDataJSON())
      return json({ user: state.user })
    }
    if (path === '/api/profile/stats') return json({ shelf: 1, authors: 0, liked: 0, booksSent: 0 })
    if (path === '/api/profile/liked') return json({ items: [] })
    if (path === '/api/profile/hidden-subjects') return json({ hidden: [] })
    if (path.endsWith('/tokens') || path.endsWith('/agent-tokens')) return json({ tokens: [] })
    if (path === '/api/demo') return json({ enabled: false })
    if (path === '/api/notifications') return json({ items: [], unread: 0, pendingRequests: 0, pendingRequestItems: [] })
    if (path === '/api/profile/onboarding') return json({ onboarded: true, interests: [] })
    if (path === '/api/discover/book') return json({ provider: 'openlibrary', providerKey: '/works/FICTIONAL', ...book,
      status: 'NOT_IN_LIBRARY', ownedBookId: null, ownedFileId: null, coverId: null, liked: false, onShelf: false })
    if (path === '/api/discover/releases') return json({ releases: candidates.slice(0, 2) })
    if (path === '/api/discover/acquisitions' || path === '/api/books/77/acquisitions') {
      const input = request.postDataJSON()
      state.writes.push({ path, input })
      if (!state.duplicate) state.acquisitions = [state.newAcquisition({ status: state.initialStatus, askBeforeDownload: input.askBeforeDownload ?? (state.user.acquisitionMode === 'ask') })]
      return json({ id: state.acquisitions[0].id, bookId: 77, status: state.acquisitions[0].status, duplicate: state.duplicate }, 202)
    }
    if (path === '/api/acquisitions') return json(state.acquisitions)
    if (path.endsWith('/candidates')) return json(candidates)
    if (path.endsWith('/select')) {
      state.writes.push({ path, input: request.postDataJSON() })
      if (state.failSelection) { state.failSelection = false; return json({ code: 'unavailable', message: 'Source temporarily unavailable' }, 503) }
      state.acquisitions[0].status = 'QUEUED'
      return json({ id: state.acquisitions[0].id, status: 'QUEUED' }, 202)
    }
    if (path === '/api/activity/direct') return json({ items: [] })
    if (path === '/api/books/77') return json(book)
    if (path === '/api/books/77/related') return json({ series: [], author: [], similar: [] })
    if (path === '/api/delivery-targets/default') return json({ address: null, source: null })
    if (path.endsWith('/cover')) return route.fulfill({ status: 404 })
    return json([])
  })
  return state
}

export default async function versionChoices(page, { base }) {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  const state = await mockVersionChoices(page)
  const discover = `${base}/discover?provider=openlibrary&providerKey=%2Fworks%2FFICTIONAL`
  await page.goto(discover, { waitUntil: 'networkidle' })
  const check = page.getByRole('button', { name: 'Check availability', exact: true })
  if (await check.count()) await check.click()
  await page.getByText('Nora.Vale.Where.Maps.End.2024.Retail.EPUB', { exact: true }).waitFor()
  expect(await page.getByText(/30 seeders/).count() > 0, 'an acquiring adult sees torrent details')
  state.initialStatus = 'REQUESTED'
  await page.getByRole('button', { name: 'Choose a version', exact: true }).click()
  await page.getByRole('status').getByText(/You will choose before anything downloads/).waitFor()
  state.acquisitions[0].status = 'NEEDS_SELECTION'
  state.initialStatus = 'NEEDS_SELECTION'
  const dialog = page.getByRole('dialog', { name: 'Choose a version', exact: true })
  await dialog.waitFor()
  await dialog.getByText('Where Maps End — illustrated PDF edition', { exact: true }).waitFor()
  expect(state.writes[0].input.askBeforeDownload === true && state.writes[0].input.sharing === 'private', 'one-book choice sends the override and sharing intent')
  expect(state.user.acquisitionMode === 'automatic', 'one-book choice preserves the account default')
  expect(await dialog.getByRole('button', { name: 'Choose', exact: true }).nth(2).isDisabled(), 'unavailable releases cannot be selected')
  expect(await page.getByRole('button', { name: /diagnostics/i }).count() === 0, 'adult choices do not expose admin diagnostics')
  for (const theme of ['paper', 'ink']) {
    await page.evaluate((value) => { document.documentElement.dataset.theme = value }, theme)
    for (const width of [320, 390, 1440]) {
      await page.setViewportSize({ width, height: 1000 })
      await page.clock.runFor(200)
      await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))))
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), 'version choices fit the viewport')
      await page.evaluate(axe.source)
      const violations = await page.evaluate(async () => (await window.axe.run(document, { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21aa', 'wcag22aa'] } })).violations.map(({ id, nodes }) => ({ id, nodes: nodes.map(({ html, failureSummary }) => ({ html, failureSummary })) })))
      expect(violations.length === 0, `Version accessibility (${theme}, ${width}): ${JSON.stringify(violations)}`)
    }
  }
  state.failSelection = true
  await dialog.getByRole('button', { name: 'Choose', exact: true }).nth(1).click()
  await dialog.getByRole('alert').getByText('Source temporarily unavailable').waitFor()
  expect(state.acquisitions[0].status === 'NEEDS_SELECTION', 'a failed choice stays available for retry')
  await dialog.getByRole('button', { name: 'Choose', exact: true }).nth(1).click()
  await dialog.waitFor({ state: 'detached' })
  expect(state.writes.at(-1).input.index === 1, 'the selected release is sent by its candidate index')
  expect(!new URL(page.url()).searchParams.has('choose'), 'successful choice clears the chooser URL')

  await page.goto(`${base}/profile/preferences`, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: /^Ask me/ }).click()
  await page.getByRole('button', { name: 'Save preferences', exact: true }).click()
  await page.getByText('Preferences saved.', { exact: true }).waitFor()
  await page.reload({ waitUntil: 'networkidle' })
  expect(await page.getByRole('button', { name: /^Ask me/ }).getAttribute('aria-pressed') === 'true', 'an adult can save Ask me as their account default')
  await page.goto(discover, { waitUntil: 'networkidle' })
  expect(await page.getByRole('button', { name: 'Choose a version', exact: true }).count() === 0, 'Ask me uses the normal Get action')
  await page.getByRole('button', { name: 'Get for my shelf', exact: true }).click()
  await dialog.waitFor()
  expect(state.writes.at(-1).input.askBeforeDownload === undefined, 'normal Get follows the account preference')
  await dialog.getByRole('button', { name: 'Close', exact: true }).last().click()
  await dialog.waitFor({ state: 'detached' })

  state.user.acquisitionMode = 'automatic'
  await page.goto(`${base}/library/77`, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: 'Get another version', exact: true }).click()
  await dialog.waitFor()
  expect(state.writes.at(-1).path === '/api/books/77/acquisitions' && state.writes.at(-1).input.askBeforeDownload === true, 'downloaded books can request another exact version')
  expect(state.writes.at(-1).input.sharing === undefined, 'getting another version preserves sharing')
  await dialog.getByRole('button', { name: 'Close', exact: true }).last().click()

  state.duplicate = true
  state.acquisitions = [state.newAcquisition({ status: 'QUEUED', askBeforeDownload: false, requestedByUserId: 99, requestedBy: 'Alex' })]
  await page.goto(`${base}/library/77`, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: 'Get another version', exact: true }).click()
  await page.getByRole('status').getByText(/already has a shared download in progress/).waitFor()
  expect(await dialog.count() === 0 && state.acquisitions[0].askBeforeDownload === false, 'joining an active download preserves its existing choice')

  state.user.canAcquire = false
  await page.goto(`${base}/library/77`, { waitUntil: 'networkidle' })
  expect(await page.getByRole('button', { name: 'Get another version', exact: true }).count() === 0, 'adults needing approval cannot start another download')
  await page.goto(discover, { waitUntil: 'networkidle' })
  expect(await page.getByRole('button', { name: 'Choose a version', exact: true }).count() === 0, 'adults needing approval cannot choose downloads')
}

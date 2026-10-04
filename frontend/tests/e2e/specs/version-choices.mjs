import axe from 'axe-core'
import { expect } from '../support.mjs'

// Fictional data shared with the documentation captures.
export async function mockVersionChoices(page) {
  const state = {
    user: { id: 88, username: 'mira', displayName: 'Mira', role: 'user', profileType: 'adult', canAcquire: true,
      preferredLanguages: ['en'], preferredFormat: 'epub', defaultBookSharing: 'private', acquisitionMode: 'automatic' },
    writes: [], acquisitions: [], failSelection: false, duplicate: false, initialStatus: 'NEEDS_SELECTION',
    targets: [],
  }
  const book = { id: 77, title: 'Where Maps End', authors: ['Nora Vale'], authorRefs: [], language: 'en', availableLanguages: ['en'],
    description: 'A fictional journey through forgotten coastlines.', publicationYear: 2024, subjects: [], editions: [], metadataSources: [],
    series: null, seriesNumber: null, rating: null, ratingCount: null, hasCover: false, onShelf: true, preference: null,
    sharing: 'private', sharingManaged: true, sharedInHousehold: false, files: [{ id: 10, format: 'epub', size: 420000, filename: 'Where Maps End.epub' }] }
  state.book = book
  const candidates = [
    { index: 0, method: 'torrent', format: 'epub', language: 'en', sizeBytes: 4200000, seeders: 30, leechers: 2, indexer: 'Reading Room', releaseName: 'Nora.Vale.Where.Maps.End.2024.Retail.EPUB', rejected: false, recommended: true, needsReview: false },
    { index: 1, method: 'nzb', format: 'pdf', language: 'en', sizeBytes: 8400000, seeders: null, indexer: 'Paper Archive', releaseName: 'Where Maps End — illustrated PDF edition', rejected: false, recommended: false, needsReview: true },
    { index: 2, method: 'torrent', format: 'epub', language: 'fr', sizeBytes: 2000000, seeders: 4, indexer: 'Reading Room', releaseName: 'Where.Maps.End.French.EPUB', rejected: true },
  ]
  state.candidates = candidates
  state.newAcquisition = (overrides = {}) => ({ id: 'maps-version', bookId: 77, bookTitle: book.title, bookAuthors: book.authors,
    status: 'NEEDS_SELECTION', askBeforeDownload: true, requestedByUserId: 88, requestedBy: 'Mira', managedByMe: false,
    progress: 0, deliveryStatus: 'NONE', deliverOnReady: false, requestedByMe: true, scheduledDeliveryAddress: null, errorMessage: null, keepLooking: false, retryAttempts: 0,
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
    if (path === '/api/discover/releases') return json({ releases: state.candidates.filter((candidate) => !candidate.rejected).map((candidate) => ({ ...candidate, selectionKey: String.fromCharCode(97 + candidate.index).repeat(64), unavailableReason: null, isCollection: false })) })
    if (path === '/api/books/77/acquisitions' && request.method() === 'GET') return json(state.acquisitions)
    if (path === '/api/discover/acquisitions' || path === '/api/books/77/acquisitions') {
      const input = request.postDataJSON()
      state.writes.push({ path, input })
      if (input.releaseKey && state.failSelection) { state.failSelection = false; return json({ code: 'unavailable', message: 'Source temporarily unavailable' }, 503) }
      if (!state.duplicate) state.acquisitions = [state.newAcquisition({ status: input.releaseKey ? 'QUEUED' : state.initialStatus, askBeforeDownload: input.askBeforeDownload ?? (state.user.acquisitionMode === 'ask') })]
      return json({ id: state.acquisitions[0].id, bookId: 77, status: state.acquisitions[0].status, duplicate: state.duplicate }, 202)
    }
    if (path === '/api/acquisitions') return json(state.acquisitions)
    if (path === '/api/acquisitions/maps-version') return json(state.acquisitions[0])
    if (path.endsWith('/candidates')) return json(state.candidates.filter((candidate) => !candidate.rejected))
    if (path.endsWith('/select')) {
      state.writes.push({ path, input: request.postDataJSON() })
      if (state.failSelection) { state.failSelection = false; return json({ code: 'unavailable', message: 'Source temporarily unavailable' }, 503) }
      state.acquisitions[0].status = 'QUEUED'
      return json({ id: state.acquisitions[0].id, status: 'QUEUED' }, 202)
    }
    if (path === '/api/activity/direct') return json({ items: [] })
    if (path === '/api/books/77') return json(book)
    if (path === '/api/books/77/related') return json({ series: [], author: [], similar: [] })
    if (path === '/api/delivery-targets/default') return json({ address: state.targets[0]?.address ?? null, source: state.targets.length ? 'personal' : 'none', senderAddress: null })
    if (path === '/api/delivery-targets' && request.method() === 'POST') {
      const input = request.postDataJSON()
      const target = { id: 501 + state.targets.length, userId: 88, enabled: true, isDefault: state.targets.length === 0, ...input }
      state.targets.push(target)
      return json(target, 201)
    }
    if (path === '/api/delivery-targets') return json(state.targets)
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
  state.user.acquisitionMode = 'ask'
  await page.reload({ waitUntil: 'networkidle' })
  state.initialStatus = 'NEEDS_SELECTION'
  let dialog = page.getByRole('dialog', { name: 'Where Maps End', exact: true })
  await dialog.getByText('Nora.Vale.Where.Maps.End.2024.Retail.EPUB', { exact: true }).waitFor()
  expect(state.writes.length === 0, 'opening the book follows the preference without starting acquisition')
  expect(state.user.acquisitionMode === 'ask', 'Get preserves the account default')
  expect(await dialog.getByText('Where Maps End — illustrated PDF edition', { exact: true }).count() === 0, 'alternatives stay out of the compact book summary')
  expect(await dialog.getByRole('button', { name: 'Get for my shelf', exact: true }).isEnabled(), 'a confident recommendation is proposed before Get')
  await dialog.getByRole('button', { name: /^Change version/ }).click()
  let picker = page.getByRole('dialog', { name: 'Available versions', exact: true })
  await picker.getByRole('radio').first().waitFor()
  expect(await picker.getByRole('radio').count() === 2, 'rejected results are omitted')
  expect(await picker.getByRole('radio').first().isChecked(), 'the proposed version is selected in the picker')
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
  await picker.getByRole('radio').nth(1).check()
  await picker.getByRole('button', { name: 'Cancel', exact: true }).click()
  await picker.waitFor({ state: 'detached' })
  expect(await dialog.getByText('Nora.Vale.Where.Maps.End.2024.Retail.EPUB', { exact: true }).count() === 1, 'cancelling preserves the proposed version')
  await dialog.getByRole('button', { name: /^Change version/ }).click()
  picker = page.getByRole('dialog', { name: 'Available versions', exact: true })
  state.failSelection = true
  await picker.getByRole('radio').nth(1).check()
  await picker.getByRole('button', { name: 'Use this version', exact: true }).click()
  await picker.waitFor({ state: 'detached' })
  expect(state.writes.length === 0, 'choosing a possible match does not start acquisition')
  await dialog.getByText('Possible match', { exact: true }).waitFor()
  await dialog.getByRole('button', { name: 'Get for my shelf', exact: true }).click()
  await dialog.getByRole('status').getByText('Source temporarily unavailable').waitFor()
  expect(state.acquisitions.length === 0, 'a failed choice stays available for retry')
  await dialog.getByRole('button', { name: 'Get for my shelf', exact: true }).click()
  await dialog.waitFor({ state: 'detached' })
  expect(state.writes.at(-1).input.releaseKey === 'b'.repeat(64), 'Get sends the exact previewed release identity')
  expect(!new URL(page.url()).searchParams.has('choose'), 'successful choice clears the chooser URL')

  state.user.acquisitionMode = 'automatic'
  await page.goto(`${base}/profile/preferences`, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: /^Show available versions/ }).click()
  await page.getByRole('button', { name: 'Save preferences', exact: true }).click()
  await page.getByText('Preferences saved.', { exact: true }).waitFor()
  await page.reload({ waitUntil: 'networkidle' })
  expect(await page.getByRole('button', { name: /^Show available versions/ }).getAttribute('aria-pressed') === 'true', 'an adult can save Show available versions as their account default')
  await page.goto(discover, { waitUntil: 'networkidle' })
  expect(await page.getByRole('button', { name: 'Choose a version', exact: true }).count() === 0, 'the account preference uses the normal Get action')
  await dialog.waitFor()
  expect(await dialog.getByRole('button', { name: 'Get for my shelf', exact: true }).isEnabled(), 'Get uses the compact proposed version')
  await dialog.getByRole('button', { name: 'Close', exact: true }).last().click()
  await dialog.waitFor({ state: 'detached' })

  state.user.acquisitionMode = 'automatic'
  await page.goto(`${base}/library/77`, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: 'More', exact: true }).click()
  await page.getByRole('button', { name: 'Find another version', exact: true }).click()
  dialog = page.getByRole('dialog', { name: 'Available versions', exact: true })
  await dialog.waitFor()
  expect(state.writes.at(-1).path === '/api/books/77/acquisitions' && state.writes.at(-1).input.askBeforeDownload === true, 'downloaded books can request another exact version')
  expect(state.writes.at(-1).input.sharing === undefined, 'getting another version preserves sharing')
  await dialog.getByRole('button', { name: 'Close', exact: true }).last().click()

  state.duplicate = true
  state.acquisitions = [state.newAcquisition({ status: 'QUEUED', askBeforeDownload: false, requestedByUserId: 99, requestedBy: 'Alex' })]
  await page.goto(`${base}/library/77`, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: 'More', exact: true }).click()
  await page.getByRole('button', { name: 'Find another version', exact: true }).click()
  dialog = page.getByRole('dialog', { name: 'Available versions', exact: true })
  await page.getByRole('status').getByText(/already has a shared download in progress/).waitFor()
  expect(await dialog.getByRole('button', { name: 'Get this version', exact: true }).count() === 0 && state.acquisitions[0].askBeforeDownload === false, 'joining an active download preserves its existing choice')

  state.duplicate = false
  state.acquisitions = []
  state.user.acquisitionMode = 'ask'
  await page.goto(discover, { waitUntil: 'networkidle' })
  dialog = page.getByRole('dialog', { name: 'Where Maps End', exact: true })
  await dialog.getByText('Nora.Vale.Where.Maps.End.2024.Retail.EPUB', { exact: true }).waitFor()
  const beforeReaderSetup = state.writes.length
  await dialog.getByRole('button', { name: 'Get & send…', exact: true }).click()
  let readerDialog = page.getByRole('dialog', { name: 'Get & send', exact: true })
  await readerDialog.getByLabel('Reader email address', { exact: true }).waitFor()
  expect(state.writes.length === beforeReaderSetup && state.targets.length === 0, 'opening reader setup does not acquire or save a reader')
  expect(await readerDialog.getByRole('button', { name: 'Get & send', exact: true }).isDisabled(), 'reader setup waits for a destination')
  await page.keyboard.press('Escape')
  await readerDialog.waitFor({ state: 'detached' })
  await dialog.waitFor()
  expect(await dialog.getByText('Nora.Vale.Where.Maps.End.2024.Retail.EPUB', { exact: true }).count() === 1, 'closing setup preserves the selected version and book dialog')
  await dialog.getByRole('button', { name: 'Get & send…', exact: true }).click()
  readerDialog = page.getByRole('dialog', { name: 'Get & send', exact: true })
  await readerDialog.getByLabel('Reader email address', { exact: true }).fill('mira@reader.example')
  await readerDialog.getByRole('button', { name: 'Get & send', exact: true }).click()
  await readerDialog.waitFor({ state: 'detached' })
  expect(state.targets.length === 1 && state.targets[0].address === 'mira@reader.example', 'reader setup saves a reader shared with Profile')
  expect(state.writes.at(-1).input.sendToReader === true && state.writes.at(-1).input.targetId === 501 && state.writes.at(-1).input.releaseKey === 'a'.repeat(64), 'Get & send keeps both the chosen version and reader')

  const originals = state.candidates
  const beforeReview = state.writes.length
  state.acquisitions = []
  state.candidates = [...originals, ...Array.from({ length: 24 }, (_, index) => ({
    ...originals[0], index: index + 3, recommended: false,
    releaseName: `Nora.Vale.Where.Maps.End.2024.EPUB.Edition.${index + 2}`,
  }))]
  await page.goto(discover, { waitUntil: 'networkidle' })
  dialog = page.getByRole('dialog', { name: 'Where Maps End', exact: true })
  await dialog.getByRole('button', { name: /^Change version/ }).waitFor()
  expect(await dialog.getByRole('radio').count() === 0, 'many alternatives never expand the book summary')
  await dialog.getByRole('button', { name: /^Change version/ }).click()
  picker = page.getByRole('dialog', { name: 'Available versions', exact: true })
  expect(await picker.getByRole('radio').count() === 26, 'all selectable alternatives remain reachable')
  expect(await picker.locator('.overflow-y-auto').evaluate((element) => element.scrollHeight > element.clientHeight), 'long choices scroll inside the picker')
  await page.keyboard.press('Escape')
  await picker.waitFor({ state: 'detached' })
  await dialog.waitFor()
  expect(state.writes.length === beforeReview, 'opening and closing a long picker stays read-only')

  state.candidates = [{ ...originals[0], releaseName: 'Where.Maps.End.EN.EPUB', recommended: false, needsReview: true }]
  await page.goto(discover, { waitUntil: 'networkidle' })
  await dialog.getByText(/No confident match found/).waitFor()
  expect(await dialog.getByRole('button', { name: 'Get for my shelf', exact: true }).isDisabled(), 'possible matches are not proposed automatically')
  await dialog.getByRole('button', { name: 'Review possible matches', exact: true }).click()
  picker = page.getByRole('dialog', { name: 'Available versions', exact: true })
  expect(await picker.getByRole('button', { name: 'Use this version', exact: true }).isDisabled(), 'review requires an explicit selection')
  await picker.getByRole('radio').first().check()
  await picker.getByRole('button', { name: 'Use this version', exact: true }).click()
  await picker.waitFor({ state: 'detached' })
  await dialog.getByText('Possible match', { exact: true }).waitFor()
  expect(await dialog.getByRole('button', { name: 'Get for my shelf', exact: true }).isEnabled() && state.writes.length === beforeReview, 'a deliberate reviewed choice enables Get without downloading')

  state.candidates = []
  await page.goto(discover, { waitUntil: 'networkidle' })
  await dialog.getByText('No matching version found.', { exact: true }).waitFor()
  expect(await dialog.getByRole('button', { name: 'Get for my shelf', exact: true }).isDisabled(), 'no results leaves Get unavailable')
  state.candidates = originals

  state.user.canAcquire = false
  await page.goto(`${base}/library/77`, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: 'More', exact: true }).click()
  expect(await page.getByRole('button', { name: 'Find another version', exact: true }).count() === 0, 'adults needing approval cannot start another download')
  await page.goto(discover, { waitUntil: 'networkidle' })
  expect(await page.getByRole('button', { name: 'Choose a version', exact: true }).count() === 0, 'adults needing approval cannot choose downloads')
}

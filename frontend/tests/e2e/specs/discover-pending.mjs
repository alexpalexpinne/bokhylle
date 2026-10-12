import { expect } from '../support.mjs'
import { check } from './accessibility.mjs'
import { mockVersionChoices } from './version-choices.mjs'

export default async function discoverPending(page, { base }) {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  const state = await mockVersionChoices(page)
  state.user.acquisitionMode = 'ask'
  state.book.files = []
  state.book.onShelf = false
  state.book.series = 'Fictional Coastlines'
  state.book.seriesNumber = '2'
  state.book.editions = [{ id: 700, title: state.book.title, language: 'en', publicationYear: 2024, isbn13: '9780000000002', isbn10: null, publisher: 'Imagined Press', metadataSources: [], unknown: false }]
  state.targets = [{ id: 501, name: 'Mira’s reader', type: 'kindle', address: 'mira@reader.example', enabled: true, isDefault: true }]
  const acquisition = state.newAcquisition({ status: 'SEARCHING', requestedByUserId: 99, requestedBy: 'Robin', requestedByMe: false })
  state.acquisitions = [acquisition]
  let joined = 0
  let catalogueReads = 0
  let catalogueDescription = 'The full fictional catalogue description.'
  await page.route('**/api/discover**', (route) => {
    const url = new URL(route.request().url())
    if (url.pathname === '/api/discover') return route.fulfill({ json: {
      query: 'maps', authors: { local: [], external: [] }, next: null,
      local: url.searchParams.get('source') === 'local', provider: 'openlibrary',
      books: [{ ...state.book, provider: 'openlibrary', providerKey: '/works/FICTIONAL', year: 2024,
        languages: ['en'], status: 'DOWNLOADING', ownedBookId: 77, ownedFileId: null, onShelf: false, coverId: null }],
    } })
    if (url.pathname === '/api/discover/book') {
      catalogueReads += 1
      return route.fulfill({ json: { ...state.book, description: catalogueDescription, provider: 'openlibrary', providerKey: '/works/FICTIONAL',
        status: 'DOWNLOADING', ownedBookId: 77, ownedFileId: null, languages: ['en'], onShelf: state.book.onShelf } })
    }
    return route.fallback()
  })
  await page.route('**/api/books/77/acquisitions', (route) => {
    if (route.request().method() === 'GET') return route.fulfill({ json: state.acquisitions })
    joined += 1
    acquisition.requestedByMe = true
    state.book.onShelf = true
    return route.fulfill({ status: 202, json: { id: acquisition.id, bookId: 77, status: acquisition.status, duplicate: true } })
  })
  await page.route('**/api/acquisitions/maps-version/delivery', (route) => {
    acquisition.deliverOnReady = true
    acquisition.scheduledDeliveryAddress = 'mira@reader.example'
    return route.fulfill({ json: acquisition })
  })

  async function openFromSearch() {
    await page.goto(`${base}/discover?q=maps`, { waitUntil: 'networkidle' })
    await page.getByRole('button', { name: 'Open details for Where Maps End', exact: true }).click()
    return page.getByRole('dialog', { name: 'Where Maps End', exact: true })
  }
  let dialog = await openFromSearch()
  await dialog.getByText('Finding a suitable file…', { exact: true }).waitFor()
  await dialog.getByText('A fictional journey through forgotten coastlines.', { exact: true }).waitFor()
  await dialog.getByText('Imagined Press', { exact: true }).waitFor()
  await dialog.getByText('Fictional Coastlines · Book 2', { exact: true }).waitFor()
  expect(catalogueReads === 0, 'complete local details do not wait for an external metadata provider')
  expect(await dialog.getByRole('link', { name: 'View book', exact: true }).getAttribute('href') === '/library/77', 'pending downloads keep a visible book-page action')
  for (const theme of ['paper', 'ink']) {
    await page.evaluate((value) => { document.documentElement.dataset.theme = value }, theme)
    for (const width of [320, 390, 1440]) {
      await page.setViewportSize({ width, height: 844 })
      await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))))
      expect(await dialog.evaluate((element) => element.scrollWidth <= element.clientWidth + 1), 'pending book details fit the dialog')
      await check(page, `Pending Discover (${theme}, ${width})`)
    }
  }
  await dialog.getByRole('button', { name: 'Get for my shelf', exact: true }).click()
  await dialog.getByRole('button', { name: 'Send when ready', exact: true }).waitFor()
  expect(joined === 1 && acquisition.requestedByUserId === 99, 'joining the retry adds the current profile without changing the original requester')
  await dialog.getByRole('button', { name: 'Send when ready', exact: true }).click()
  const send = page.getByRole('dialog', { name: 'Send when ready', exact: true })
  await send.getByRole('button', { name: 'Send when ready', exact: true }).click()
  await send.getByText(/Scheduled. The book will be sent to/).waitFor()
  await page.keyboard.press('Escape')
  await send.waitFor({ state: 'detached' })
  await dialog.getByRole('button', { name: 'Will send to mira@reader.example when ready.', exact: true }).waitFor()

  state.book.description = null
  dialog = await openFromSearch()
  await dialog.getByText(catalogueDescription, { exact: true }).waitFor()
  expect(catalogueReads === 1, 'a sparse pending local record is supplemented with catalogue details')
  await dialog.getByText('Finding a suitable file…', { exact: true }).waitFor()

  acquisition.status = 'READY'
  state.book.files = [{ id: 10, format: 'epub', size: 420000, filename: 'Where Maps End.epub' }]
  await dialog.getByRole('button', { name: 'Send to My Reader', exact: true }).waitFor({ timeout: 5000 })
  expect(joined === 1, 'completion replaces pending actions with ready actions without starting another download')
}

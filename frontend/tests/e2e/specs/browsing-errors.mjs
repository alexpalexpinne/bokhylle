import { expect } from '../support.mjs'
import { check } from './accessibility.mjs'

const books = Array.from({ length: 48 }, (_, index) => ({ id: 9600 + index, title: `Fictional Retry ${index + 1}`, authors: ['Invented Writer'], hasCover: false, language: 'en' }))
const suggestion = { bookId: null, title: 'Fictional Retry Suggestion', authors: ['Invented Writer'], source: 'discover', ownership: 'discover', reasonType: 'taste', reasonLabel: 'Matches your interests: Fantasy', subjects: ['Fantasy'], provider: 'fake', providerKey: 'retry-suggestion', recommendationKey: '9'.repeat(64), cta: 'discover' }
const catalogueBook = { provider: 'fake', providerKey: 'retry-search', title: 'Fictional Search Result', authors: ['Invented Writer'], language: 'en', languages: ['en'], status: 'NOT_IN_LIBRARY', ownedBookId: null, ownedFileId: null, onShelf: false, year: null }

export default async function browsingErrors(page, { base }) {
  let releaseInitial
  const initial = new Promise((resolve) => { releaseInitial = resolve })
  let firstRecommendations = true
  let failRefresh = false
  let failLibrary = true
  let failLibraryMore = true
  let libraryMoreFailures = 0
  let failSearchMore = true
  let failHomeRefresh = true
  await page.route('**/api/**', async (route) => {
    const url = new URL(route.request().url())
    const path = url.pathname
    const error = (message) => route.fulfill({ status: 503, json: { code: 'unavailable', message } })
    let body = []
    if (path === '/api/auth/me') body = { user: { id: 9595, username: 'fictional-reader', role: 'user', profileType: 'adult', preferredLanguages: [], spotlightRotation: false } }
    else if (path === '/api/demo') body = { enabled: false }
    else if (path === '/api/profile/onboarding') body = { onboarded: true, interests: ['fantasy'] }
    else if (path === '/api/notifications') body = { items: [], unread: 0, pendingRequests: 0, pendingRequestItems: [] }
    else if (path === '/api/recommendations') {
      if (firstRecommendations) { firstRecommendations = false; await initial; return error('Suggestions temporarily unavailable') }
      if (failRefresh && url.searchParams.get('cachedOnly') === 'false') return error('Catalogue temporarily unavailable')
      body = { items: [suggestion], subjects: ['fantasy'], total: 1, nextOffset: null }
    } else if (path === '/api/books') {
      if (failLibrary) { failLibrary = false; return error('Library temporarily unavailable') }
      const number = Number(url.searchParams.get('page') ?? 1)
      if (number === 2 && failLibraryMore) { failLibraryMore = false; libraryMoreFailures += 1; return error('Next page temporarily unavailable') }
      body = { items: books.slice((number - 1) * 24, number * 24), total: 48, page: number, pageSize: 24, letters: [] }
    } else if (path === '/api/books/facets') body = { formats: [], languages: [], subjects: [], series: [], publicationKinds: [] }
    else if (path === '/api/discover') {
      const local = url.searchParams.get('source') === 'local'
      if (url.searchParams.has('continuation')) {
        if (failSearchMore) { failSearchMore = false; return error('Online next page unavailable') }
        body = { books: [{ ...catalogueBook, providerKey: 'another-retry-search', title: 'Another Fictional Search Result' }], authors: { local: [], external: [] }, next: null }
      } else body = { books: local ? [] : [catalogueBook], authors: { local: [], external: [] }, next: local ? null : 'fictional-continuation' }
    } else if (path === '/api/home/spotlight') {
      if (url.searchParams.get('cachedOnly') === 'false' && failHomeRefresh) { failHomeRefresh = false; return error('Home catalogue unavailable') }
      body = { items: [], recommendations: [suggestion] }
    } else if (path === '/api/home/updates') body = { library: [], ready: [], discoveries: [] }
    else if (path === '/api/home/subjects') body = { hidden: [] }
    await route.fulfill({ json: body })
  })

  await page.goto(`${base}/recommendations`, { waitUntil: 'domcontentloaded' })
  await page.getByText('Finding books for you…', { exact: true }).waitFor()
  expect(await page.locator('.shelf-grid-cell').count() === 6, 'recommendations reserve shelf geometry while the first request is pending')
  expect(await page.getByText('No suggestions yet', { exact: true }).count() === 0, 'loading must not present an empty-state explanation')
  releaseInitial()
  await page.getByRole('alert').filter({ hasText: 'Suggestions temporarily unavailable' }).waitFor()
  await page.getByRole('button', { name: 'Try again', exact: true }).click()
  await page.getByRole('heading', { name: suggestion.title, exact: true }).waitFor()
  expect(await page.getByRole('alert').count() === 0, 'successful retry clears the first-load error')

  failRefresh = true
  await page.goto(`${base}/recommendations?subject=fantasy`, { waitUntil: 'networkidle' })
  await page.getByRole('alert').filter({ hasText: 'Catalogue temporarily unavailable' }).waitFor()
  expect(await page.getByRole('heading', { name: suggestion.title, exact: true }).count() === 1, 'cached suggestions remain visible when a catalogue refresh fails')
  expect(await page.getByLabel('Reading interest').inputValue() === 'fantasy', 'a failed refresh retains the selected interest')
  failRefresh = false
  await page.getByRole('button', { name: 'Try again', exact: true }).click()
  await page.getByRole('alert').waitFor({ state: 'detached' })
  await check(page, 'recommendations after retry')

  await page.getByRole('link', { name: 'Library', exact: true }).click()
  await page.getByRole('alert').filter({ hasText: 'Library temporarily unavailable' }).waitFor()
  expect(await page.getByText('Nothing here yet.', { exact: true }).count() === 0, 'a failed library request must not claim the shelf is empty')
  await page.getByRole('button', { name: 'Try again', exact: true }).click()
  await page.getByRole('heading', { name: 'Fictional Retry 1', exact: true }).waitFor()
  await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight))
  await page.getByRole('alert').filter({ hasText: 'Could not load more books' }).waitFor()
  expect(await page.locator('.shelf-grid-cell').count() === 24, 'a next-page failure preserves the first library page')
  await page.waitForTimeout(200)
  expect(libraryMoreFailures === 1, 'a failed next page does not start an automatic retry loop')
  await page.getByRole('button', { name: 'Try again', exact: true }).click()
  await page.waitForFunction(() => document.querySelectorAll('.shelf-grid-cell').length === 48)
  expect(await page.getByRole('heading', { name: 'Fictional Retry 40', exact: true }).count() === 1, 'a successful retry appends each book once')

  await page.goto(`${base}/discover?q=fictional&type=any`, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: 'Load more results', exact: true }).click()
  await page.getByRole('alert').filter({ hasText: 'Could not load more results' }).waitFor()
  expect(await page.getByRole('heading', { name: catalogueBook.title, exact: true }).count() === 1, 'a failed online page preserves search results')
  await page.getByRole('button', { name: 'Try again', exact: true }).click()
  await page.getByRole('heading', { name: 'Another Fictional Search Result', exact: true }).waitFor()
  expect(await page.getByRole('heading', { name: catalogueBook.title, exact: true }).count() === 1, 'the retried online page appends to existing results')

  await page.getByRole('link', { name: 'Home', exact: true }).click()
  await page.getByRole('alert').filter({ hasText: 'Catalogue suggestions could not be refreshed' }).waitFor()
  expect(await page.getByRole('heading', { name: suggestion.title, exact: true }).count() === 1, 'Home retains cached books after refresh failure')
  await page.getByRole('button', { name: 'Try again', exact: true }).click()
  await page.getByRole('alert').waitFor({ state: 'detached' })
}

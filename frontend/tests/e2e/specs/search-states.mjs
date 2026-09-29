import { expect } from '../support.mjs'

const BOOK = {
  provider: 'openlibrary', providerKey: '/works/OLDELAYED', title: 'Delayed Book',
  authors: ['Delayed Author'], year: 2020, language: 'en', isbn10: null, isbn13: null,
  series: null, seriesNumber: null, coverId: null, status: 'NOT_IN_LIBRARY',
  ownedBookId: null, ownedFileId: null, onShelf: false,
}
const AUTHOR = {
  authorId: 77, name: 'Delayed Author', following: false, bookCount: 0,
  provider: null, providerKey: null,
}

export default async function searchStates(page, { base }) {
  let releaseCatalogue
  const catalogueReady = new Promise((resolve) => { releaseCatalogue = resolve })
  await page.route('**/api/**', async (route) => {
    const url = new URL(route.request().url())
    const path = url.pathname
    let body
    if (path === '/api/auth/me') {
      body = { user: {
        id: 4343, username: 'test', displayName: 'Test Reader', role: 'user',
        profileType: 'adult', preferredFormat: 'epub', preferredLanguage: 'en',
        preferredLanguages: ['en'], defaultLanguage: 'en', acquisitionMode: 'automatic',
        notificationEmail: null, emailNotifications: false,
      } }
    } else if (path === '/api/discover') {
      const local = url.searchParams.get('source') === 'local'
      if (!local) await new Promise((resolve) => setTimeout(resolve, 700))
      body = { query: 'delayed', authors: { local: [], external: local ? [] : [AUTHOR] },
        books: local ? [] : [BOOK], next: null, local, provider: local ? null : 'openlibrary' }
    } else if (path === '/api/books') {
      await new Promise((resolve) => setTimeout(resolve, 700))
      body = { items: [], total: 0, page: 1, pageSize: 24, letters: [] }
    } else if (path === '/api/books/search') {
      await new Promise((resolve) => setTimeout(resolve, 700))
      body = []
    } else if (path === '/api/authors/77') {
      body = { id: 77, name: 'Delayed Author', books: [] }
    } else if (path === '/api/authors/77/profile') {
      body = { bio: 'A concise biography.', birthDate: '1900', deathDate: null,
        sourceUrl: 'https://openlibrary.org/authors/OL77A' }
    } else if (path === '/api/authors/77/catalogue') {
      await catalogueReady
      body = { items: [], next: null }
    } else if (path === '/api/authors/77/follow') {
      body = { following: false, autoAcquire: false, deliveryTargetId: null }
    } else {
      body = []
    }
    return route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(body) })
  })

  await page.goto(`${base}/discover?q=delayed&type=any`, { waitUntil: 'domcontentloaded' })
  await page.getByText('Searching books and authors…').waitFor()
  expect((await page.getByText('No books found').count()) === 0, 'empty message must wait for the full search')
  await page.getByText(BOOK.title).first().waitFor()
  const resultsTop = (await page.getByText('Results', { exact: true }).boundingBox()).y
  const authorsTop = (await page.getByText('Authors', { exact: true }).boundingBox()).y
  expect(resultsTop < authorsTop, 'books should lead an Anywhere search')

  await page.getByRole('link', { name: 'Delayed Author' }).first().click()
  await page.waitForURL(`${base}/authors/77`)
  await page.getByRole('link', { name: 'Back to Discover' }).waitFor()
  await page.getByText('No books by this author are in your household library yet.').waitFor()
  await page.getByText('A concise biography.').waitFor()
  expect(await page.getByRole('link', { name: 'Author information from Open Library' }).getAttribute('href') ===
    'https://openlibrary.org/authors/OL77A', 'author biography should link to its source')
  await page.getByText('Looking for more books by this author…').waitFor()
  const catalogueResponse = page.waitForResponse((response) => new URL(response.url()).pathname === '/api/authors/77/catalogue')
  releaseCatalogue()
  await catalogueResponse
  await page.getByRole('link', { name: 'Back to Discover' }).click()
  await page.waitForURL((url) => url.pathname === '/discover' && url.searchParams.get('q') === 'delayed')

  await page.goto(`${base}/library`, { waitUntil: 'domcontentloaded' })
  await page.getByText('Loading your library…').waitFor()
  expect((await page.locator('.animate-pulse').count()) === 0, 'an unknown or empty shelf must not show six ghost books')
  await page.getByText('Nothing here yet.').waitFor()
  await page.getByRole('textbox', { name: 'Search your shelf' }).fill('missing')
  await page.getByText('Searching your library…').waitFor()
  expect((await page.locator('.animate-pulse').count()) === 0, 'empty-shelf search must not show ghost books')
  await page.getByText('Nothing here yet.').waitFor()
}

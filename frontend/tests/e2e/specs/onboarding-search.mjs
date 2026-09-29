import { expect } from '../support.mjs'

const USER = {
  id: 1, username: 'reader', displayName: 'Reader', role: 'user', profileType: 'adult',
  preferredLanguages: ['en'], preferredLanguage: 'en', defaultLanguage: 'en',
}

export default async function onboardingSearch(page, { base }) {
  let release
  let pending = Promise.resolve()
  await page.route('**/api/**', async (route) => {
    const url = new URL(route.request().url())
    const path = url.pathname
    const delayed = url.searchParams.get('q')?.startsWith('delayed')
    if (delayed) await pending
    let body = []
    if (path === '/api/auth/me' || path === '/api/profile') body = { user: USER }
    else if (path === '/api/demo') body = { enabled: false }
    else if (path === '/api/books') body = { items: [], total: 0, page: 1, pageSize: 1, letters: [] }
    else if (path === '/api/delivery-targets/default') body = { address: null, senderAddress: null, amazonUrl: '' }
    else if (path === '/api/discover/authors') {
      body = { local: [{ authorId: 1, name: delayed ? 'Delayed Author' : 'Current Author', following: false, bookCount: 0 }], external: [] }
    } else if (path === '/api/books/search') {
      body = [{ id: 1, title: delayed ? 'Delayed Shelf Book' : 'Current Shelf Book', authors: [], language: 'en', hasCover: false }]
    } else if (path === '/api/discover/search') {
      body = [{ provider: 'openlibrary', providerKey: '/works/OLTEST', title: delayed ? 'Delayed Catalogue Book' : 'Current Catalogue Book', authors: [], coverId: null, status: 'NOT_IN_LIBRARY' }]
    }
    return route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(body) })
  })

  async function discardLateResponses(input, paths, replacement, staleTitles, currentTitles = []) {
    pending = new Promise((resolve) => { release = resolve })
    const requests = paths.map((path) => page.waitForRequest((request) => new URL(request.url()).pathname === path && new URL(request.url()).searchParams.get('q') === 'delayed query'))
    await input.fill('delayed query')
    await Promise.all(requests)
    await input.fill(replacement)
    for (const title of currentTitles) await page.getByText(title, { exact: true }).waitFor()
    const responses = paths.map((path) => page.waitForResponse((response) => new URL(response.url()).pathname === path && new URL(response.url()).searchParams.get('q') === 'delayed query'))
    release()
    await Promise.all(responses)
    // Wait for response callbacks and React's render before inspecting the UI.
    await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))))
    for (const title of staleTitles) {
      expect(await page.getByText(title, { exact: true }).count() === 0, `late search results must be discarded after changing the query to ${JSON.stringify(replacement)}`)
    }
    for (const title of currentTitles) expect(await page.getByText(title, { exact: true }).count() === 1, 'the current query must keep its results')
  }

  await page.goto(`${base}/welcome`, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: 'Next', exact: true }).click()
  await page.getByRole('button', { name: 'Next', exact: true }).click()
  const authors = page.getByRole('textbox', { name: 'Search authors' })
  for (const replacement of ['', 'ab', 'current author']) {
    await discardLateResponses(authors, ['/api/discover/authors'], replacement, ['Delayed Author'], replacement.length >= 3 ? ['Current Author'] : [])
  }

  await page.getByRole('button', { name: 'Next', exact: true }).click()
  const books = page.getByRole('textbox', { name: 'Search books' })
  for (const replacement of ['', 'ab', 'current book']) {
    await discardLateResponses(books, ['/api/books/search', '/api/discover/search'], replacement, ['Delayed Shelf Book', 'Delayed Catalogue Book'], replacement.length >= 3 ? ['Current Shelf Book', 'Current Catalogue Book'] : [])
  }
}

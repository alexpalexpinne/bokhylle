import { expect } from '../support.mjs'

const BOOK = {
  provider: 'fake', providerKey: 'current', title: 'Current Fictional Book',
  authors: ['Fictional Author'], language: 'en', languages: ['en'],
  coverId: null, status: 'NOT_IN_LIBRARY', ownedBookId: null, onShelf: false,
}

export default async function searchCancellation(page, { base }) {
  // A controllable fetch transport observes the real AbortSignals passed by
  // the API client. Held responses let each race run without timer guesses.
  await page.addInitScript(() => {
    const original = window.fetch.bind(window)
    const pending = new Map()
    window.searchRequests = []
    window.completeSearch = (id, payload) => pending.get(id)?.(payload)
    window.fetch = (input, options = {}) => {
      const url = new URL(typeof input === 'string' ? input : input.url, location.href)
      if (url.pathname !== '/api/discover') return original(input, options)
      const record = {
        id: window.searchRequests.length,
        query: url.searchParams.get('q'),
        type: url.searchParams.get('type'),
        local: url.searchParams.get('source') === 'local',
        cursor: url.searchParams.get('continuation'),
        aborted: false,
        completed: false,
      }
      window.searchRequests.push(record)
      return new Promise((resolve, reject) => {
        const abort = () => {
          record.aborted = true
          pending.delete(record.id)
          reject(new DOMException('Search cancelled', 'AbortError'))
        }
        if (options.signal?.aborted) { abort(); return }
        options.signal?.addEventListener('abort', abort, { once: true })
        pending.set(record.id, (payload) => {
          record.completed = true
          options.signal?.removeEventListener('abort', abort)
          pending.delete(record.id)
          resolve(new Response(JSON.stringify(payload), { headers: { 'Content-Type': 'application/json' } }))
        })
      })
    }
  })
  await page.route('**/api/**', (route) => {
    const path = new URL(route.request().url()).pathname
    let body = []
    if (path === '/api/auth/me') body = { user: {
      id: 7331, username: 'reader', role: 'user', profileType: 'adult',
      preferredLanguages: ['en'], preferredLanguage: 'en', defaultLanguage: 'en',
    } }
    else if (path === '/api/demo') body = { enabled: false }
    else if (path === '/api/books') body = { items: [], total: 0, page: 1, pageSize: 24, letters: [] }
    else if (path === '/api/books/facets') body = { formats: [], languages: [], subjects: [], series: [], publicationKinds: [] }
    return route.fulfill({ json: body })
  })

  async function waitForSearch(query, count = 2) {
    await page.waitForFunction(({ query, count }) =>
      window.searchRequests.filter((request) => request.query === query && !request.cursor).length >= count,
    { query, count })
  }
  async function complete(query, local, books = [], next = null, cursor = null) {
    await page.evaluate(({ query, local, books, next, cursor }) => {
      const request = [...window.searchRequests].reverse().find((request) =>
        request.query === query && request.local === local && request.cursor === cursor && !request.aborted && !request.completed)
      if (!request) throw new Error(`No pending search for ${query}`)
      window.completeSearch(request.id, { books, next, authors: { local: [], external: [] } })
    }, { query, local, books, next, cursor })
  }
  async function waitForAbort(query, cursor = null) {
    await page.waitForFunction(({ query, cursor }) => {
      const requests = window.searchRequests.filter((request) => request.query === query && request.cursor === cursor && !request.completed)
      return requests.length > 0 && requests.every((request) => request.aborted)
    }, { query, cursor })
  }

  await page.goto(`${base}/discover?q=held-initial&type=any`, { waitUntil: 'domcontentloaded' })
  const input = page.getByRole('textbox', { name: 'Search books' })
  await waitForSearch('held-initial')
  await input.fill('current')
  await waitForAbort('held-initial')
  await waitForSearch('current')
  await complete('current', true)
  await complete('current', false, [BOOK])
  await page.getByRole('heading', { name: BOOK.title, exact: true }).waitFor()

  await input.fill('held-mode')
  await waitForSearch('held-mode')
  await page.getByRole('button', { name: 'Search all languages', exact: true }).click()
  await page.waitForFunction(() => window.searchRequests.filter((request) => request.query === 'held-mode').slice(0, 2).every((request) => request.aborted))
  await waitForSearch('held-mode', 4)
  await page.getByRole('button', { name: 'Advanced', exact: true }).click()
  await page.getByRole('button', { name: 'Title', exact: true }).click()
  await page.waitForFunction(() => window.searchRequests.filter((request) => request.query === 'held-mode').slice(2, 4).every((request) => request.aborted))
  await waitForSearch('held-mode', 6)
  expect(await page.evaluate(() => window.searchRequests.filter((request) => request.query === 'held-mode').slice(-2).every((request) => request.type === 'title')),
    'changing the search type must start requests for the selected type')
  await input.fill('')
  await waitForAbort('held-mode')
  await page.getByRole('button', { name: 'Anywhere', exact: true }).click()
  await page.getByRole('button', { name: 'Preferred only', exact: true }).click()
  expect(await page.getByText('Search failed', { exact: true }).count() === 0, 'cancellation must not show a search error')

  await input.fill('held-backfill')
  await waitForSearch('held-backfill')
  await complete('held-backfill', true)
  await complete('held-backfill', false, [], 'page-1')
  await page.waitForFunction(() => window.searchRequests.some((request) => request.query === 'held-backfill' && request.cursor === 'page-1'))
  await input.fill('current') // restoring a cached search must cancel backfill too
  await waitForAbort('held-backfill', 'page-1')
  await page.getByRole('heading', { name: BOOK.title, exact: true }).waitFor()
  expect(await page.evaluate(() => !window.searchRequests.some((request) => request.query === 'held-backfill' && request.cursor !== null && request.cursor !== 'page-1')),
    'a cancelled search must not fetch more pages')

  await input.fill('held-backfill')
  await waitForSearch('held-backfill', 4)
  await input.fill('')
  await waitForAbort('held-backfill')
  expect(await page.getByText('Searching books and authors…').count() === 0, 'clearing a search must stop loading')

  await input.fill('It')
  await input.press('Enter')
  await waitForSearch('It')
  await complete('It', true)
  await complete('It', false, [{ ...BOOK, providerKey: 'it', title: 'It' }])
  await page.getByRole('heading', { name: 'It', exact: true }).waitFor()
  await page.getByRole('link', { name: 'Library', exact: true }).click()
  await page.getByRole('link', { name: 'Discover', exact: true }).click()
  await page.getByRole('heading', { name: 'It', exact: true }).waitFor()
  expect(await input.inputValue() === 'It', 'a short submitted title must survive session restoration')

  await input.fill('paginated')
  await waitForSearch('paginated')
  await complete('paginated', true)
  await complete('paginated', false, Array.from({ length: 18 }, (_, index) =>
    ({ ...BOOK, providerKey: `page-${index}`, title: `Fictional Page Book ${index}` })), 'manual-page')
  await page.getByRole('button', { name: 'Load more results', exact: true }).click()
  await page.waitForFunction(() => window.searchRequests.some((request) => request.cursor === 'manual-page'))
  await input.fill('current')
  await waitForAbort('paginated', 'manual-page')
  await page.getByRole('heading', { name: BOOK.title, exact: true }).waitFor()

  await input.fill('held-unmount')
  await waitForSearch('held-unmount')
  await page.getByRole('link', { name: 'Library', exact: true }).click()
  await waitForAbort('held-unmount')
  expect(await page.getByText('Search failed', { exact: true }).count() === 0, 'cancellation must not show a search error')
}

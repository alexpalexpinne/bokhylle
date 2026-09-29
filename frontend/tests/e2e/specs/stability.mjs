function expect(condition, message) {
  if (!condition) {
    throw new Error(message)
  }
}

const BOOK_ID = 9501

const BOOK = {
  id: BOOK_ID,
  title: 'Stable Home Book',
  authors: ['Stable Author'],
  language: 'en',
  series: null,
  seriesNumber: null,
  hasCover: false,
  hasDescription: true,
  rating: null,
  ratingCount: null,
  ratingSource: null,
  addedAt: 1,
}

const SPOTLIGHT = {
  source: 'shelf',
  ownership: 'shelf',
  reasonType: 'because_liked',
  reasonLabel: 'Because you liked X',
  title: 'Spotlight Anchor',
  authors: ['Stable Author'],
  blurb:
    'A long enough blurb that the hero presents real copy instead of a placeholder string.',
  language: 'en',
  subjects: ['Fiction'],
  bookId: BOOK_ID,
  cta: 'explore',
}

const HARRY_BOOKS = [
  'Harry Potter and the Philosopher’s Stone',
  'Harry Potter and the Chamber of Secrets',
  'Harry Potter and the Prisoner of Azkaban',
].map((title, index) => ({
  provider: 'openlibrary',
  providerKey: `/works/OLHP${index}`,
  title,
  authors: ['J. K. Rowling'],
  year: 1997 + index,
  language: 'en',
  isbn10: null,
  isbn13: null,
  series: null,
  seriesNumber: null,
  coverId: null,
  status: 'NOT_IN_LIBRARY',
  ownedBookId: null,
  ownedFileId: null,
  onShelf: false,
}))

const KING_BOOK = {
  provider: 'openlibrary',
  providerKey: '/works/OLKING1',
  title: 'The Shining',
  authors: ['Stephen King'],
  year: 1977,
  language: 'en',
  isbn10: null,
  isbn13: null,
  series: null,
  seriesNumber: null,
  coverId: null,
  status: 'NOT_IN_LIBRARY',
  ownedBookId: null,
  ownedFileId: null,
  onShelf: false,
}

const EXTERNAL_HARRY_AUTHOR = {
  authorId: null,
  name: 'Harry Potter',
  following: false,
  bookCount: 0,
  provider: 'openlibrary',
  providerKey: '/authors/OLHARRYA',
}

const DERIVED_ROWLING_AUTHOR = {
  authorId: null,
  name: 'J. K. Rowling',
  following: false,
  bookCount: 0,
  provider: 'openlibrary',
  providerKey: null,
}

const KNOWN_KING_AUTHOR = {
  authorId: 5,
  name: 'Stephen King',
  following: false,
  bookCount: 3,
  provider: null,
  providerKey: null,
}

async function installLayoutShiftObserver(page) {
  await page.addInitScript(() => {
    window.__cls = 0
    new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        if (!entry.hadRecentInput) {
          window.__cls += entry.value
        }
      }
    }).observe({ type: 'layout-shift', buffered: true })
  })
}

async function mockHome(page, { spotlightDelay = 0 } = {}) {
  await page.route('**/api/profile/onboarding', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ onboarded: true, interests: [] }),
    }),
  )
  await page.route('**/api/home/spotlight', async (route) => {
    if (spotlightDelay > 0) {
      await new Promise((resolve) => setTimeout(resolve, spotlightDelay))
    }
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ items: [SPOTLIGHT] }),
    })
  })
  await page.route('**/api/home/updates', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ library: [], discoveries: [], ready: [] }),
    }),
  )
  await page.route('**/api/home/rails', (route) =>
    route.fulfill({ status: 200, contentType: 'application/json', body: '[]' }),
  )
  await page.route('**/api/collections', (route) =>
    route.fulfill({ status: 200, contentType: 'application/json', body: '[]' }),
  )
  await page.route('**/api/books/recent**', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify([BOOK]),
    }),
  )
  await page.route('**/api/books/highlights**', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify([BOOK]),
    }),
  )
  await page.route('**/api/books/continue', (route) =>
    route.fulfill({ status: 200, contentType: 'application/json', body: '[]' }),
  )
  await page.route('**/api/books**', (route) => {
    const url = new URL(route.request().url())
    if (route.request().method() !== 'GET') {
      return route.continue()
    }
    if (url.pathname === '/api/books') {
      return route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ items: [BOOK], total: 1, letters: [], page: 1, pageSize: 24 }),
      })
    }
    if (url.pathname.endsWith('/cover')) {
      return route.fulfill({ status: 404 })
    }
    if (url.pathname === `/api/books/${BOOK_ID}`) {
      return route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          ...BOOK,
          description: 'Stable.',
          publicationYear: 2020,
          onShelf: true,
          preference: null,
          subjects: [],
          editions: [],
          files: [],
        }),
      })
    }
    // Let the more specific routes (recent/highlights) answer.
    return route.fallback()
  })
}

async function mockDiscover(page) {
  await page.route('**/api/discover**', async (route) => {
    const url = new URL(route.request().url())
    if (url.pathname === '/api/discover/authors/ensure') {
      return route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ authorId: 777 }),
      })
    }
    if (url.pathname !== '/api/discover') {
      return route.fallback()
    }
    const query = (url.searchParams.get('q') ?? '').toLowerCase()
    const type = url.searchParams.get('type') ?? 'any'
    const localOnly = url.searchParams.get('source') === 'local'
    const outage = query.includes('outage')
    const books = outage
      ? [KING_BOOK]
      : query.includes('harry')
        ? HARRY_BOOKS
        : query.includes('king')
          ? [KING_BOOK]
          : []
    const localAuthors =
      query.includes('king') || outage ? [KNOWN_KING_AUTHOR] : []
    // The server derives the matching books' author and suppresses the
    // provider-only namesake on Anywhere searches; the mock mirrors that
    // contract for the frontend test.
    const externalAuthors = query.includes('harry')
      ? type === 'author'
        ? [EXTERNAL_HARRY_AUTHOR, DERIVED_ROWLING_AUTHOR]
        : [DERIVED_ROWLING_AUTHOR]
      : []
    if (localOnly) {
      return route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          query,
          authors: { local: localAuthors, external: [] },
          books: outage ? [KING_BOOK] : [],
          next: null,
          local: true,
          provider: null,
        }),
      })
    }
    // The full response is slow so the local fast path is observable.
    await new Promise((resolve) => setTimeout(resolve, 1000))
    if (outage) {
      return route.fulfill({
        status: 503,
        contentType: 'application/json',
        body: JSON.stringify({ code: 'unavailable', message: 'provider down', details: null }),
      })
    }
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        query,
        authors: { local: localAuthors, external: externalAuthors },
        books,
        next: null,
        local: false,
        provider: 'openlibrary',
      }),
    })
  })
}

export default async function stability(page, { base }) {
  await installLayoutShiftObserver(page)
  await mockHome(page, { spotlightDelay: 1500 })
  await mockDiscover(page)

  // A cold Home reserves the hero's space: the delayed spotlight must not
  // push the rails down after they rendered.
  await page.goto(`${base}/`, { waitUntil: 'domcontentloaded' })
  // The cold-load skeleton must fit the viewport too, not just the final page.
  await page.locator('.animate-pulse').first().waitFor({ state: 'visible', timeout: 3000 })
  const skeletonOverflow = await page.evaluate(
    () => document.documentElement.scrollWidth - window.innerWidth,
  )
  expect(
    skeletonOverflow <= 1,
    `the cold Home skeleton must not overflow at 390px (${skeletonOverflow}px)`,
  )
  // Core sections must not wait for the slow Spotlight call.
  await page.getByText(BOOK.title).first().waitFor({ state: 'visible', timeout: 1200 })
  await page.getByText(SPOTLIGHT.title).first().waitFor({ state: 'visible', timeout: 8000 })
  await page.waitForTimeout(250)
  const coldShift = await page.evaluate(() => window.__cls)
  expect(
    coldShift < 0.1,
    `cold Home should not shift when the hero arrives (CLS ${coldShift})`,
  )

  let spotlightRequests = 0
  page.on('request', (request) => {
    if (request.url().includes('/api/home/spotlight')) {
      spotlightRequests += 1
    }
  })

  // Revisiting Home renders from the session cache: no skeleton, no refetch.
  await page.getByRole('link', { name: 'Library' }).first().click()
  await page.waitForURL(`${base}/library`)
  await page.evaluate(() => {
    window.__cls = 0
  })
  await page.goBack()
  await page.waitForURL(`${base}/`)
  await page.getByText(SPOTLIGHT.title).first().waitFor({ state: 'visible', timeout: 3000 })
  await page.waitForTimeout(250)
  const revisitShift = await page.evaluate(() => window.__cls)
  expect(
    revisitShift < 0.05,
    `a cached Home revisit should not shift (CLS ${revisitShift})`,
  )
  expect(
    spotlightRequests === 0,
    `a fresh cached Home must not refetch Spotlight (got ${spotlightRequests})`,
  )

  // A cached Discover search restores books and authors together and keeps
  // the provider-only author out of an Anywhere book search.
  let searchRequests = 0
  page.on('request', (request) => {
    const url = request.url()
    if (url.includes('/api/discover?') && !url.includes('source=local')) {
      searchRequests += 1
    }
  })

  const fullDiscover = (suffix) =>
    page.waitForResponse(
      (response) => {
        const url = new URL(response.url())
        return (
          url.pathname === '/api/discover' &&
          url.searchParams.get('source') !== 'local' &&
          url.searchParams.get('q') === suffix
        )
      },
      { timeout: 8000 },
    )

  const harryAuthors = fullDiscover('harry potter')
  await page.goto(`${base}/discover?q=harry+potter&type=any`, { waitUntil: 'domcontentloaded' })
  await page.getByText(HARRY_BOOKS[0].title).first().waitFor({ timeout: 8000 })
  await harryAuthors
  await page.waitForTimeout(200)
  expect(
    (await page.getByText('Authors').count()) > 0,
    'the matching books\' author must surface',
  )
  await page.getByText('J. K. Rowling').first().waitFor({ timeout: 4000 })
  expect(
    (await page.getByText('Harry Potter', { exact: true }).count()) === 0,
    'the provider-only namesake must be suppressed',
  )
  const firstSearchRequests = searchRequests

  await page.getByRole('link', { name: 'Library' }).first().click()
  await page.waitForURL(`${base}/library`)
  await page.evaluate(() => {
    window.__cls = 0
  })
  await page.goBack()
  await page.waitForURL((url) => url.pathname === '/discover')
  await page.getByText(HARRY_BOOKS[0].title).first().waitFor({ timeout: 3000 })
  const cachedShift = await page.evaluate(() => window.__cls)
  expect(cachedShift < 0.05, `cached Discover should restore without shift (CLS ${cachedShift})`)
  expect(
    searchRequests === firstSearchRequests,
    `a cached search must not re-query the provider (${searchRequests})`,
  )

  // A known household author surfaces from the local fast path, before the
  // slow full response arrives.
  const kingAuthors = fullDiscover('stephen king')
  await page.goto(`${base}/discover?q=stephen+king&type=any`, { waitUntil: 'domcontentloaded' })
  // Well under the full response's 1000 ms delay, with room for the lazy
  // Discover chunk on a loaded machine.
  await page.getByText('Stephen King').first().waitFor({ timeout: 700 })
  await kingAuthors
  expect((await page.getByText('Authors').count()) > 0, 'a known author must surface')

  await page.goto(`${base}/discover?q=harry+potter&type=author`, { waitUntil: 'networkidle' })
  await page.getByText('Harry Potter', { exact: true }).first().waitFor({ timeout: 8000 })
  expect(
    (await page.getByText('Authors').count()) > 0,
    'an explicit author search keeps provider authors',
  )

  // A transient author opens without requiring a follow.
  await page.getByRole('region', { name: 'Author results' }).getByRole('button', { name: 'J. K. Rowling', exact: true }).click()
  await page.waitForURL(`${base}/authors/777`, { timeout: 4000 })

  // A failing provider keeps the local catalogue visible with a note.
  await page.goto(`${base}/discover?q=outage&type=any`, { waitUntil: 'domcontentloaded' })
  await page.getByText(KING_BOOK.title).first().waitFor({ timeout: 8000 })
  await page
    .getByText('Showing your catalogue — online results unavailable.')
    .waitFor({ timeout: 8000 })
  expect(
    (await page.getByText('Stephen King').count()) > 0,
    'the local author must survive a provider outage',
  )
}

import { expect } from '../support.mjs'

const item = (overrides) => ({
  provider: 'openlibrary',
  providerKey: '/works/OL1W',
  title: 'A Mocked Book',
  authors: ['Mock Author'],
  year: 2021,
  language: 'en',
  isbn10: null,
  isbn13: null,
  series: null,
  seriesNumber: null,
  coverId: null,
  status: 'NOT_IN_LIBRARY',
  ownedBookId: null,
  ownedFileId: null,
  ...overrides,
})

/** Discover renders catalogue entries, availability signals and owned sheets. */
export default async function discover(page, { base, user }) {
  const books = [
    item({
      providerKey: '/works/OLOWNEDW',
      title: 'Owned Mock',
      status: 'IN_LIBRARY',
      ownedBookId: 42,
      ownedFileId: 420,
    }),
    item({ providerKey: '/works/OLNEWW', title: 'Wanted Mock', coverId: null }),
    item({ providerKey: '/works/OLREADYW', title: 'Ready Mock', ownedBookId: 45 }),
    item({
      providerKey: 'local:44',
      provider: 'local',
      title: 'Local Wanted Mock',
      status: 'NOT_IN_LIBRARY',
      ownedBookId: 44,
    }),
  ]
  await page.route('**/api/discover**', (route) => {
    const url = new URL(route.request().url())
    if (url.pathname !== '/api/discover') {
      return route.fallback()
    }
    const localOnly = url.searchParams.get('source') === 'local'
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        query: url.searchParams.get('q') ?? '',
        authors: { local: [], external: [] },
        books: localOnly ? [] : books,
        next: null,
        local: localOnly,
        provider: localOnly ? null : 'openlibrary',
      }),
    })
  })
  await page.route('**/api/discover/releases**', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        releases: [
          { releaseName: 'A.Mock.Release.EPUB', format: 'epub', language: 'en', sizeBytes: 4200000, seeders: 30, leechers: 0, indexer: 'mock' },
        ],
      }),
    }),
  )
  let externalLiked = false
  await page.route('**/api/discover/book**', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        ...item({
          providerKey: '/works/OLNEWW',
          title: 'Wanted Mock',
          ownedBookId: externalLiked ? 555 : null,
          onShelf: false,
        }),
        description: 'A wanted mock description.',
        publisher: null,
        liked: externalLiked,
      }),
    }),
  )
  let likedBody = null
  await page.route('**/api/discover/like', (route) => {
    likedBody = JSON.parse(route.request().postData() ?? '{}')
    externalLiked = true
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ bookId: 555, preference: 'liked' }),
    })
  })
  let acquisitions = 0
  page.on('request', (request) => {
    const path = new URL(request.url()).pathname
    if (request.method() === 'POST' && path.startsWith('/api/acquisitions')) {
      acquisitions += 1
    }
  })

  await page.route('**/api/books/42', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        id: 42,
        title: 'Owned Mock',
        authors: ['Mock Author'],
        language: 'en',
        series: null,
        seriesNumber: null,
        hasCover: false,
        addedAt: 1,
        description: 'An owned mock description.',
        publicationYear: 2021,
        subjects: [],
        editions: [],
        files: [],
      }),
    }),
  )

  await page.goto(`${base}/discover`, { waitUntil: 'networkidle' })
  await page.fill('input[aria-label="Search books"]', 'mock')
  await page.getByRole('button', { name: /^Open details for/ }).first().waitFor({ timeout: 15000 })
  expect(await page.getByText('Owned Mock').first().isVisible(), 'result title should render')

  // Owned entries open from local data and offer the library actions.
  await page.getByRole('button', { name: 'Open details for Owned Mock' }).click()
  await page.getByRole('link', { name: /view book/i }).waitFor({ timeout: 8000 })
  await page.getByText('An owned mock description.').waitFor({ timeout: 8000 })
  const sheet = await page.locator('[role="dialog"]').innerText()
  expect(
    sheet.includes('An owned mock description.'),
    `owned sheet should render local details, got: ${sheet.replace(/\n+/g, ' | ')}`,
  )

  await page.getByRole('button', { name: /close/i }).first().click()
  await page.locator('[role="dialog"]').waitFor({ state: 'detached', timeout: 5000 })

  // Availability is a signal, not a release dump: regular users must not see
  // torrent names, indexers or seeders.
  await page.getByRole('button', { name: 'Open details for Wanted Mock' }).click()
  const check = page.getByRole('button', { name: /check availability/i })
  if (await check.isVisible().catch(() => false)) {
    const [releasesResponse] = await Promise.all([
      page
        .waitForResponse((response) => response.url().includes('/api/discover/releases'), {
          timeout: 10000,
        })
        .catch(() => null),
      check.click(),
    ])
    expect(releasesResponse !== null, 'clicking Check availability should request releases')
    await page
      .getByText(/good availability|limited availability|no copy/i)
      .first()
      .waitFor({ timeout: 8000 })
    const availability = await page.locator('[role="dialog"]').innerText()
    expect(
      /good availability|limited availability|no copy/i.test(availability),
      `availability signal should render, got: ${availability.replace(/\n+/g, ' | ')}`,
    )
    if (user !== 'admin') {
      expect(!availability.includes('A.Mock.Release.EPUB'), 'release names are admin-only')
      expect(!availability.includes('seeders'), 'seeder counts are admin-only')
    }
    await page.getByRole('button', { name: /close/i }).first().click()
    await page.locator('[role="dialog"]').waitFor({ state: 'detached', timeout: 5000 })
  }

  // A fresh external result can be liked without owning or acquiring it.
  if ((await page.locator('[role="dialog"]').count()) > 0) {
    await page.getByRole('button', { name: /close/i }).first().click()
    await page.locator('[role="dialog"]').waitFor({ state: 'detached', timeout: 5000 })
  }
  await page.getByRole('button', { name: 'Open details for Wanted Mock' }).click()
  await page.getByText('A wanted mock description.').waitFor({ timeout: 8000 })
  const [likeResponse] = await Promise.all([
    page.waitForResponse(
      (candidate) => candidate.url().includes('/api/discover/like'),
      { timeout: 8000 },
    ),
    page.getByRole('button', { name: 'Like', exact: true }).click(),
  ])
  expect(likeResponse.status() < 400, 'liking an external book must succeed')
  expect(
    likedBody?.providerKey === '/works/OLNEWW',
    `liking must post the provider key: ${JSON.stringify(likedBody)}`,
  )
  await page.getByRole('button', { name: 'Liked', exact: true }).waitFor({ timeout: 8000 })
  expect(acquisitions === 0, 'liking must not start an acquisition')

  await page.getByRole('button', { name: /close/i }).first().click()
  await page.locator('[role="dialog"]').waitFor({ state: 'detached', timeout: 5000 })
  await page.getByRole('button', { name: 'Open details for Wanted Mock' }).click()
  await page.getByRole('button', { name: 'Liked', exact: true }).waitFor({ timeout: 8000 })
  await page.getByRole('button', { name: /close/i }).first().click()
  await page.locator('[role="dialog"]').waitFor({ state: 'detached', timeout: 5000 })

  // The liked metadata-only book shows up in Profile without a shelf row.
  await page.route('**/api/profile/liked', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        items: [
          {
            bookId: 555,
            title: 'Wanted Mock',
            authors: ['Mock Author'],
            readable: false,
            onShelf: false,
            provider: 'openlibrary',
            providerKey: '/works/OLNEWW',
          },
        ],
      }),
    }),
  )
  await page.goto(`${base}/profile/taste`, { waitUntil: 'networkidle' })
  await page.getByText('Books you like').waitFor({ state: 'visible', timeout: 8000 })
  await page.getByText('Wanted Mock').waitFor({ state: 'visible', timeout: 8000 })
  await page.goto(`${base}/discover`, { waitUntil: 'networkidle' })

  // A metadata-only local book is acquired by id, without provider
  // resolution (its provider may be gone or local-only).
  let acquiredById = false
  await page.route('**/api/books/44/acquisitions', (route) => {
    acquiredById = route.request().method() === 'POST'
    return route.fulfill({
      status: 202,
      contentType: 'application/json',
      body: JSON.stringify({ id: 'acq-local', status: 'REQUESTED', duplicate: false, bookId: 44 }),
    })
  })
  await page
    .locator('article')
    .filter({ hasText: 'Local Wanted Mock' })
    .getByRole('button', { name: /^Get$/ })
    .click()
  await page.waitForTimeout(300)
  expect(acquiredById, 'a known local book must acquire by book id')
  // The card reflects the acquisition state the server returned.
  await page
    .locator('article')
    .filter({ hasText: 'Local Wanted Mock' })
    .getByText('On its way')
    .waitFor({ timeout: 8000 })

  // A duplicate that is already READY must render as in-library, not as a
  // download: the server status drives the card.
  await page.route('**/api/books/45/acquisitions', (route) =>
    route.fulfill({
      status: 202,
      contentType: 'application/json',
      body: JSON.stringify({ id: 'acq-ready', status: 'READY', duplicate: true, bookId: 45 }),
    }),
  )
  const readyCard = page.locator('article').filter({ hasText: 'Ready Mock' })
  await readyCard.getByRole('button', { name: /^Get$/ }).click()
  await readyCard.getByText('In library').waitFor({ timeout: 8000 })
  expect(
    (await readyCard.getByText('On its way').count()) === 0,
    'a READY duplicate must not be shown as downloading',
  )

  // Add to my shelf patches the visible card and the cached search page.
  await page.route('**/api/books/42/shelf', (route) =>
    route.fulfill({ status: 204, body: '' }),
  )
  let discoverRequests = 0
  page.on('request', (request) => {
    const url = new URL(request.url())
    if (request.method() === 'GET' && url.pathname === '/api/discover') {
      discoverRequests += 1
    }
  })
  const ownedCard = page.locator('article').filter({ hasText: 'Owned Mock' })
  await ownedCard.getByRole('button', { name: 'Add to my shelf' }).click()
  await ownedCard.getByText('On my shelf').waitFor({ timeout: 8000 })
  const requestsBefore = discoverRequests
  // In-SPA navigation only: a full reload legitimately drops the session cache.
  await page.getByRole('link', { name: 'Library' }).first().click()
  await page.waitForURL(`${base}/library`)
  await page.goBack()
  await page.waitForURL((url) => url.pathname === '/discover')
  await page
    .locator('article')
    .filter({ hasText: 'Owned Mock' })
    .getByText('On my shelf')
    .waitFor({ timeout: 8000 })
  expect(
    discoverRequests === requestsBefore,
    'the cached search must carry the shelf patch without refetching',
  )

  // Fast typing must never be rolled back by a debounced search or a URL
  // update: the input is authoritative.
  const typingInput = page.locator('input[aria-label="Search books"]').first()
  await typingInput.fill('')
  await typingInput.pressSequentially('har', { delay: 30 })
  await page.waitForTimeout(350)
  await typingInput.pressSequentially('ry', { delay: 30 })
  await page.waitForTimeout(900)
  const typedValue = await typingInput.inputValue()
  expect(typedValue === 'harry', `the input must keep exactly what was typed, got ${typedValue}`)
  expect(page.url().includes('q=harry'), `the URL must match the typed query, got ${page.url()}`)

  // Returning to a cached query must invalidate an older provider response.
  let releaseSlowSearch
  const slowSearchGate = new Promise((resolve) => {
    releaseSlowSearch = resolve
  })
  let markSlowSearchStarted
  const slowSearchStarted = new Promise((resolve) => {
    markSlowSearchStarted = resolve
  })
  await page.route('**/api/discover**', async (route) => {
    const url = new URL(route.request().url())
    if (
      url.pathname !== '/api/discover' ||
      url.searchParams.get('q') !== 'slowrace' ||
      url.searchParams.get('source') === 'local'
    ) {
      return route.fallback()
    }
    markSlowSearchStarted()
    await slowSearchGate
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        query: 'slowrace',
        authors: { local: [], external: [] },
        books: [item({ providerKey: '/works/OLSLOWW', title: 'Slow Race Book' })],
        next: null,
        local: false,
        provider: 'openlibrary',
      }),
    })
  })
  const slowRequest = page.waitForRequest((request) => {
    const url = new URL(request.url())
    return (
      url.pathname === '/api/discover' &&
      url.searchParams.get('q') === 'slowrace' &&
      url.searchParams.get('source') !== 'local'
    )
  })
  await typingInput.fill('slowrace')
  await slowRequest
  await slowSearchStarted
  await typingInput.fill('mock')
  await page.getByRole('button', { name: 'Open details for Owned Mock' }).waitFor({ timeout: 8000 })
  const slowResponse = page.waitForResponse((response) => {
    const url = new URL(response.url())
    return (
      url.pathname === '/api/discover' &&
      url.searchParams.get('q') === 'slowrace' &&
      url.searchParams.get('source') !== 'local'
    )
  })
  releaseSlowSearch()
  await slowResponse
  await page.waitForTimeout(100)
  expect(
    await page.getByRole('button', { name: 'Open details for Owned Mock' }).isVisible(),
    'the cached search must remain visible after an older response finishes',
  )
  expect(
    (await page.getByRole('button', { name: 'Open details for Slow Race Book' }).count()) === 0,
    'an older response must not replace the cached search',
  )

  // A provider failure must remain retryable for the same query in this tab.
  let retryRequests = 0
  await page.route('**/api/discover**', async (route) => {
    const url = new URL(route.request().url())
    if (
      url.pathname !== '/api/discover' ||
      url.searchParams.get('q') !== 'retrycase' ||
      url.searchParams.get('source') === 'local'
    ) {
      return route.fallback()
    }
    retryRequests += 1
    if (retryRequests === 1) {
      await new Promise((resolve) => setTimeout(resolve, 150))
      return route.fulfill({ status: 503, contentType: 'application/json', body: '{"error":"unavailable"}' })
    }
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        query: 'retrycase',
        authors: { local: [], external: [] },
        books: [item({ providerKey: '/works/OLRETRYW', title: 'Retried Book' })],
        next: null,
        local: false,
        provider: 'openlibrary',
      }),
    })
  })
  await typingInput.fill('retrycase')
  await page.getByText('Showing your catalogue — online results unavailable.').waitFor({ timeout: 8000 })
  await page.getByRole('button', { name: 'Search books' }).click()
  await page.getByRole('button', { name: 'Open details for Retried Book' }).waitFor({ timeout: 8000 })
  expect(retryRequests >= 2, 'the same query must fetch again after a failure')

  const searchAllLanguages = page.getByRole('button', { name: 'Search all languages' })
  if (await searchAllLanguages.isVisible()) {
    const beforeModeChange = retryRequests
    const refreshed = page.waitForResponse((response) => {
      const url = new URL(response.url())
      return url.pathname === '/api/discover' && url.searchParams.get('q') === 'retrycase' && url.searchParams.get('source') !== 'local'
    })
    await searchAllLanguages.click()
    await refreshed
    expect(retryRequests > beforeModeChange, 'changing language mode must search with the new cache key')
    await page.getByRole('button', { name: 'Open details for Retried Book' }).waitFor({ timeout: 8000 })
  }
}

function expect(condition, message) {
  if (!condition) {
    throw new Error(message)
  }
}

const BOOK_ID = 9401
const FILE_ID = 92

const CHILD = {
  id: 2,
  username: 'child',
  displayName: 'Child',
  role: 'user',
  preferredFormat: null,
  preferredLanguage: null,
  acquisitionMode: 'automatic',
  notificationEmail: null,
  emailNotifications: false,
  profileType: 'child',
  defaultLanguage: null,
  preferredLanguages: [],
}

const BOOK = {
  id: BOOK_ID,
  title: 'Child Mobile Book',
  authors: ['Child Author'],
  language: 'en',
  series: null,
  seriesNumber: null,
  hasCover: false,
  hasDescription: true,
  rating: null,
  ratingCount: null,
  ratingSource: null,
  addedAt: 1,
  description: 'A book on the child shelf.',
  publicationYear: 2020,
  onShelf: true,
  preference: null,
  subjects: [],
  editions: [],
  files: [
    { id: FILE_ID, editionId: 1, format: 'epub', size: 420000, filename: 'child.epub' },
  ],
}

export default async function mobileChild(page, { base }) {
  const forbidden = []
  page.on('request', (request) => {
    const path = new URL(request.url()).pathname
    if (
      path === '/api/books/facets' ||
      path === '/api/collections' ||
      path === '/api/home/updates' ||
      path.startsWith('/api/delivery-targets') ||
      path === '/api/deliveries' ||
      path.endsWith('/related') ||
      path === '/api/acquisitions' ||
      path === '/api/authors'
    ) {
      forbidden.push(path)
    }
  })

  await page.route('**/api/auth/me', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ user: CHILD }),
    }),
  )
  await page.route('**/api/profile/onboarding', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ onboarded: true, interests: [] }),
    }),
  )
  await page.route('**/api/books**', (route) => {
    const url = new URL(route.request().url())
    const path = url.pathname
    if (route.request().method() !== 'GET') {
      return route.continue()
    }
    if (path === '/api/books/recent' || path === '/api/books/highlights') {
      return route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify([BOOK]),
      })
    }
    if (path === '/api/books/continue') {
      return route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify([{
          book: BOOK,
          percentage: 0.42,
          updatedAt: 1,
          source: 'koreader',
          browserFileId: null,
          browserPercentage: null,
          epubFileId: FILE_ID,
        }]),
      })
    }
    if (path === '/api/books') {
      return route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ items: [BOOK], total: 1, letters: [], page: 1, pageSize: 24 }),
      })
    }
    if (path === `/api/books/${BOOK_ID}/cover`) {
      return route.fulfill({ status: 404 })
    }
    if (path === `/api/books/${BOOK_ID}`) {
      return route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(BOOK),
      })
    }
    return route.continue()
  })

  await page.reload({ waitUntil: 'networkidle' })
  forbidden.length = 0

  const bottomNav = page.locator('nav.fixed')
  await bottomNav.waitFor({ state: 'visible', timeout: 8000 })
  expect(
    (await bottomNav.locator('a').count()) === 3,
    'the child mobile nav must show Home, Library and Requests',
  )
  expect(
    (await page.locator('a[href="/discover"]').count()) === 0,
    'children must not see a Discover link',
  )

  await page.goto(`${base}/`, { waitUntil: 'networkidle' })
  await page.getByText(BOOK.title).first().waitFor({ state: 'visible', timeout: 8000 })
  await page.getByText('Continue reading').first().waitFor({ state: 'visible', timeout: 8000 })
  const homeText = await page.locator('main').innerText()
  expect(
    /42%/.test(homeText),
    `the continue rail must show reader progress: ${homeText.slice(0, 240)}`,
  )
  expect(await page.getByRole('progressbar', { name: 'KOReader reading progress' }).getAttribute('aria-valuenow') === '42',
    'KOReader progress must have an accessible value')
  expect(/Starts here at beginning/.test(homeText), 'KOReader progress must not imply a browser resume position')
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - window.innerWidth,
  )
  expect(overflow <= 1, `child home: ${overflow}px of horizontal overflow at 390x844`)

  await bottomNav.locator('a', { hasText: 'Library' }).click()
  await page.waitForURL(`${base}/library`)
  await page.getByLabel('Search your shelf').waitFor({ state: 'visible', timeout: 8000 })

  await page.goto(`${base}/library/${BOOK_ID}`, { waitUntil: 'networkidle' })
  await page.waitForSelector(`text=${BOOK.title}`, { timeout: 8000 })
  const detailText = await page.locator('main').innerText()
  expect(!/Send to my reader/.test(detailText), 'child detail must not offer sending')
  expect(!/Download/.test(detailText), 'child detail must not offer downloads')
  expect(!/Manage collections/.test(detailText), 'child detail must not offer collection management')
  expect(
    !/On my shelf|Add to my shelf|Not for me/.test(detailText),
    'child detail must not offer shelf or taste mutations',
  )
  expect(await page.getByRole('button', { name: 'Like', exact: true }).isVisible(), 'child detail must still offer a like toggle')
  expect(
    forbidden.length === 0,
    `the child UI must not request forbidden endpoints: ${forbidden.join(', ')}`,
  )
}

function expect(condition, message) {
  if (!condition) {
    throw new Error(message)
  }
}

const BOOK_ID = 9301
const FILE_ID = 91

const BOOK = {
  id: BOOK_ID,
  title: 'Mobile Journey Book',
  authors: ['Mobile Author'],
  language: 'en',
  series: null,
  seriesNumber: null,
  hasCover: false,
  hasDescription: true,
  rating: null,
  ratingCount: null,
  ratingSource: null,
  addedAt: 1,
  description: 'A quiet book for a small screen.',
  publicationYear: 2021,
  onShelf: true,
  preference: null,
  subjects: [],
  editions: [],
  files: [
    { id: FILE_ID, editionId: 1, format: 'epub', size: 420000, filename: 'mobile.epub' },
  ],
}

const DISCOVERY = {
  provider: 'openlibrary',
  providerKey: '/works/OLMOBILEW',
  title: 'Mobile Discovery Book',
  authors: ['Mobile Author'],
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
  onShelf: false,
}

function reader(index) {
  return {
    id: 800 + index,
    userId: 1,
    type: 'pocketbook',
    name: `Reader ${index}`,
    address: `reader${index}@pbsync.com`,
    connector: 'email',
    enabled: true,
    isDefault: index === 1,
    createdAt: 1,
    updatedAt: 1,
  }
}

async function mockJourney(page, homeReady) {
  // Pending requests never add a tab: adults approve from the bell.
  await page.route('**/api/notifications', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        items: [],
        unread: 0,
        pendingRequests: 1,
        pendingRequestItems: [],
      }),
    }),
  )
  await page.route('**/api/profile/onboarding', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ onboarded: true, interests: [] }),
    }),
  )
  await page.route('**/api/home/spotlight*', async (route) => {
    await homeReady
    return route.fulfill({ status: 200, contentType: 'application/json', body: '{"items":[],"recommendations":[]}' })
  })

  await page.route('**/api/books**', async (route) => {
    const url = new URL(route.request().url())
    const path = url.pathname
    if (route.request().method() !== 'GET') {
      return route.continue()
    }
    if (
      path === '/api/books/recent' ||
      path === '/api/books/highlights' ||
      path === '/api/books/continue'
    ) {
      // Keep Home pending until the test has inspected its loading layout.
      await homeReady
      const items = path === '/api/books/continue' ? [] : [BOOK]
      return route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(items),
      })
    }
    if (path === '/api/books') {
      return route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          items: [BOOK],
          total: 1,
          letters: [],
          page: 1,
          pageSize: 24,
        }),
      })
    }
    if (path === `/api/books/${BOOK_ID}/related`) {
      return route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ series: [], author: [], similar: [] }),
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
        books: localOnly ? [] : [DISCOVERY],
        next: null,
        local: localOnly,
        provider: localOnly ? null : 'openlibrary',
      }),
    })
  })
  await page.route('**/api/discover/book**', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ ...DISCOVERY, description: 'A discovery for the small screen.', publisher: null, liked: false }),
    }),
  )

  await page.route('**/api/delivery-targets/default', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        address: 'reader1@pbsync.com',
        source: 'personal',
        senderAddress: 'library@bokhylle.local',
        amazonUrl: 'https://www.amazon.com/hz/mycd/digital-console/contentlist/pdocs',
      }),
    }),
  )
  await page.route('**/api/delivery-targets', (route) => {
    if (route.request().method() !== 'GET') {
      return route.continue()
    }
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(
        Array.from({ length: 10 }, (_, index) => reader(index + 1)),
      ),
    })
  })
  await page.route('**/api/deliveries**', (route) =>
    route.fulfill({ status: 200, contentType: 'application/json', body: '[]' }),
  )
}

async function noHorizontalOverflow(page, label) {
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - window.innerWidth,
  )
  expect(overflow <= 1, `${label}: ${overflow}px of horizontal overflow at 390x844`)
}

function insideViewport(box, label) {
  expect(box !== null, `${label}: missing bounding box`)
  expect(box.x >= -0.5 && box.y >= -0.5, `${label}: starts outside the viewport ${JSON.stringify(box)}`)
  expect(box.x + box.width <= 390.5, `${label}: exceeds the viewport width ${JSON.stringify(box)}`)
  expect(box.y + box.height <= 844.5, `${label}: exceeds the viewport height ${JSON.stringify(box)}`)
}

export default async function mobile(page, { base }) {
  let releaseHome
  const homeReady = new Promise((resolve) => { releaseHome = resolve })
  await mockJourney(page, homeReady)

  await page.goto(`${base}/`, { waitUntil: 'domcontentloaded' })
  try {
    await page.locator('.animate-pulse').first().waitFor({ state: 'visible', timeout: 3000 })
    const skeletonOverflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)
    expect(skeletonOverflow <= 1, `the cold Home skeleton must not overflow at 390x844 (${skeletonOverflow}px)`)
  } finally {
    releaseHome()
  }
  const bottomNav = page.locator('nav.fixed')
  await bottomNav.waitFor({ state: 'visible', timeout: 8000 })
  expect(
    (await bottomNav.locator('a').count()) === 4,
    'the adult mobile nav shows Home, Discover, Library and Activity',
  )
  await page.getByText(BOOK.title).first().waitFor({ state: 'visible', timeout: 8000 })
  await noHorizontalOverflow(page, 'home')

  await page.getByRole('button', { name: 'Notifications' }).click()
  const notifications = page.locator('.shadow-modal').filter({ hasText: 'Notifications' })
  await notifications.waitFor({ state: 'visible' })
  insideViewport(await notifications.boundingBox(), 'notifications')
  await noHorizontalOverflow(page, 'notifications')
  await page.getByRole('button', { name: 'Notifications' }).click()

  await bottomNav.locator('a', { hasText: 'Discover' }).click()
  await page.waitForURL(`${base}/discover`)
  await page.locator('input[aria-label="Search books"]').fill('mobile discovery')
  await page.getByText(DISCOVERY.title).first().waitFor({ state: 'visible', timeout: 8000 })
  await page.getByText(DISCOVERY.title).first().click()

  const sheet = page.getByRole('dialog', { name: DISCOVERY.title })
  await sheet.waitFor({ state: 'visible', timeout: 8000 })
  insideViewport(await sheet.boundingBox(), 'discover sheet')
  const getIt = sheet.getByRole('button', { name: /Get & Send to My Reader/ })
  await getIt.waitFor({ state: 'visible', timeout: 8000 })
  const getBox = await getIt.boundingBox()
  expect(
    getBox !== null && getBox.y + getBox.height <= 844.5,
    `the primary sheet action must stay reachable: ${JSON.stringify(getBox)}`,
  )
  await sheet.getByRole('button', { name: 'Close' }).click()
  await sheet.waitFor({ state: 'hidden', timeout: 8000 })

  await page.goto(`${base}/library/${BOOK_ID}`, { waitUntil: 'networkidle' })
  await page.waitForSelector(`text=${BOOK.title}`, { timeout: 8000 })
  await noHorizontalOverflow(page, 'book detail')
  const detailText = await page.locator('main').innerText()
  expect(
    !/Available to/.test(detailText),
    'admin shelf assignment must not clutter the book hero',
  )
  await page.getByRole('button', { name: 'More', exact: true }).click()
  await page.getByRole('button', { name: /Manage access/ }).waitFor({ state: 'visible', timeout: 8000 })
  await page.getByRole('button', { name: 'Close book options', exact: true }).click()

  await page.getByRole('button', { name: /Send to my reader/ }).first().click()
  const readerDialog = page.getByRole('dialog', { name: 'Send to your reader' })
  await readerDialog.waitFor({ state: 'visible', timeout: 8000 })
  insideViewport(await readerDialog.boundingBox(), 'send dialog')
  const send = readerDialog.getByRole('button', { name: /^Send$/ })
  await send.waitFor({ state: 'visible', timeout: 8000 })
  const sendBox = await send.boundingBox()
  expect(
    sendBox !== null && sendBox.y + sendBox.height <= 844.5,
    `the Send action must stay reachable with many readers: ${JSON.stringify(sendBox)}`,
  )
  const scrollableBody = await readerDialog.evaluate((node) =>
    Array.from(node.querySelectorAll('*')).some((element) => {
      const style = getComputedStyle(element)
      return (
        style.overflowY === 'auto' &&
        element.scrollHeight > element.clientHeight + 10 &&
        element.clientHeight > 0
      )
    }),
  )
  expect(scrollableBody, 'the dialog body must scroll inside the viewport')
  await readerDialog.getByRole('button', { name: 'Close' }).click()
  await readerDialog.waitFor({ state: 'hidden', timeout: 8000 })

  await bottomNav.locator('a', { hasText: 'Library' }).click()
  await page.waitForURL(`${base}/library`)
  await bottomNav.locator('a', { hasText: 'Home' }).click()
  await page.waitForURL(`${base}/`)
  await noHorizontalOverflow(page, 'home after navigation')
}

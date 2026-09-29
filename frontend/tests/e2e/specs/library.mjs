function expect(condition, message) {
  if (!condition) {
    throw new Error(message)
  }
}

const SHELF_BOOK = {
  id: 9001, title: 'Shelf Only Book', authors: ['Shelf Author'], language: 'en',
  series: null, seriesNumber: null, hasCover: false, hasDescription: true,
  rating: null, ratingCount: null, ratingSource: null, addedAt: 1,
}
const HOUSEHOLD_BOOK = {
  ...SHELF_BOOK, id: 9002, title: 'Household Only Book', authors: ['Household Author'],
}

const PAGE_SIZE = 1

async function mockLibrary(page) {
  // One handler for both list and search so route order cannot matter.
  await page.route('**/api/books**', (route) => {
    const url = new URL(route.request().url())
    if (url.pathname.endsWith('/facets')) {
      return route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ formats: [], languages: [], series: [], subjects: [] }),
      })
    }
    const mine = url.searchParams.get('mine') === 'true'
    const isSearch = url.pathname.endsWith('/search')
    if (isSearch) {
      const items = mine ? [SHELF_BOOK] : [SHELF_BOOK, HOUSEHOLD_BOOK]
      return route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(items),
      })
    }
    const pageParam = Number(url.searchParams.get('page') ?? '1')
    const all = mine ? [SHELF_BOOK] : [SHELF_BOOK, HOUSEHOLD_BOOK]
    const start = (pageParam - 1) * PAGE_SIZE
    const items = all.slice(start, start + PAGE_SIZE)
    const letters = mine ? ['s'] : ['h', 's']
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ items, total: all.length, letters, page: pageParam, pageSize: PAGE_SIZE }),
    })
  })
}

export default async function library(page, { base }) {
  await mockLibrary(page)

  // FE-32: My shelf search excludes household-only books; Household includes them.
  await page.goto(`${base}/library?scope=mine`, { waitUntil: 'networkidle' })
  const search = page.locator('input[aria-label^="Search"]')
  await search.fill('book')
  await page.waitForTimeout(1400)
  expect(
    await page.getByText('Shelf Only Book').first().isVisible().catch(() => false),
    'shelf book must appear in My shelf search',
  )
  expect(
    (await page.getByText('Household Only Book').count()) === 0,
    'household-only book must not appear in My shelf search',
  )

  // The scope control hides while a query is active, so clear first.
  await search.fill('')
  await page.waitForTimeout(1000)
  await page.getByRole('button', { name: /^Household$/ }).first().click()
  await page.waitForTimeout(1000)
  await search.fill('book')
  await page.waitForTimeout(1400)
  expect(
    await page.getByText('Household Only Book').first().isVisible().catch(() => false),
    'household-only book must appear in Household search',
  )

  // FE-32: a real page-1 -> page-2 load keeps book links unique. The list
  // auto-loads when its sentinel scrolls into view, so wait for page 2
  // instead of requiring the Load more button to stay on screen.
  const pageTwoPromise = page.waitForResponse(
    (candidate) =>
      candidate.url().includes('/api/books?') && candidate.url().includes('page=2'),
    { timeout: 8000 },
  )
  await page.goto(`${base}/library?scope=household`, { waitUntil: 'networkidle' })
  const pageTwo = await pageTwoPromise
  expect(pageTwo.status() < 400, `page 2 must load, got ${pageTwo.status()}`)
  const pageTwoBody = await pageTwo.json()
  expect(
    pageTwoBody.items.length === 1 && pageTwoBody.items[0].id === HOUSEHOLD_BOOK.id,
    `page 2 must serve the second book: ${JSON.stringify(pageTwoBody.items)}`,
  )
  await page.getByText('Household Only Book').first().waitFor({ state: 'visible', timeout: 8000 })
  const hrefs = await page.locator('a[href^="/library/"]').evaluateAll((nodes) =>
    nodes.map((node) => node.getAttribute('href') ?? '').filter((href) => /^\/library\/\d+$/.test(href)),
  )
  expect(hrefs.length === 2, `both pages must be present, got ${JSON.stringify(hrefs)}`)
  expect(new Set(hrefs).size === hrefs.length, 'no duplicate book links after loading more')

  // Fast typing must survive the search debounce and URL synchronisation.
  await page.goto(`${base}/library?scope=mine`, { waitUntil: 'networkidle' })
  const typingSearch = page.locator('input[aria-label^="Search"]')
  await typingSearch.pressSequentially('she', { delay: 30 })
  await page.waitForTimeout(200)
  await typingSearch.pressSequentially('lf', { delay: 30 })
  await page.waitForTimeout(800)
  const typedShelf = await typingSearch.inputValue()
  expect(typedShelf === 'shelf', `the shelf input must keep exactly what was typed, got ${typedShelf}`)
  expect(page.url().includes('q=shelf'), `the URL must match the typed query, got ${page.url()}`)

  // FE-33: Authors Following with a zero-book external author, unfollow, A–Z.
  let following = true
  await page.route('**/api/authors**', (route) => {
    if (route.request().url().includes('/follow')) {
      return route.fallback()
    }
    const url = new URL(route.request().url())
    const only = url.searchParams.get('following') === 'true'
    const authors = [
      { id: 1, name: 'Aaron External', bookCount: 0, following, autoAcquire: false },
      { id: 2, name: 'Zed Local', bookCount: 3, following: false, autoAcquire: false },
    ].filter((author) => (only ? author.following : true))
    route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(authors) })
  })
  await page.route('**/api/authors/*/follow', (route) => {
    if (route.request().method() === 'DELETE') {
      following = false
      return route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ ok: true }),
      })
    }
    following = true
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ ok: true }),
    })
  })

  await page.goto(`${base}/library?mode=authors&following=1`, { waitUntil: 'networkidle' })
  await page.waitForTimeout(1400)
  expect(
    (await page.locator('div[data-letter]').allInnerTexts()).some((text) =>
      text.includes('Aaron External'),
    ),
    'a followed external author with zero books must appear under Following',
  )

  const row = page.locator('div[data-letter]').first()
  const [response] = await Promise.all([
    page.waitForResponse(
      (candidate) =>
        candidate.url().includes('/follow') && candidate.request().method() === 'DELETE',
      { timeout: 8000 },
    ),
    row.getByRole('button', { name: /^Following$/ }).click(),
  ])
  expect(response.status() < 400, `DELETE /follow must succeed, got ${response.status()}`)
  await page.waitForFunction(
    () => !document.body.innerText.includes('Aaron External'),
    undefined,
    { timeout: 8000 },
  )
  expect(
    (await page.locator('div[data-letter]').count()) === 0,
    'unfollowing the only followed author must remove the row',
  )
  await page.getByRole('button', { name: /^All authors$/ }).first().click()
  await page.waitForTimeout(1600)
  const allNames = await page.locator('div[data-letter]').allInnerTexts()
  expect(allNames.length === 2, `All authors shows both authors: ${JSON.stringify(allNames)}`)

  // FE-33: A–Z filters to the expected author.
  await page.getByRole('button', { name: 'Jump to A' }).click()
  await page.waitForTimeout(500)
  const filtered = await page.locator('div[data-letter]').allInnerTexts()
  expect(
    filtered.length === 1 && filtered[0].includes('Aaron External'),
    'A must filter to the A author only',
  )

  // Preferred languages are the author page's browsing lens, with a toggle.
  await page.route('**/api/auth/me', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        user: {
          id: 1,
          username: 'admin',
          displayName: 'Admin',
          role: 'admin',
          preferredFormat: null,
          preferredLanguage: null,
          acquisitionMode: 'automatic',
          notificationEmail: null,
          emailNotifications: false,
          profileType: 'adult',
          defaultLanguage: 'en',
          preferredLanguages: ['sv'],
        },
      }),
    }),
  )
  await page.route('**/api/authors/777/follow', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ following: false, autoAcquire: false, deliveryTargetId: null }),
    }),
  )
  await page.route('**/api/authors/777/catalogue**', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ items: [], next: null }),
    }),
  )
  await page.route('**/api/authors/777', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        id: 777,
        name: 'Lens Author',
        books: [
          { ...SHELF_BOOK, id: 7001, title: 'Swedish Lens Book', authors: ['Lens Author'], language: 'sv' },
          { ...SHELF_BOOK, id: 7002, title: 'English Lens Book', authors: ['Lens Author'], language: 'en' },
        ],
      }),
    }),
  )
  await page.goto(`${base}/authors/777`, { waitUntil: 'networkidle' })
  await page.getByText('Swedish Lens Book').waitFor({ timeout: 8000 })
  expect(
    (await page.getByText('English Lens Book').count()) === 0,
    'the author page must start on the preferred-language lens',
  )
  await page.getByRole('button', { name: 'Show all languages' }).click()
  await page.getByText('English Lens Book').waitFor({ timeout: 8000 })
  expect(
    (await page.getByText('Swedish Lens Book').count()) > 0,
    'All languages keeps the preferred book visible',
  )
}

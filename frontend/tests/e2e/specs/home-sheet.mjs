import { expect } from '../support.mjs'

const BOOK = {
  source: 'discovery',
  ownership: 'missing',
  reasonType: 'because_following',
  reasonLabel: 'Open Library Staff Picks',
  title: 'The Home Sheet Book',
  authors: ['Test Author'],
  blurb: 'A catalogue book with enough description to appear in the Home spotlight.',
  language: 'en',
  subjects: ['Open Library Staff Picks', 'Science Fiction'],
  bookId: null,
  provider: 'openlibrary',
  providerKey: '/works/OLHOMESHEET',
  coverId: 'test-cover',
  cta: 'check',
}
const SECOND_BOOK = {
  ...BOOK,
  title: 'Another Spotlight Book',
  providerKey: '/works/OLHOMESHEET2',
}

export default async function homeSheet(page, { base }) {
  let hasReader = false
  await page.route('**/api/**', (route) => {
    const path = new URL(route.request().url()).pathname
    let body
    if (path === '/api/auth/me') {
      body = { user: {
        id: 4242, username: 'test', displayName: 'Test Reader', role: 'user',
        profileType: 'adult', preferredFormat: 'epub', preferredLanguage: 'en',
        preferredLanguages: ['en'], defaultLanguage: 'en', acquisitionMode: 'automatic',
        notificationEmail: null, emailNotifications: false,
      } }
    } else if (path === '/api/profile/onboarding') {
      body = { onboarded: true, interests: [] }
    } else if (path === '/api/home/spotlight') {
      body = { items: [BOOK, SECOND_BOOK], recommendations: [] }
    } else if (path === '/api/discover/book') {
      body = { ...BOOK, status: 'NOT_IN_LIBRARY', ownedBookId: null, ownedFileId: null,
        onShelf: false, liked: false, description: BOOK.blurb, publisher: null }
    } else if (path === '/api/discover/releases') {
      body = { releases: [] }
    } else if (path === '/api/delivery-targets/default') {
      body = { address: hasReader ? 'reader@example.com' : null, source: hasReader ? 'personal' : null, senderAddress: null, amazonUrl: '' }
    } else if (path.startsWith('/api/discover/cover/')) {
      return route.fulfill({ status: 200, contentType: 'image/svg+xml', body: '<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#486f8a"/></svg>' })
    } else if (path === '/api/books') {
      body = { items: [], total: 0, page: 1, pageSize: 24, letters: [] }
    } else if (path === '/api/home/updates') {
      body = { library: [], discoveries: [], ready: [] }
    } else {
      body = []
    }
    return route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(body) })
  })

  await page.goto(base, { waitUntil: 'networkidle' })
  await page.getByRole('heading', { name: BOOK.title }).waitFor()
  expect(await page.getByText(/open library/i).count() === 0, 'hero copy must not require familiarity with the catalogue provider')
  await page.getByText('Staff picks', { exact: true }).first().waitFor()
  expect(await page.getByRole('heading', { name: 'Recently Added' }).count() === 0, 'Home should hide empty book rows when Spotlight has content')
  expect(await page.getByRole('heading', { name: 'Rediscover your library' }).count() === 0, 'Home should hide empty rediscovery rows')
  const hero = page.getByRole('region', { name: 'Spotlight', exact: true })
  const background = await hero.evaluate((element) => getComputedStyle(element).backgroundColor)
  expect(await hero.locator('[style*="background-color"]').count() === 0, 'Spotlight must not derive its background from the cover')
  await page.getByRole('button', { name: 'Next spotlight book' }).click()
  await page.getByRole('heading', { name: SECOND_BOOK.title }).waitFor()
  expect(await hero.evaluate((element) => getComputedStyle(element).backgroundColor) === background, 'Spotlight keeps a neutral background between books')
  expect(await page.getByRole('navigation', { name: 'Spotlight books' }).getByText('02 / 02').count() === 1, 'spotlight should show a numeric position')
  await page.getByRole('button', { name: 'Previous spotlight book' }).click()
  await page.getByRole('heading', { name: BOOK.title }).waitFor()
  await page.getByRole('link', { name: 'Check availability' }).click()
  await page.waitForURL((url) => url.pathname === '/discover')
  await page.getByRole('dialog').waitFor()
  const shelfAction = page.getByRole('dialog').getByRole('button', { name: 'Get for my shelf' })
  await shelfAction.waitFor()
  expect((await shelfAction.getAttribute('class')).includes('bg-accent'), 'the available shelf action must be a visible primary button')
  expect((await page.getByRole('dialog').getByRole('button', { name: 'Like' }).getAttribute('class')).includes('bg-surface-2'), 'Like must look like a button')
  expect((await page.getByRole('dialog').getByRole('button', { name: 'Get & Send to My Reader' }).count()) === 0, 'sending is unavailable without a reader')
  expect((await page.getByRole('heading', { name: BOOK.title }).count()) > 0, 'Home remains behind the book sheet')
  expect((await page.getByRole('heading', { name: 'Find something worth reading' }).count()) === 0, 'Discover page must not replace Home under the sheet')
  await page.getByRole('button', { name: 'Close' }).last().click()
  await page.waitForURL(base)
  expect((await page.getByRole('dialog').count()) === 0, 'closing the sheet returns to Home')
  await page.goForward()
  await page.getByRole('dialog').waitFor()
  await page.goBack()
  await page.waitForURL(base)

  await page.setViewportSize({ width: 390, height: 844 })
  hasReader = true
  await page.getByRole('link', { name: 'Check availability' }).click()
  const sendAction = page.getByRole('dialog').getByRole('button', { name: 'Get & Send to My Reader' })
  await sendAction.waitFor()
  expect((await sendAction.getAttribute('class')).includes('bg-accent'), 'sending is primary when a reader exists')
  expect((await page.getByRole('dialog').getByRole('button', { name: 'Get for my shelf' }).getAttribute('class')).includes('bg-surface-2'), 'shelf acquisition becomes a secondary choice')
  const mobileSheet = await page.getByRole('dialog').boundingBox()
  expect(mobileSheet.x >= -1 && mobileSheet.x + mobileSheet.width <= 391, 'book sheet must fit a narrow phone')
  for (const width of [320, 390]) {
    await page.setViewportSize({ width, height: 844 })
    const choices = page.getByRole('group', { name: 'Book choices', exact: true })
    const group = await choices.boundingBox()
    const primary = await sendAction.boundingBox()
    const secondary = await choices.getByRole('button', { name: 'Get for my shelf', exact: true }).boundingBox()
    expect(Math.abs(primary.width - group.width) < 2, 'primary acquisition must fill the mobile action row')
    expect(primary.y + primary.height <= secondary.y + 1, 'secondary choices must sit below the primary acquisition')
    for (const action of await choices.locator('button, a').all()) {
      const box = await action.boundingBox()
      expect(box.x >= 0 && box.x + box.width <= width && box.height >= 44, 'book choices must fit and have comfortable touch targets')
    }
  }
  await page.getByRole('button', { name: 'Close' }).last().click()
  await page.waitForURL(base)
  await page.evaluate(() => window.scrollTo(0, 0))
  const spotlightControl = await page.getByRole('button', { name: 'Next spotlight book' }).boundingBox()
  const bottomNav = await page.locator('nav.fixed').boundingBox()
  expect(spotlightControl.y + spotlightControl.height <= bottomNav.y, 'spotlight controls should remain visible above mobile navigation')
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1), 'the shelf line must not cause horizontal scrolling')
}

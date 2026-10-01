import { expect } from '../support.mjs'

const BOOKS = Array.from({ length: 5 }, (_, index) => ({
  id: 8800 + index, title: `The Small Shelf ${index + 1}`, authors: ['Test Author'],
  language: 'en', hasCover: false, hasDescription: true, addedAt: 1,
}))

async function mockSmallShelf(page, { demo = false, child = false, subject = false } = {}) {
  const books = child ? BOOKS.slice(0, 1) : BOOKS
  const requests = []
  await page.route('**/api/**', (route) => {
    const path = new URL(route.request().url()).pathname
    requests.push(path)
    let body = []
    if (path === '/api/auth/me') body = { user: {
      id: 4848, username: 'small-shelf', role: 'user', profileType: child ? 'child' : 'adult',
      preferredLanguages: ['en'], spotlightRotation: false,
    } }
    else if (path === '/api/demo') body = { enabled: demo }
    else if (path === '/api/profile/onboarding') body = { onboarded: true, interests: [] }
    else if (path === '/api/notifications') body = { items: [], unread: 0, pendingRequests: 0, pendingRequestItems: [] }
    else if (path === '/api/books') body = { items: books, total: books.length, page: 1, pageSize: 24, letters: [] }
    else if (path === '/api/books/recent' || path === '/api/books/highlights') body = books
    else if (path === '/api/home/spotlight') body = {
      items: books.map((book) => ({
        ...book, bookId: book.id, source: 'shelf', ownership: 'shelf', reasonType: 'shelf',
        reasonLabel: 'From your shelf', cta: 'explore', subjects: [],
        blurb: 'A reader discovers a story among familiar books and finds a new place to begin.',
      })),
      recommendations: [],
    }
    else if (path === '/api/home/rails') body = child || subject ? [{
      key: child ? 'my-shelf' : 'shelf-adventure', title: child ? 'My shelf' : 'Adventure',
      subject: child ? null : 'adventure', books,
    }] : []
    else if (path === '/api/home/updates') body = { library: [], discoveries: [], ready: [] }
    return route.fulfill({ json: body })
  })
  return { books, requests }
}

export default async function homeRails(page, { base }) {
  for (const scenario of [
    { demo: true, width: 1440 },
    { subject: true, width: 390 },
    { child: true, width: 390 },
  ]) {
    await page.unroute('**/api/**')
    const { books, requests } = await mockSmallShelf(page, scenario)
    await page.setViewportSize({ width: scenario.width, height: 1000 })
    await page.goto(base, { waitUntil: 'networkidle' })
    await page.reload({ waitUntil: 'networkidle' })
    const hero = page.getByRole('region', { name: 'Spotlight', exact: true })
    await hero.getByRole('heading', { name: books[0].title, exact: true }).waitFor()

    const recent = page.getByRole('region', { name: 'Recently Added books', exact: true })
    const rediscover = page.getByRole('region', { name: 'Rediscover your library books', exact: true })
    await recent.waitFor()
    await rediscover.waitFor()
    expect(await recent.locator('.shelf-book').count() === books.length, 'small shelves keep every recent book when all books are also in Spotlight')
    expect(await rediscover.locator('.shelf-book').count() === Math.min(3, books.length), 'Spotlight does not empty the rediscovery selection')
    expect(await page.getByText('Nothing here yet.', { exact: true }).count() === 0, 'Spotlight must not leave an empty subject or child shelf section')
    if (scenario.subject || scenario.child) {
      const title = scenario.child ? 'My shelf' : 'Adventure'
      const rail = page.getByRole('region', { name: `${title} books`, exact: true })
      await rail.waitFor()
      expect(await rail.locator('.shelf-book').count() === books.length, 'subject and child shelf rails retain their Spotlight books')
    }
    if (books.length > 1) {
      const before = await recent.boundingBox()
      await hero.getByRole('button', { name: 'Next spotlight book', exact: true }).click()
      await hero.getByRole('heading', { name: books[1].title, exact: true }).waitFor()
      expect(await recent.locator('.shelf-book').count() === books.length, 'Spotlight navigation keeps rail membership stable')
      expect(Math.abs((await recent.boundingBox()).y - before.y) < 1, 'Spotlight navigation keeps rail placement stable')
    }
    if (scenario.child) {
      expect(!requests.some((path) => ['/api/authors', '/api/collections', '/api/home/updates', '/api/books'].includes(path)), 'child Home continues to fetch only permitted sections')
    }
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), 'small-shelf Home fits the viewport')
  }
}

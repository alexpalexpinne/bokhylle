import { expect } from '../support.mjs'
import { check } from './accessibility.mjs'

const books = Array.from({ length: 48 }, (_, index) => ({ id: 9200 + index, title: `Fictional Browse ${index + 1}`, authors: ['Invented Writer'], hasCover: false, language: 'en' }))
const suggestions = books.slice(0, 36).map((book, index) => ({ ...book, bookId: book.id, source: 'household', ownership: 'household', reasonType: 'taste', reasonLabel: 'Matches your interests: Fantasy', subjects: ['Fantasy'], recommendationKey: String(index + 1).padStart(64, '0'), cta: 'explore' }))

export default async function browseRestoration(page, { base }) {
  await page.route('**/api/**', async (route) => {
    const url = new URL(route.request().url())
    const path = url.pathname
    let body = []
    if (path === '/api/auth/me') body = { user: { id: 9191, username: 'fictional-reader', role: 'user', profileType: 'adult', preferredLanguages: ['en'], spotlightRotation: false } }
    else if (path === '/api/demo') body = { enabled: false }
    else if (path === '/api/profile/onboarding') body = { onboarded: true, interests: ['fantasy'] }
    else if (path === '/api/notifications') body = { items: [], unread: 0, pendingRequests: 0, pendingRequestItems: [] }
    else if (path === '/api/books') {
      const number = Number(url.searchParams.get('page') ?? 1)
      body = { items: books.slice((number - 1) * 24, number * 24), total: books.length, page: number, pageSize: 24, letters: [] }
    } else if (/^\/api\/books\/\d+$/.test(path)) {
      const book = books.find((entry) => entry.id === Number(path.split('/').at(-1)))
      body = { ...book, authorRefs: [], description: 'A fictional book for navigation checks.', availableLanguages: [], subjects: [], editions: [], files: [{ id: book.id, format: 'epub', size: 100, filename: 'fiction.epub' }], onShelf: true, preference: null, metadataSources: [] }
    } else if (path.endsWith('/related')) body = { series: [], author: [], similar: [] }
    else if (path === '/api/books/facets') body = { formats: [], languages: [], subjects: [], series: [], publicationKinds: [] }
    else if (path === '/api/recommendations') body = { items: suggestions, subjects: ['fantasy'], total: suggestions.length, nextOffset: null }
    else if (path === '/api/home/spotlight') body = { items: [], recommendations: suggestions }
    else if (path === '/api/home/updates') body = { library: [], ready: [], discoveries: [] }
    else if (path === '/api/home/subjects') body = { hidden: [] }
    else if (path === '/api/delivery-targets/default') body = { address: null, source: null, senderAddress: null, amazonUrl: '' }
    await route.fulfill({ json: body })
  })

  await page.goto(`${base}/library`, { waitUntil: 'networkidle' })
  await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight))
  await page.waitForFunction(() => document.querySelectorAll('.shelf-grid-cell').length === 48)
  const book = page.getByRole('link').filter({ has: page.getByRole('heading', { name: 'Fictional Browse 40', exact: true }) })
  await book.scrollIntoViewIfNeeded()
  const libraryY = await page.evaluate(() => window.scrollY)
  await book.click()
  await page.waitForURL(`${base}/library/9239`)
  await page.getByRole('heading', { name: 'Fictional Browse 40', exact: true }).waitFor()
  await page.waitForFunction(() => document.activeElement.id === 'main-content')
  await page.clock.setFixedTime(new Date(Date.now() + 70_000))
  await page.goBack()
  await page.waitForURL(`${base}/library`)
  await page.waitForFunction((y) => document.querySelectorAll('.shelf-grid-cell').length === 48 && Math.abs(window.scrollY - y) < 3, libraryY)
  expect(await page.evaluate(() => document.activeElement.getAttribute('href')) === '/library/9239', 'Back restores the book link focus as well as the expanded library and scroll position')

  await page.getByRole('link', { name: 'Home', exact: true }).click()
  await page.getByRole('region', { name: 'Picked for you books', exact: true }).waitFor()
  const rail = page.getByRole('region', { name: 'Picked for you books', exact: true })
  await rail.evaluate((node) => { node.scrollLeft = 800 })
  const railBook = rail.getByRole('link').filter({ has: page.getByRole('heading', { name: 'Fictional Browse 9', exact: true }) })
  await railBook.scrollIntoViewIfNeeded()
  const railX = await rail.evaluate((node) => node.scrollLeft)
  await railBook.click()
  await page.getByRole('heading', { name: 'Fictional Browse 9', exact: true }).waitFor()
  await page.goBack()
  await rail.waitFor()
  await page.waitForFunction((x) => Math.abs(document.querySelector('[data-browse-rail="Picked for you books"]').scrollLeft - x) < 3, railX)

  await page.getByRole('link', { name: 'Explore more', exact: true }).click()
  await page.getByRole('button', { name: 'Show more suggestions', exact: true }).click()
  const recommendation = page.getByRole('link').filter({ has: page.getByRole('heading', { name: 'Fictional Browse 30', exact: true }) })
  await recommendation.scrollIntoViewIfNeeded()
  const recommendationY = await page.evaluate(() => window.scrollY)
  await recommendation.click()
  await page.getByRole('heading', { name: 'Fictional Browse 30', exact: true }).waitFor()
  await page.goBack()
  await page.waitForFunction((y) => document.querySelectorAll('.shelf-grid-cell').length === 36 && Math.abs(window.scrollY - y) < 3, recommendationY)

  // The tablet range previously had a bottom bar without matching page padding.
  for (const width of [390, 700]) {
    await page.setViewportSize({ width, height: 900 })
    await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight))
    const bar = await page.getByRole('navigation', { name: 'Main navigation' }).filter({ visible: true }).boundingBox()
    const last = await page.locator('.shelf-grid-cell').last().boundingBox()
    expect(last.y + last.height < bar.y, `the final book remains above mobile navigation at ${width}px`)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), `no horizontal page scrolling at ${width}px`)
  }
  await page.setViewportSize({ width: 1440, height: 1000 })
  await page.goto(`${base}/recommendations`, { waitUntil: 'networkidle' })
  await page.keyboard.press('Tab')
  expect(await page.getByRole('link', { name: 'Skip to content', exact: true }).evaluate((node) => node === document.activeElement), 'the first keyboard stop is Skip to content')
  await page.keyboard.press('Enter')
  expect(await page.evaluate(() => document.activeElement.id) === 'main-content', 'Skip to content focuses the main landmark')
  const trigger = page.getByRole('button', { name: 'Change suggestions for Fictional Browse 1', exact: true })
  await trigger.click()
  const dialog = page.getByRole('dialog', { name: 'Your suggestion: Fictional Browse 1', exact: true })
  const bounds = await dialog.boundingBox()
  expect(bounds.width <= 340, 'recommendation feedback uses a compact desktop popover')
  await check(page, 'recommendation desktop popover')
  await page.keyboard.press('Shift+Tab')
  expect(await dialog.getByRole('button', { name: 'Show something else', exact: true }).evaluate((node) => node === document.activeElement), 'the feedback popover traps keyboard focus')
  await page.keyboard.press('Escape')
  expect(await trigger.evaluate((node) => node === document.activeElement), 'Escape returns focus to the feedback trigger')
  expect(await page.locator('#root').evaluate((node) => !node.inert), 'closing the popover restores access to the page')
  for (const theme of ['paper', 'ink']) {
    await page.evaluate((value) => { localStorage.setItem('bokhylle.theme', value); document.documentElement.dataset.theme = value }, theme)
    await page.setViewportSize({ width: 390, height: 900 })
    await trigger.click()
    const sheet = await dialog.boundingBox()
    expect(sheet.width === 390 && sheet.y + sheet.height >= 899, 'mobile feedback is a sheet at the bottom of the viewport')
    await check(page, `recommendation mobile sheet in ${theme}`)
    await page.keyboard.press('Escape')
    await check(page, `recommendations mobile page in ${theme}`)
  }
}

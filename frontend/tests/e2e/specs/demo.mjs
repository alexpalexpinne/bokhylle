import { expect } from '../support.mjs'

const USER = {
  id: 101, username: 'demo_adult_test', displayName: 'Demo Reader', role: 'user',
  profileType: 'adult', preferredLanguages: ['en'], preferredLanguage: 'en', defaultLanguage: 'en',
}
const BOOK = {
  provider: 'local', providerKey: 'local:77', title: 'Demo Book', authors: ['Demo Author'],
  languages: ['en'], language: 'en', coverId: null, year: 1900,
  status: 'IN_LIBRARY', ownedBookId: 77, ownedFileId: 10, onShelf: false,
}
const COVER = '<svg xmlns="http://www.w3.org/2000/svg" width="120" height="180"><rect width="120" height="180" fill="#8b5134"/></svg>'

export default async function demo(page, { base }) {
  let pending = false
  let ready = false
  let sendStatus = null
  let getCount = 0
  let sendCount = 0
  let sendWhenReady = false
  const realActions = []
  await page.route('**/api/**', (route) => {
    const request = route.request()
    const url = new URL(request.url())
    const path = url.pathname
    const json = (body, status = 200) => route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) })
    if (path === '/api/auth/me') return json({ user: USER })
    if (path === '/api/demo') return json({ enabled: true })
    if (path === '/api/profile/onboarding') return json({ onboarded: true, interests: [] })
    if (path === '/api/home/spotlight') return json({ items: [], recommendations: [{
      bookId: 77, title: BOOK.title, authors: BOOK.authors, source: 'household', ownership: 'household',
      reasonType: 'household', reasonLabel: 'Matches your interests', cta: 'explore', subjects: [],
    }] })
    if (path === '/api/home/updates') return json({ library: [], ready: [], discoveries: [{
      authorId: 1, title: BOOK.title, authors: BOOK.authors, provider: 'local', providerKey: 'local:77',
    }] })
    if (path === '/api/books') return json({ items: [], total: 1, page: 1, pageSize: 24, letters: [] })
    if (path === '/api/notifications') return json({ items: [], unread: 0, pendingRequests: 0, pendingRequestItems: [] })
    if (path === '/api/books/77/cover') return route.fulfill({ contentType: 'image/svg+xml', body: COVER })
    if (path === '/api/discover') return json({
      query: 'Demo', books: [{ ...BOOK, onShelf: ready, status: pending ? 'DOWNLOADING' : 'IN_LIBRARY' }],
      authors: { local: [{ authorId: 1, name: 'Demo Author', following: false, bookCount: 1 }], external: [] },
      next: null, local: true, provider: null,
    })
    if (path === '/api/books/77') return json({
      id: 77, title: BOOK.title, authors: BOOK.authors, authorRefs: [], hasCover: true,
      files: [{ id: 10, editionId: 1, format: 'epub', size: 123, filename: 'demo.epub' }],
      onShelf: ready, preference: null, subjects: [], editions: [],
    })
    if (path === '/api/books/77/related') return json({ series: [], author: [], similar: [] })
    if (path === '/api/books/77/shelf-users') return json({ users: [] })
    if (path === '/api/demo/get') {
      getCount += 1
      pending = true
      sendWhenReady = request.postDataJSON().sendWhenReady
      return json({ id: 'get-1' })
    }
    if (path === '/api/demo/send') {
      sendCount += 1
      sendStatus = 'PREPARING'
      return json({ id: 1, status: 'SIMULATED', message: 'On its way to Demo Kindle.' })
    }
    if (path === '/api/demo/activity') {
      if (sendWhenReady && ready && !sendStatus) sendStatus = 'PREPARING'
      return json({
      gets: getCount ? [{ id: 'get-1', bookId: 77, title: BOOK.title, status: pending ? 'LOOKING' : 'READY', startedAt: 1, readyAt: 13, completedAt: pending ? null : 13, sendWhenReady }] : [],
      sends: sendStatus ? [{ id: 1, bookId: 77, title: BOOK.title, status: sendStatus, createdAt: 1 }] : [],
    })
    }
    if (path.startsWith('/api/delivery') || path.includes('/acquisitions')) realActions.push(path)
    return json([])
  })

  await page.goto(base, { waitUntil: 'networkidle' })
  for (const label of ['Picked for you books', 'Books from authors you follow']) {
    const rail = page.getByRole('region', { name: label, exact: true })
    await rail.waitFor()
    expect(await rail.locator('img').getAttribute('src') === '/api/books/77/cover', 'sample recommendations use local covers')
    expect(await rail.locator('a').getAttribute('href') === '/library/77', 'sample recommendations open a usable local book page')
  }
  await page.getByRole('region', { name: 'Picked for you books', exact: true }).getByRole('link').click()
  await page.waitForURL(`${base}/library/77`)
  await page.getByRole('button', { name: 'Get for my shelf', exact: true }).waitFor()

  await page.goto(`${base}/discover?q=Demo&type=author`, { waitUntil: 'networkidle' })
  const card = page.locator('article').filter({ has: page.getByRole('button', { name: 'Open details for Demo Book' }) })
  const cover = card.locator('img')
  await cover.waitFor()
  expect(await cover.getAttribute('src') === '/api/books/77/cover', 'demo search must use the local cover')
  expect(await cover.evaluate((img) => img.complete && img.naturalWidth > 0), 'demo cover must load')
  const booksTop = (await page.getByText('Results', { exact: true }).boundingBox()).y
  const authorsTop = (await page.getByText('Authors', { exact: true }).boundingBox()).y
  expect(booksTop < authorsTop, 'author searches must also put books first')
  await page.getByRole('button', { name: 'Open details for Demo Book' }).click()
  const sheet = page.getByRole('dialog', { name: BOOK.title, exact: true })
  expect(await sheet.locator('img').getAttribute('src') === '/api/books/77/cover', 'demo detail sheet must use the local cover')
  await sheet.getByRole('button', { name: 'Get & Send to Kindle', exact: true }).waitFor()
  await sheet.getByRole('button', { name: 'Get for my shelf', exact: true }).click()
  expect(sendWhenReady === false, 'shelf-only acquisition must not queue Kindle delivery')
  await page.waitForURL(`${base}/activity`)
  await page.getByText('Demo acquisition in progress').waitFor()
  await page.goto(`${base}/discover?q=Demo&type=title`, { waitUntil: 'networkidle' })
  expect(await page.getByRole('button', { name: 'Try Get', exact: true }).count() === 0, 'pending demo Get must not be offered again')
  await page.getByRole('button', { name: 'Open details for Demo Book' }).click()
  await page.getByRole('link', { name: 'Follow in Activity' }).waitFor()
  await page.goto(`${base}/library/77`, { waitUntil: 'networkidle' })
  expect(await page.getByRole('button', { name: 'Getting…', exact: true }).isDisabled(), 'book page must disable repeated Get during acquisition')
  expect(getCount === 1, 'the journey must acquire the book only once')

  ready = true
  pending = false
  await page.reload({ waitUntil: 'networkidle' })
  await page.getByRole('button', { name: 'Send to Demo Kindle', exact: true }).click()
  let dialog = page.getByRole('dialog', { name: 'Send to Demo Kindle', exact: true })
  await dialog.getByText('EPUB → Demo Kindle').waitFor()
  await dialog.getByRole('button', { name: 'Cancel', exact: true }).click()
  expect(sendCount === 0, 'cancelling the preview must not start a delivery')
  await page.getByRole('button', { name: 'Send to Demo Kindle', exact: true }).click()
  dialog = page.getByRole('dialog', { name: 'Send to Demo Kindle', exact: true })
  await dialog.getByRole('button', { name: 'Simulate send', exact: true }).click()
  await page.waitForURL(`${base}/activity`)
  await page.getByText('Preparing your book for Demo Kindle…').waitFor()
  sendStatus = 'SENDING'
  await page.getByText('Your book is on its way to Demo Kindle…').waitFor()
  sendStatus = 'DELIVERED'
  await page.getByText('Delivered to Demo Kindle — simulation complete.').waitFor()
  await page.reload({ waitUntil: 'networkidle' })
  await page.getByText('Delivered to Demo Kindle — simulation complete.').waitFor()
  expect(sendCount === 1, 'the journey must start one demo delivery')
  await page.goto(`${base}/discover?q=Demo&type=title`, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: 'Open details for Demo Book' }).click()
  await sheet.getByRole('button', { name: 'Send to Demo Kindle', exact: true }).click()
  await page.getByRole('dialog', { name: 'Send to Demo Kindle', exact: true }).getByRole('button', { name: 'Cancel', exact: true }).click()
  await sheet.waitFor()
  expect(sendCount === 1, 'cancelling Discover send must return to the book without starting a delivery')

  await page.keyboard.press('Escape')
  ready = false
  pending = false
  sendStatus = null
  await page.setViewportSize({ width: 320, height: 844 })
  await page.goto(`${base}/discover?q=Demo&type=title`, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: 'Open details for Demo Book' }).click()
  const getAndSend = sheet.getByRole('button', { name: 'Get & Send to Kindle', exact: true })
  const sendBox = await getAndSend.boundingBox()
  const choicesBox = await sheet.getByRole('group', { name: 'Book choices' }).boundingBox()
  expect(Math.abs(sendBox.width - choicesBox.width) < 2, 'demo Get & Send must fill the primary mobile row')
  await getAndSend.click()
  await page.waitForURL(`${base}/activity`)
  expect(sendWhenReady === true, 'demo Get & Send must persist delivery intent during acquisition')
  await page.getByText('It will be sent to Demo Kindle as soon as it is on your shelf.').waitFor()
  ready = true
  pending = false
  await page.getByText('Preparing your book for Demo Kindle…').waitFor()
  sendStatus = 'DELIVERED'
  await page.getByText('Delivered to Demo Kindle — simulation complete.').waitFor()
  expect(sendCount === 1, 'automatic delivery must not require a separate Send request')

  ready = false
  pending = false
  sendWhenReady = false
  await page.goto(`${base}/library/77`, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: 'Get for my shelf', exact: true }).waitFor()
  await page.getByRole('button', { name: 'Get & Send to Kindle', exact: true }).click()
  await page.waitForURL(`${base}/activity`)
  expect(sendWhenReady === true, 'full book details must also support Get & Send')
  expect(realActions.length === 0, 'demo must not contact real acquisition or delivery endpoints')
}

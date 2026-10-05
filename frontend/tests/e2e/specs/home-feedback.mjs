import { expect } from '../support.mjs'

export default async function homeFeedback(page, { base }) {
  const book = { id: 8101, title: 'An Imaginary Journey', authors: ['Fictional Writer'], language: 'en', hasCover: false }
  const hero = {
    ...book, bookId: book.id, source: 'household', ownership: 'household', reasonType: 'taste',
    reasonLabel: 'Because you like Space opera', subjects: ['Space opera'], cta: 'explore',
    blurb: 'A reader discovers a lost story and follows a distant light through unfamiliar worlds to find a place to call home.',
  }
  const suggestion = { ...hero, bookId: null, source: 'discover', ownership: 'discover', provider: 'fake', providerKey: 'fictional-catalogue', title: 'A Catalogue Journey' }
  const initialSpotlight = { items: [hero], recommendations: [suggestion] }
  let hidden = false
  let heldRefresh = false
  let release
  const gate = new Promise((resolve) => { release = resolve })
  await page.route('**/api/**', async (route) => {
    const url = new URL(route.request().url())
    const path = url.pathname
    let body = []
    if (path === '/api/auth/me') body = { user: { id: 8181, username: 'fictional-reader', role: 'user', profileType: 'adult', preferredLanguages: ['en'], spotlightRotation: false } }
    else if (path === '/api/demo') body = { enabled: false }
    else if (path === '/api/profile/onboarding') body = { onboarded: true, interests: [] }
    else if (path === '/api/notifications') body = { items: [], unread: 0, pendingRequests: 0, pendingRequestItems: [] }
    else if (path === '/api/books') body = { items: [], total: 0, page: 1, pageSize: 24, letters: [] }
    else if (path === '/api/books/highlights') body = hidden ? [] : [book]
    else if (path === '/api/home/rails') body = hidden ? [] : [{ key: 'shelf-space-opera', title: 'Space opera', subject: 'space opera', books: [book] }]
    else if (path.startsWith('/api/home/subjects/')) { hidden = JSON.parse(route.request().postData()).hidden; body = { ok: true } }
    else if (path === '/api/home/updates') body = { library: [], ready: [], discoveries: hidden ? [] : [{ authorId: 1, authors: book.authors, title: 'Another Imaginary Journey', provider: 'fake', providerKey: 'followed-fiction', year: 2020 }] }
    else if (path === '/api/home/spotlight') {
      if (url.searchParams.get('cachedOnly') === 'false' && !heldRefresh) {
        heldRefresh = true
        await gate
        body = initialSpotlight
      } else body = hidden ? { items: [], recommendations: [] } : initialSpotlight
    }
    await route.fulfill({ json: body })
  })
  await page.goto(base, { waitUntil: 'domcontentloaded' })
  await page.getByRole('region', { name: 'Picked for you books', exact: true }).waitFor()
  await page.getByRole('region', { name: 'Rediscover your library books', exact: true }).waitFor()
  await page.getByRole('button', { name: 'Not interested in Space opera', exact: true }).click()
  await page.getByRole('heading', { name: 'Picked for you', exact: true }).waitFor({ state: 'detached' })
  await page.getByRole('heading', { name: 'Rediscover your library', exact: true }).waitFor({ state: 'detached' })
  await page.getByRole('heading', { name: 'From authors you follow', exact: true }).waitFor({ state: 'detached' })
  expect(await page.getByRole('region', { name: 'Spotlight', exact: true }).count() === 0, 'explicit feedback filters Spotlight in the current visit')
  release()
  await page.waitForLoadState('networkidle')
  expect(await page.getByText('A Catalogue Journey', { exact: true }).count() === 0, 'an older catalogue response cannot restore hidden suggestions')
  await page.getByRole('button', { name: 'Undo', exact: true }).click()
  await page.getByRole('region', { name: 'Picked for you books', exact: true }).waitFor()
  expect(await page.getByRole('region', { name: 'Spotlight', exact: true }).count() === 1, 'restoring a subject updates every recommendation surface')
  await page.getByRole('button', { name: 'Not interested in Space opera', exact: true }).click()
  await page.getByRole('heading', { name: 'Picked for you', exact: true }).waitFor({ state: 'detached' })
  await page.reload({ waitUntil: 'networkidle' })
  expect(await page.getByText('A Catalogue Journey', { exact: true }).count() === 0, 'the saved Home snapshot preserves feedback after reload')
}

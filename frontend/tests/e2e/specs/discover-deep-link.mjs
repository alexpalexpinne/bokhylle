import { expect } from '../support.mjs'

export default async function discoverDeepLink(page, { base }) {
  let releaseDetail
  const detailReady = new Promise((resolve) => { releaseDetail = resolve })
  let acquired = false
  let writes = 0
  let sharing
  await page.route('**/api/**', async (route) => {
    const path = new URL(route.request().url()).pathname
    const json = (body, status = 200) => route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) })
    if (path === '/api/auth/me') return json({ user: { id: 88, username: 'reader', role: 'user', profileType: 'adult', preferredLanguages: ['en'], preferredFormat: 'epub', defaultBookSharing: 'private' } })
    if (path === '/api/demo') return json({ enabled: false })
    if (path === '/api/discover/book') {
      await detailReady
      return json({ provider: 'openlibrary', providerKey: '/works/FOUNDATION', title: 'Foundation', authors: ['Isaac Asimov'], coverId: null, description: null,
        status: acquired ? 'DOWNLOADING' : 'NOT_IN_LIBRARY', ownedBookId: acquired ? 77 : null, ownedFileId: null, onShelf: false, liked: false })
    }
    if (path === '/api/discover/acquisitions') {
      sharing = route.request().postDataJSON().sharing
      acquired = true
      writes += 1
      return json({ id: 'foundation-get', bookId: 77, status: 'REQUESTED', duplicate: false }, 202)
    }
    if (path === '/api/delivery-targets/default') return json({ address: null, source: null })
    if (path === '/api/discover/releases') return json({ releases: [] })
    if (path === '/api/notifications') return json({ items: [], unread: 0, pendingRequests: 0, pendingRequestItems: [] })
    return json([])
  })
  const url = `${base}/discover?provider=openlibrary&providerKey=%2Fworks%2FFOUNDATION`
  await page.goto(url, { waitUntil: 'domcontentloaded' })
  const get = page.getByRole('button', { name: 'Get for my shelf', exact: true })
  await get.waitFor()
  expect(await get.isDisabled(), 'Get must wait for deep-linked book details')
  releaseDetail()
  await page.getByRole('heading', { name: 'Foundation', exact: true }).waitFor()
  const choice = page.getByRole('combobox', { name: /^Book sharing/ })
  expect(await choice.inputValue() === 'private', 'Get starts with the account sharing default')
  await choice.selectOption('shared')
  await get.click()
  await page.getByText('Getting "Foundation" — it will appear on your shelf.', { exact: true }).waitFor()
  await page.goto(url, { waitUntil: 'networkidle' })
  expect(await page.getByRole('button', { name: 'Get for my shelf', exact: true }).count() === 0, 'deep link must not offer Get while the book is already on its way')
  expect(writes === 1, 'Foundation must be acquired only once')
  expect(sharing === 'shared', 'Get sends the override selected before acquisition')
}

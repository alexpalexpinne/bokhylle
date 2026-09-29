import { expect } from '../support.mjs'

const USER = {
  id: 1, username: 'reader', displayName: 'Reader', role: 'admin', profileType: 'adult',
  preferredLanguages: ['en'], preferredLanguage: 'en', defaultLanguage: 'en',
}
const FILES = [
  { id: 10, editionId: 1, format: 'epub', size: 420000, filename: 'book.epub' },
  { id: 11, editionId: 1, format: 'pdf', size: 840000, filename: 'book.pdf' },
]

export default async function bookDetail(page, { base }) {
  let user = USER
  let demo = false
  let book = {
    id: 77, title: 'Test Detail Book', authors: ['Test Author'], authorRefs: [],
    description: 'A test book.', language: 'en', availableLanguages: [], publicationYear: 2020,
    series: null, seriesNumber: null, rating: null, ratingCount: null,
    subjects: [], editions: [], files: FILES, onShelf: true, preference: null,
  }
  const collections = [{ id: 1, name: 'Original', bookCount: 1 }, { id: 2, name: 'Selected', bookCount: 0 }]
  const memberships = new Set([1])
  const writes = []
  let onChildShelf = false
  let failAccess = true
  let failRetry = true
  let failEdit = true
  let failFileDelete = true
  let deliveries = [{ id: 1, bookId: 77, address: 'old-reader@example.com', status: 'FAILED', errorMessage: 'Mail unavailable', createdAt: 1 }]
  let deliveryReads = 0
  const forbiddenChildReads = []
  const forbiddenDemoReads = []

  await page.route('**/api/**', (route) => {
    const request = route.request()
    const path = new URL(request.url()).pathname
    const method = request.method()
    const input = request.postDataJSON()
    const protectedPath = path === '/api/delivery-targets' || path === '/api/deliveries' || path.endsWith('/shelf-users') || path.startsWith('/api/admin/') || path.startsWith('/api/collections')
    if (user.profileType === 'child' && protectedPath) forbiddenChildReads.push(path)
    if (demo && (path === '/api/deliveries' || path === '/api/delivery-targets' || path.startsWith('/api/collections'))) forbiddenDemoReads.push(path)
    if (method !== 'GET') writes.push({ path, method, input })
    const json = (body, status = 200) => route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) })
    const error = (message) => json({ code: 'internal_error', message }, 500)
    if (path === '/api/auth/me') return json({ user })
    if (path === '/api/demo') return json({ enabled: demo })
    if (path === '/api/notifications') return json({ items: [], unread: 0, pendingRequests: 0, pendingRequestItems: [] })
    if (path === '/api/books/77') return json(book)
    if (path === '/api/books/77/related') return json({ series: [], author: [], similar: [] })
    if (path === '/api/books/77/shelf-users') return json({ users: [{ userId: 2, displayName: 'Kid', onShelf: onChildShelf }] })
    if (path === '/api/users/2/shelf/77') {
      if (failAccess) { failAccess = false; return error('Shelf update unavailable') }
      onChildShelf = input.onShelf
      return json({ ok: true })
    }
    if (path === '/api/books/77/collections') return json(collections.filter((collection) => memberships.has(collection.id)))
    if (path === '/api/collections') {
      if (method === 'POST') {
        const created = { id: 3, name: input.name, bookCount: 0 }
        collections.push(created)
        return json(created, 201)
      }
      return json(collections)
    }
    if (/^\/api\/collections\/\d+\/books/.test(path)) {
      const id = Number(path.split('/')[3])
      if (method === 'POST') memberships.add(id)
      else memberships.delete(id)
      return route.fulfill({ status: 204 })
    }
    if (path === '/api/delivery-targets') return json([{ id: 1, name: 'Reader', type: 'other', address: 'reader@example.com', enabled: true, isDefault: true }])
    if (path === '/api/delivery-targets/default') return json({ address: 'reader@example.com', source: 'personal', senderAddress: null, amazonUrl: '' })
    if (path === '/api/deliveries') { deliveryReads += 1; return json(deliveries) }
    if (path === '/api/deliveries/1/retry') {
      if (failRetry) { failRetry = false; return error('Retry unavailable') }
      deliveries = [{ ...deliveries[0], status: 'SENT', errorMessage: null }]
      return json(deliveries[0])
    }
    if (path === '/api/books/77/files/10/deliver') {
      const delivery = { id: 2, bookId: 77, address: 'reader@example.com', status: 'SENT', errorMessage: null, createdAt: 2 }
      deliveries.push(delivery)
      return json(delivery)
    }
    if (path === '/api/admin/books/77' && method === 'PUT') {
      if (failEdit) { failEdit = false; return error('Metadata unavailable') }
      book = { ...book, ...input }
      return json({ ok: true })
    }
    if (path === '/api/admin/books/77/files/11') {
      if (failFileDelete) { failFileDelete = false; return error('File deletion unavailable') }
      book = { ...book, files: book.files.filter((file) => file.id !== 11) }
      return route.fulfill({ status: 204 })
    }
    if (path === '/api/admin/books/77' && method === 'DELETE') return route.fulfill({ status: 204 })
    if (path.endsWith('/cover')) return route.fulfill({ status: 404 })
    if (path === '/api/books') return json({ items: [], total: 0, page: 1, pageSize: 24, letters: [] })
    if (path === '/api/books/facets') return json({ languages: [], formats: [], subjects: [], series: [] })
    if (path === '/api/household/members') return json({ members: [] })
    if (path === '/api/profile/onboarding') return json({ onboarded: true, interests: [] })
    return json([])
  })

  await page.goto(`${base}/library/77`, { waitUntil: 'networkidle' })
  await page.getByRole('heading', { name: book.title, exact: true }).waitFor()
  for (const width of [320, 390]) {
    await page.setViewportSize({ width, height: 844 })
    const send = await page.getByRole('button', { name: 'Send to my reader', exact: true }).boundingBox()
    const actions = await page.getByRole('group', { name: 'Reading actions', exact: true }).boundingBox()
    expect(Math.abs(send.width - actions.width) < 2, 'reader sending must fill the mobile primary row')
    for (const download of await page.locator('a[href*="/download"]').all()) {
      const box = await download.boundingBox()
      expect(box.x >= 0 && box.x + box.width <= width && box.height >= 44, 'each download must fit as a touch-friendly file row')
    }
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), 'book details must not overflow on mobile')
  }
  await page.setViewportSize({ width: 1440, height: 1000 })

  await page.getByRole('button', { name: 'Manage collections' }).click()
  let dialog = page.getByRole('dialog', { name: 'Collections', exact: true })
  await dialog.getByRole('checkbox', { name: /Original/ }).uncheck()
  await dialog.getByRole('checkbox', { name: /Selected/ }).check()
  await dialog.getByRole('button', { name: 'Save', exact: true }).click()
  await dialog.waitFor({ state: 'detached' })
  expect(memberships.has(2) && !memberships.has(1), 'saving collections must add the selected membership and remove the old one')
  await page.getByRole('button', { name: 'Manage collections' }).click()
  await dialog.getByPlaceholder('New collection name').fill('New shelf')
  await dialog.getByRole('button', { name: 'Add', exact: true }).click()
  await dialog.getByRole('checkbox', { name: /New shelf/ }).waitFor()
  expect(await dialog.getByRole('checkbox', { name: /New shelf/ }).isChecked(), 'a new collection must be selected')
  await dialog.getByRole('button', { name: 'Save', exact: true }).click()
  await dialog.waitFor({ state: 'detached' })
  expect(memberships.has(3), 'saving must assign the book to the newly created collection')
  expect(writes.filter((write) => write.path === '/api/collections/2/books').length === 1, 'reopening collections must reload persisted memberships')

  await page.getByLabel('Book actions').click()
  await page.getByRole('button', { name: 'Manage access…', exact: true }).click()
  dialog = page.getByRole('dialog', { name: 'Available to', exact: true })
  const child = dialog.getByRole('button', { name: /Kid/ })
  await child.click()
  await dialog.getByText('Shelf update unavailable', { exact: true }).waitFor()
  expect(await child.getAttribute('aria-pressed') === 'false', 'failed access updates must leave the child shelf unchanged')
  await child.click()
  await dialog.getByText('On their shelf', { exact: true }).waitFor()
  expect(onChildShelf, 'successful access updates must target the selected child and book')
  await page.keyboard.press('Escape')
  await dialog.waitFor({ state: 'detached' })

  await page.getByRole('button', { name: 'Retry', exact: true }).click()
  await page.getByRole('alert').getByText('Retry unavailable', { exact: true }).waitFor()
  await page.getByRole('button', { name: 'Retry', exact: true }).click()
  await page.getByText('Delivered', { exact: true }).waitFor()
  const beforeSend = deliveryReads
  await page.getByRole('button', { name: 'Send to my reader', exact: true }).click()
  dialog = page.getByRole('dialog', { name: 'Send to your reader', exact: true })
  await dialog.getByRole('button', { name: 'Send', exact: true }).click()
  await dialog.getByRole('button', { name: 'Done', exact: true }).click()
  await page.getByText(/^reader@example\.com/).waitFor()
  expect(deliveryReads > beforeSend, 'successful sending must refresh delivery history')

  await page.getByRole('button', { name: 'Fix details', exact: true }).click()
  dialog = page.getByRole('dialog', { name: 'Fix details', exact: true })
  await dialog.getByLabel('Title', { exact: true }).fill('Updated Book')
  await dialog.getByLabel('Authors (comma separated)').fill(' One, Two ')
  await dialog.getByRole('button', { name: 'Save', exact: true }).click()
  await dialog.getByText('Metadata unavailable', { exact: true }).waitFor()
  expect(await dialog.getByLabel('Title', { exact: true }).inputValue() === 'Updated Book', 'a failed edit must preserve the draft')
  await dialog.getByRole('button', { name: 'Save', exact: true }).click()
  await page.getByRole('heading', { name: 'Updated Book', exact: true }).waitFor()
  expect(book.authors.join(',') === 'One,Two', 'metadata edits must trim comma-separated authors')

  await page.getByRole('button', { name: 'Delete PDF file', exact: true }).click()
  dialog = page.getByRole('dialog', { name: 'Delete the PDF file?', exact: true })
  await dialog.getByRole('button', { name: 'Cancel', exact: true }).click()
  expect(!writes.some((write) => write.path === '/api/admin/books/77/files/11'), 'cancel must not delete a file')
  await page.getByRole('button', { name: 'Delete PDF file', exact: true }).click()
  await dialog.getByRole('button', { name: 'Delete file', exact: true }).click()
  await dialog.getByText('File deletion unavailable', { exact: true }).waitFor()
  await dialog.getByRole('button', { name: 'Delete file', exact: true }).click()
  await page.getByRole('button', { name: 'Delete PDF file', exact: true }).waitFor({ state: 'detached' })
  expect(book.files.length === 1 && book.files[0].id === 10, 'file deletion must refresh the book and keep its other files')

  user = { ...USER, role: 'user' }
  await page.reload({ waitUntil: 'networkidle' })
  expect(await page.getByLabel('Book actions').count() === 0, 'ordinary members must not see admin actions')
  expect(await page.getByRole('button', { name: /Delete EPUB file/ }).count() === 0, 'ordinary members must not see file deletion')
  expect(await page.getByRole('button', { name: 'Manage access', exact: true }).count() === 0, 'ordinary members must not assign child shelves')
  user = USER
  await page.reload({ waitUntil: 'networkidle' })
  await page.setViewportSize({ width: 390, height: 844 })
  await page.getByLabel('Book actions').click()
  await page.getByRole('button', { name: 'Manage access…', exact: true }).click()
  dialog = page.getByRole('dialog', { name: 'Available to', exact: true })
  const box = await dialog.boundingBox()
  expect(box.x >= -1 && box.x + box.width <= 391, 'household access must fit a narrow phone')
  await page.keyboard.press('Escape')

  user = { ...USER, role: 'user', profileType: 'child' }
  await page.reload({ waitUntil: 'networkidle' })
  expect(forbiddenChildReads.length === 0, `child details must not fetch protected data: ${forbiddenChildReads.join(', ')}`)
  for (const name of ['Manage collections', 'Manage access', 'Send to my reader']) {
    expect(await page.getByRole('button', { name, exact: true }).count() === 0, `children must not see ${name}`)
  }
  expect(await page.locator('a[href*="/download"]').count() === 0, 'children must not see download links')

  user = { ...USER, role: 'user' }
  demo = true
  await page.reload({ waitUntil: 'networkidle' })
  expect(forbiddenDemoReads.length === 0, 'demo details must not load real readers, collections, or delivery history')
  expect(await page.getByRole('button', { name: 'Manage collections' }).count() === 0, 'demo details must not offer collection management')

  user = USER
  demo = false
  await page.setViewportSize({ width: 1440, height: 1000 })
  await page.reload({ waitUntil: 'networkidle' })
  await page.getByLabel('Book actions').click()
  await page.getByRole('button', { name: 'Delete book', exact: true }).click()
  dialog = page.getByRole('dialog', { name: 'Delete "Updated Book"?', exact: true })
  await dialog.getByRole('button', { name: 'Cancel', exact: true }).click()
  expect(!writes.some((write) => write.path === '/api/admin/books/77' && write.method === 'DELETE'), 'cancel must not delete the book')
  await page.getByRole('button', { name: 'Delete book', exact: true }).click()
  await dialog.getByRole('button', { name: 'Delete book', exact: true }).click()
  await page.waitForURL(`${base}/library`)
}

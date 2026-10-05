import { expect } from '../support.mjs'

export default async function profileHelp(page, { base }) {
  let targets = []
  let statsRequests = 0
  await page.route('**/api/**', (route) => {
    const url = new URL(route.request().url())
    let body
    if (url.pathname === '/api/auth/me') {
      body = { user: {
        id: 4545, username: 'reader', displayName: 'Test Reader', role: 'user',
        profileType: 'adult', preferredFormat: 'epub', preferredLanguage: 'en',
        preferredLanguages: ['en'], defaultLanguage: 'en', acquisitionMode: 'automatic',
        notificationEmail: null, emailNotifications: false,
      } }
    } else if (url.pathname === '/api/profile/stats') {
      statsRequests += 1
      body = { shelf: 3, authors: 2, liked: 1, booksSent: 4 }
    } else if (url.pathname === '/api/profile/rejected') {
      body = { items: [] }
    } else if (url.pathname === '/api/profile/liked') {
      body = { items: [{ bookId: 1, title: 'Liked Book', authors: [], readable: false, onShelf: false, provider: null, providerKey: null }] }
    } else if (url.pathname === '/api/profile/hidden-subjects') {
      body = { hidden: [] }
    } else if (url.pathname.endsWith('/tokens')) {
      body = { tokens: [] }
    } else if (url.pathname === '/api/delivery-targets/default') {
      body = { address: null, source: null, senderAddress: null, amazonUrl: '' }
    } else if (url.pathname === '/api/delivery-targets') {
      body = targets
    } else if (url.pathname === '/api/delivery-targets/1' && route.request().method() === 'DELETE') {
      targets = []
      return route.fulfill({ status: 204 })
    } else {
      body = []
    }
    return route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(body) })
  })

  await page.goto(`${base}/profile`, { waitUntil: 'networkidle' })
  const summary = page.getByRole('region', { name: 'Your reading at a glance' })
  await summary.getByText('On my shelf').waitFor()
  expect((await summary.innerText()).includes('3\nOn my shelf'), 'shelf count is personal')
  expect((await summary.innerText()).includes('2\nAuthors followed'), 'followed author count is personal')
  expect((await summary.innerText()).includes('1\nBooks I like'), 'like count is personal')
  expect((await summary.innerText()).includes('4\nBooks sent to readers'), 'delivered books count is personal')
  expect(statsRequests === 1, 'overview counts should load together')
  expect((await page.getByText('Formats, languages, requests, and notifications.').count()) === 0, 'section cards should not repeat the navigation')

  await page.getByRole('link', { name: 'Send to readers', exact: true }).click()
  await page.waitForURL(`${base}/profile/readers`)
  await page.getByText('No readers yet').waitFor()
  expect((await page.getByRole('button', { name: 'Add reader' }).count()) === 1, 'an empty reader list has one action')
  targets = [{ id: 1, userId: 4545, type: 'kindle', name: 'Kindle', address: 'reader@kindle.com', connector: 'email', enabled: true, isDefault: true, createdAt: 1, updatedAt: 1 }]
  await page.reload({ waitUntil: 'networkidle' })
  const readerSection = page.getByRole('heading', { name: 'My readers' }).locator('..').locator('..')
  expect((await readerSection.getByRole('button', { name: 'Add reader' }).count()) === 1, 'the action belongs beside My readers')
  await page.getByRole('button', { name: 'Remove', exact: true }).click()
  await page.getByRole('dialog').getByRole('button', { name: 'Remove' }).click()
  await page.getByRole('status').getByText('Kindle removed.').waitFor()
  expect((await page.getByRole('button', { name: 'Add reader' }).count()) === 1, 'the empty-state action returns after removal')
  await page.getByRole('link', { name: 'Overview', exact: true }).click()
  await page.waitForURL(`${base}/profile`)
  await page.getByRole('region', { name: 'Your reading at a glance' }).waitFor()
  expect((await page.getByText('Kindle removed.').count()) === 0, 'reader feedback must stay on its section')
  await page.setViewportSize({ width: 390, height: 844 })
  expect((await page.evaluate(() => document.documentElement.scrollWidth - innerWidth)) <= 1, 'Profile overview must fit a narrow phone')

  await page.getByRole('button', { name: 'Account menu' }).click()
  expect((await page.getByRole('button', { name: 'Profile picture' }).count()) === 0,
    'adult picture settings belong on Profile')
  expect((await page.getByRole('link', { name: 'Profile', exact: true }).count()) === 1,
    'the adult account menu has one Profile entry')
  await page.getByRole('link', { name: 'Help' }).click()
  await page.waitForURL(`${base}/help`)
  await page.getByRole('heading', { name: 'Using Bokhylle' }).waitFor()
  expect((await page.evaluate(() => document.documentElement.scrollWidth - innerWidth)) <= 1, 'Help must fit a narrow phone')
}

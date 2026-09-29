import { expect } from '../support.mjs'

export default async function chunkRecovery(page, { base }) {
  let blocked = 0
  let failEveryChunk = false
  await page.route('**/assets/Profile-*.js', (route) => {
    if (failEveryChunk || blocked === 0) {
      blocked += 1
      return route.fulfill({ status: 404, contentType: 'text/plain', body: 'missing chunk' })
    }
    blocked += 1
    return route.continue()
  })
  await page.route('**/api/**', (route) => {
    const path = new URL(route.request().url()).pathname
    let body = []
    if (path === '/api/auth/me') {
      body = { user: {
        id: 4646, username: 'reader', displayName: 'Test Reader', role: 'user',
        profileType: 'adult', preferredFormat: 'epub', preferredLanguage: 'en',
        preferredLanguages: ['en'], defaultLanguage: 'en', acquisitionMode: 'automatic',
        notificationEmail: null, emailNotifications: false,
      } }
    } else if (path === '/api/profile/stats') {
      body = { shelf: 0, authors: 0, liked: 0, booksSent: 0 }
    } else if (path === '/api/profile/liked') {
      body = { items: [] }
    } else if (path.endsWith('/tokens')) {
      body = { tokens: [] }
    } else if (path === '/api/profile/hidden-subjects') {
      body = { hidden: [] }
    } else if (path === '/api/delivery-targets/default') {
      body = { address: null, source: null, senderAddress: null, amazonUrl: '' }
    }
    return route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(body) })
  })

  await page.goto(`${base}/profile`, { waitUntil: 'domcontentloaded' })
  await page.getByRole('heading', { name: 'My profile' }).waitFor({ timeout: 10000 })
  expect(blocked >= 2, 'one missing route chunk should trigger a fresh page load')

  failEveryChunk = true
  await page.reload({ waitUntil: 'domcontentloaded' })
  await page.getByRole('heading', { name: 'This page needs a refresh' }).waitFor({ timeout: 10000 })
  expect((await page.getByText('Unexpected Application Error!').count()) === 0, 'the router developer screen must stay hidden')
  expect((await page.getByRole('button', { name: 'Reload' }).count()) === 1, 'a failed refresh needs a recovery action')
}

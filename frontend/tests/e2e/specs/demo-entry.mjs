import { expect } from '../support.mjs'
import { check } from './accessibility.mjs'

export default async function demoEntry(page, { base }) {
  let user = null
  let entryStatus = 200
  let releaseEntry
  const entries = []
  const householdRequests = []
  await page.route('**/api/**', async (route) => {
    const request = route.request()
    const path = new URL(request.url()).pathname
    if (path === '/api/auth/users' || path === '/api/auth/login') householdRequests.push(path)
    if (path === '/api/auth/me') return route.fulfill(user
      ? { json: { user } }
      : { status: 401, json: { code: 'unauthorized', message: 'Authentication required' } })
    if (path === '/api/demo') return route.fulfill({ json: { enabled: true } })
    if (path === '/api/demo/enter') {
      const entry = { method: request.method(), ...request.postDataJSON() }
      entries.push(entry)
      await new Promise((resolve) => { releaseEntry = resolve })
      if (entryStatus !== 200) return route.fulfill({ status: entryStatus, json: { code: 'rate_limited', message: 'Please wait before entering the demo again.' } })
      user = {
        id: entry.profile === 'adult' ? 101 : 102, username: `demo-${entry.profile}`,
        displayName: entry.profile === 'adult' ? 'Adult reader' : 'Child reader',
        role: 'user', profileType: entry.profile, preferredLanguages: ['en'],
      }
      return route.fulfill({ json: { user } })
    }
    if (path === '/api/profile/onboarding') return route.fulfill({ json: { onboarded: true, interests: [] } })
    if (path === '/api/notifications') return route.fulfill({ json: { items: [], unread: 0, pendingRequests: 0, pendingRequestItems: [] } })
    if (path === '/api/books') return route.fulfill({ json: { items: [], total: 0, page: 1, pageSize: 24, letters: [] } })
    if (path === '/api/home/spotlight') return route.fulfill({ json: { items: [], recommendations: [] } })
    if (path === '/api/home/updates') return route.fulfill({ json: { library: [], discoveries: [], ready: [] } })
    return route.fulfill({ json: [] })
  })

  await page.goto(`${base}/login`, { waitUntil: 'networkidle' })
  for (const theme of ['paper', 'ink']) {
    // Apply the saved palette on page load, before color transitions begin.
    await page.evaluate((value) => localStorage.setItem('bokhylle.theme', value), theme)
    for (const width of [1440, 390, 320]) {
      user = null
      const profile = width === 390 ? 'child' : 'adult'
      await page.setViewportSize({ width, height: width === 1440 ? 1000 : 844 })
      // A first-time visitor reaches the picker from the demo's root URL.
      await page.goto(base, { waitUntil: 'networkidle' })
      await page.getByRole('heading', { name: 'Who’s reading?', exact: true }).waitFor()
      expect(new URL(page.url()).pathname === '/login', 'new demo visitors start at the household profile picker')
      expect(await page.evaluate(() => document.documentElement.dataset.theme) === theme, 'demo entry respects the saved palette')
      const profiles = page.getByRole('group', { name: 'Household profiles', exact: true })
      const adult = profiles.getByRole('button', { name: 'Adult reader', exact: true })
      const child = profiles.getByRole('button', { name: 'Child reader', exact: true })
      expect(await profiles.getByRole('button').count() === 2, 'demo shows the two clearly labelled reader profiles')
      const adultBox = await adult.boundingBox()
      const childBox = await child.boundingBox()
      expect(Math.abs(adultBox.y - childBox.y) < 2, 'both demo profiles fit on one row')
      expect(await page.locator('input').count() === 0, 'demo entry needs no username, PIN, or password')
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), 'demo entry fits the viewport')
      await check(page, `${theme} ${width} demo profile picker`)
      const before = entries.length
      const selected = profile === 'adult' ? adult : child
      await selected.focus()
      await page.keyboard.press('Enter')
      await page.getByRole('status').filter({ hasText: 'Opening your shelf…' }).waitFor()
      expect(await adult.isDisabled() && await child.isDisabled(), 'both profile choices are disabled while a visitor is created')
      await selected.evaluate((button) => button.click())
      expect(entries.length === before + 1, 'repeated selection must not create another visitor pair')
      expect(entries.at(-1).method === 'POST' && entries.at(-1).profile === profile, 'demo entry uses the selected profile through the isolated demo endpoint')
      releaseEntry()
      await page.waitForURL(`${base}/`)
      await page.getByRole('link', { name: 'Home', exact: true }).first().waitFor()
      await page.reload({ waitUntil: 'networkidle' })
      expect(new URL(page.url()).pathname === '/', 'returning demo visitors keep their current reader session')
    }
  }

  user = null
  entryStatus = 429
  await page.goto(`${base}/login`, { waitUntil: 'networkidle' })
  const adult = page.getByRole('button', { name: 'Adult reader', exact: true })
  const child = page.getByRole('button', { name: 'Child reader', exact: true })
  await adult.click()
  await page.getByRole('status').filter({ hasText: 'Opening your shelf…' }).waitFor()
  releaseEntry()
  await page.getByRole('alert').filter({ hasText: 'Please wait before entering the demo again.' }).waitFor()
  expect(await adult.isEnabled() && await child.isEnabled(), 'failed entry restores both profile choices for retry')
  await check(page, 'demo entry error')
  entryStatus = 200
  await child.click()
  await page.getByRole('status').filter({ hasText: 'Opening your shelf…' }).waitFor()
  releaseEntry()
  await page.waitForURL(`${base}/`)
  expect(user.profileType === 'child', 'retry can choose a different reader profile')
  await page.getByText('Your shelf is empty', { exact: true }).waitFor()
  await check(page, 'demo child Home after entry')
  expect(householdRequests.length === 0, 'demo entry never lists household accounts or uses household credentials')
}

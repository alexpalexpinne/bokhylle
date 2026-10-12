import { expect } from '../support.mjs'

const books = Array.from({ length: 3 }, (_, index) => ({
  id: 8700 + index, title: `An Invented Adventure ${index + 1}`, authors: ['Fictional Writer'],
  language: 'en', hasCover: false,
}))

export default async function homeLoading(page, { base }) {
  let releaseSlow
  let releaseRails
  const slow = new Promise((resolve) => { releaseSlow = resolve })
  const rails = new Promise((resolve) => { releaseRails = resolve })
  let holdRails = false
  let profile = 8787
  let homeRequests = 0
  await page.route('**/api/**', async (route) => {
    const path = new URL(route.request().url()).pathname
    if (path.startsWith('/api/home/')) homeRequests += 1
    let body = []
    if (path === '/api/auth/me') body = { user: {
      id: profile, username: `loading-reader-${profile}`, role: 'user', profileType: 'adult',
      preferredLanguages: ['en'], spotlightRotation: false, canDiscover: true,
    } }
    else if (path === '/api/demo') body = { enabled: false }
    else if (path === '/api/profile/onboarding') body = { onboarded: true, interests: ['adventure'] }
    else if (path === '/api/notifications') body = { items: [], unread: 0, pendingRequests: 0, pendingRequestItems: [] }
    else if (path === '/api/books') body = { items: books, total: books.length, page: 1, pageSize: 24, letters: [] }
    else if (path === '/api/books/facets') body = { formats: [], languages: [], subjects: [], series: [], publicationKinds: [] }
    else if (path === '/api/home/spotlight') body = { items: [], recommendations: [] }
    else if (path === '/api/home/rails') {
      if (holdRails) await rails
      body = profile === 8787 ? [{ key: 'adventure', title: 'Adventure', subject: 'adventure', books }] : []
    } else if (path === '/api/home/updates') {
      await slow
      body = { library: [], ready: [], discoveries: [{
        title: 'A Later Discovery', authors: ['Fictional Writer'], provider: 'fake', providerKey: 'later-discovery', year: 2026,
      }] }
    } else if (path === '/api/authors' || path === '/api/collections') await slow
    await route.fulfill({ json: body })
  })
  try {
    await page.goto(base, { waitUntil: 'domcontentloaded' })
    const rail = page.getByRole('region', { name: 'Adventure books', exact: true })
    await rail.waitFor({ timeout: 3000 })
    expect(await rail.locator('.shelf-book').count() === 3, 'ready local rails appear while earlier updates and later collections are still pending')

    // Leave before the full Home has loaded, then return beyond the former
    // one-minute expiry with the new rail request deliberately held open.
    await page.clock.setFixedTime(new Date(Date.now() + 70_000))
    await page.getByRole('link', { name: 'Library', exact: true }).click()
    await page.waitForURL(`${base}/library`)
    holdRails = true
    await page.getByRole('link', { name: 'Home', exact: true }).click()
    await rail.waitFor({ timeout: 2000 })
    expect(await rail.locator('.shelf-book').count() === 3, 'a partial Home snapshot restores rails immediately after a longer return visit')

    await page.evaluate(() => {
      history.replaceState({ ...history.state, usr: { fromOnboarding: true } }, '', location.href)
    })
    await page.reload({ waitUntil: 'domcontentloaded' })
    await rail.waitFor({ timeout: 2000 })
    expect(await rail.locator('.shelf-book').count() === 3, 'a partial snapshot survives a reload even on the Home history entry created by onboarding')

    releaseSlow()
    await page.getByRole('region', { name: 'Books from authors you follow', exact: true }).waitFor({ timeout: 3000 })
    expect(await rail.locator('.shelf-book').count() === 3, 'missing sections can fill without replacing the restored rail')

    releaseRails()
    await page.waitForLoadState('networkidle')
    const beforeReturn = homeRequests
    await page.getByRole('link', { name: 'Library', exact: true }).click()
    await page.waitForURL(`${base}/library`)
    await page.getByRole('link', { name: 'Home', exact: true }).click()
    await rail.waitFor({ timeout: 2000 })
    await page.waitForLoadState('networkidle')
    expect(homeRequests === beforeReturn, 'a rapid return to a complete Home reuses the saved visit without repeating Home requests')

    profile = 8788
    await page.reload({ waitUntil: 'networkidle' })
    expect(await rail.count() === 0, 'a different profile never sees another profile’s saved rails')
  } finally {
    releaseSlow()
    releaseRails()
  }
}

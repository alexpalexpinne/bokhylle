import { expect } from '../support.mjs'

export default async function recommendations(page, { base }) {
  const suggestions = Array.from({ length: 36 }, (_, i) => ({
    source: 'discover', ownership: 'discover', reasonType: 'taste', reasonLabel: 'Matches your interests: Fantasy',
    title: `Fictional Suggestion ${i + 1}`, authors: [`Invented Writer ${i + 1}`], subjects: [i % 2 ? 'Fantasy fiction' : 'Space opera'],
    provider: 'fake', providerKey: `suggestion-${i}`, recommendationKey: (i + 1).toString(16).padStart(64, '0'), cta: 'discover',
  }))
  let rejected = []
  let catalogueRequests = 0
  const impressions = new Set()
  let feedback
  await page.addInitScript(() => {
    globalThis.__suggestionTabHidden = true
    Object.defineProperty(document, 'hidden', { configurable: true, get: () => globalThis.__suggestionTabHidden })
  })
  await page.route('**/api/**', async (route) => {
    const url = new URL(route.request().url())
    const path = url.pathname
    let body = []
    if (path === '/api/auth/me') body = { user: { id: 8484, username: 'fictional-reader', role: 'user', profileType: 'adult', preferredLanguages: ['en'], spotlightRotation: false } }
    else if (path === '/api/demo') body = { enabled: false }
    else if (path === '/api/profile/onboarding') body = { onboarded: true, interests: ['fantasy'] }
    else if (path === '/api/notifications') body = { items: [], unread: 0, pendingRequests: 0, pendingRequestItems: [] }
    else if (path === '/api/recommendations') {
      catalogueRequests += 1
      expect(url.searchParams.get('limit') === '72', 'a bounded selection is loaded once per visit')
      const items = suggestions.filter((s) => !rejected.some((b) => b.title === s.title)).filter((s) => !url.searchParams.get('subject') || s.subjects.includes('Fantasy fiction'))
      body = { items, subjects: ['fantasy', 'space opera'], total: items.length, nextOffset: null }
    } else if (path === '/api/recommendations/impressions') {
      for (const key of JSON.parse(route.request().postData()).keys) impressions.add(key)
      await route.fulfill({ status: 204 }); return
    } else if (path.includes('/feedback/') && route.request().method() === 'DELETE') {
      rejected = []
      await route.fulfill({ status: 204 }); return
    } else if (path.endsWith('/feedback')) {
      feedback = JSON.parse(route.request().postData()).action
      const item = suggestions.find((s) => path.includes(s.recommendationKey))
      rejected = [{ bookId: 8485, title: item.title, authors: item.authors, readable: false, onShelf: false }]
      await route.fulfill({ json: { undoToken: 'fictional-undo-token' } }); return
    } else if (path === '/api/profile/rejected') body = { items: rejected }
    else if (path.startsWith('/api/profile/rejected/')) { rejected = []; await route.fulfill({ status: 204 }); return }
    else if (path === '/api/profile/liked') body = { items: [] }
    else if (path === '/api/home/series') body = [{ seriesId: 1, seriesName: 'Fictional Cycle', book: { id: 8486, title: 'Fictional Volume Two', authors: ['Invented Writer'], hasCover: false }, readable: true, missingVolume: null }, { seriesId: 2, seriesName: 'Another Fictional Cycle', book: null, readable: false, missingVolume: 3 }]
    else if (path === '/api/home/spotlight') body = { items: [], recommendations: [] }
    else if (path === '/api/home/updates') body = { library: [], ready: [], discoveries: [] }
    else if (path === '/api/books') body = { items: [], total: 0, page: 1, pageSize: 24, letters: [] }
    else if (path === '/api/home/subjects') body = { hidden: [] }
    else if (path === '/api/profile/stats') body = { shelf: 0, authors: 0, liked: 0, booksSent: 0 }
    else if (path.endsWith('/tokens')) body = { tokens: [] }
    else if (path === '/api/delivery-targets/default') body = { address: null, source: null, senderAddress: null, amazonUrl: '' }
    await route.fulfill({ json: body })
  })
  await page.goto(`${base}/recommendations`, { waitUntil: 'networkidle' })
  await page.getByRole('heading', { name: 'Picked for you', exact: true }).waitFor()
  expect(await page.locator('.shelf-grid-cell').count() === 24, 'the first group contains twenty-four books')
  await page.waitForTimeout(1300)
  expect(impressions.size === 0, 'a hidden tab does not establish impressions')
  await page.evaluate(() => { globalThis.__suggestionTabHidden = false; document.dispatchEvent(new Event('visibilitychange')) })
  await page.waitForTimeout(1300)
  expect(impressions.size > 0 && impressions.size < 24, 'only covers visible in the viewport establish impressions')
  const calls = catalogueRequests
  await page.getByRole('button', { name: 'Show more suggestions', exact: true }).click()
  expect(await page.locator('.shelf-grid-cell').count() === 36, 'show more reveals the same saved selection')
  expect(catalogueRequests === calls, 'impressions cannot reorder the next group')
  await page.getByRole('button', { name: 'Change suggestions for Fictional Suggestion 1', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: 'Your suggestion: Fictional Suggestion 1', exact: true })
  await dialog.getByRole('button', { name: 'Not for me', exact: true }).click()
  await dialog.waitFor({ state: 'detached' })
  await page.getByRole('heading', { name: 'Fictional Suggestion 1', exact: true }).waitFor({ state: 'detached' })
  expect(feedback === 'not_for_me', 'the suggestion menu writes explicit negative preference')
  expect(await page.locator('.shelf-grid-cell').count() === 35, 'feedback preserves an expanded selection while removing the rejected book')
  const confirmation = page.getByRole('complementary', { name: 'Change confirmation' })
  await confirmation.getByRole('button', { name: 'Undo', exact: true }).click()
  await page.getByRole('heading', { name: 'Fictional Suggestion 1', exact: true }).waitFor()
  expect(await page.locator('.shelf-grid-cell').count() === 36, 'nearby Undo restores the suggestion without collapsing the expanded list')
  await page.getByRole('button', { name: 'Change suggestions for Fictional Suggestion 1', exact: true }).click()
  await page.getByRole('dialog', { name: 'Your suggestion: Fictional Suggestion 1', exact: true }).getByRole('button', { name: 'Not for me', exact: true }).click()
  await page.goto(`${base}/profile/taste`, { waitUntil: 'networkidle' })
  await page.getByRole('heading', { name: 'Not for me', exact: true }).waitFor()
  await page.getByRole('button', { name: 'Restore', exact: true }).click()
  await page.getByRole('heading', { name: 'Not for me', exact: true }).waitFor({ state: 'detached' })
  await page.goto(`${base}/recommendations?subject=fantasy`, { waitUntil: 'networkidle' })
  expect(await page.locator('.shelf-grid-cell').count() === 18, 'the reading-interest filter keeps canonical fantasy matches')
  expect(await page.getByLabel('Reading interest').inputValue() === 'fantasy', 'the filter is reflected in the URL and control')
  await page.goto(base, { waitUntil: 'networkidle' })
  const series = page.getByRole('region', { name: 'Continue a series books', exact: true })
  await series.waitFor()
  expect(await series.getByRole('link', { name: /Fictional Volume Two/ }).getAttribute('href') === '/library/8486', 'the series rail opens the suggested successor')
  expect(await series.getByRole('link', { name: /Volume 3/ }).getAttribute('href') === '/discover?q=Another%20Fictional%20Cycle%203&type=any', 'a known gap offers a catalogue search')
}

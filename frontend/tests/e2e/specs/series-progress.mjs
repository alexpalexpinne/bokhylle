function expect(condition, message) {
  if (!condition) throw new Error(message)
}

const volumes = [1, 2].map((number) => ({
  id: 100 + number,
  title: `The Paper Moon · Volume ${number}`,
  authors: ['Iris North'],
  language: 'en',
  series: 'The Paper Moon',
  seriesId: 42,
  seriesNumber: String(number),
  seriesSortOrder: number,
  publicationKind: 'manga',
  hasCover: false,
  hasDescription: false,
  rating: null,
  ratingCount: null,
  ratingSource: null,
  addedAt: 1,
}))

export default async function seriesProgress(page, { base }) {
  let finished = false
  await page.route('**/api/**', (route) => {
    const path = new URL(route.request().url()).pathname
    let body = []
    if (path === '/api/auth/me') body = { user: {
      id: 1, username: 'iris', displayName: 'Iris', role: 'user', profileType: 'adult',
      preferredFormat: 'epub', preferredLanguage: 'en', preferredLanguages: ['en'],
      defaultLanguage: 'en', acquisitionMode: 'automatic', canAcquire: true,
    } }
    else if (path === '/api/profile/onboarding') body = { onboarded: true, interests: [] }
    else if (path === '/api/series/42') body = {
      id: 42, name: 'The Paper Moon', sortName: null, defaultReadingDirection: 'rtl',
      volumes,
      reading: {
        finishedBookIds: finished ? [101] : [],
        current: finished ? null : { bookId: 101, browserFileId: null, percentage: 0.3 },
        nextBookId: finished ? 102 : null,
        missingNextVolume: null,
      },
    }
    else if (path === '/api/books/101/completion' && route.request().method() === 'PUT') {
      finished = JSON.parse(route.request().postData() ?? '{}').completed === true
      body = { completed: finished, completedAt: finished ? 1 : null }
    }
    return route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(body) })
  })

  await page.goto(`${base}/series/42`, { waitUntil: 'networkidle' })
  await page.getByRole('heading', { name: 'The Paper Moon', exact: true }).waitFor()
  expect(await page.getByText('Continue reading').count() > 0, 'current volume appears before completion')
  expect(await page.getByText('Next in series').count() === 0, 'opening a volume does not suggest its successor')

  await page.getByRole('button', { name: 'Mark finished: The Paper Moon · Volume 1' }).click()
  await page.getByText('Next in series').waitFor()
  expect(await page.getByText('Your progress · 1 of 2 finished').isVisible(), 'series shows profile completion')
  expect(await page.getByRole('link', { name: 'Open volume' }).getAttribute('href') === '/library/102', 'next volume opens its own book detail')

  await page.getByRole('button', { name: 'Mark unfinished: The Paper Moon · Volume 1' }).click()
  await page.getByText('Your progress · 0 of 2 finished').waitFor()
  expect(await page.getByText('Next in series').count() === 0, 'removing completion removes the suggestion')

  await page.setViewportSize({ width: 390, height: 844 })
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth)
  expect(!overflow, 'series progress controls must fit a phone viewport')
}

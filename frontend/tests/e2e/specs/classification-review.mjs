import { expect } from '../support.mjs'

const user = {
  id: 1, username: 'admin', displayName: 'Admin', role: 'admin',
  profileType: 'adult', preferredFormat: 'epub', preferredLanguage: 'en',
  preferredLanguages: ['en'], defaultLanguage: 'en', acquisitionMode: 'automatic',
  notificationEmail: null, emailNotifications: false, canAcquire: true,
}

const items = [
  ...Array.from({ length: 300 }, (_, index) => ({
    id: index + 1, title: `Fictional Novel ${index + 1}`, fileName: `novel-${index + 1}.epub`, format: 'epub',
    legacySeriesText: null, seriesId: null, seriesName: null, seriesNumber: null,
    seriesSortOrder: null, publicationKind: 'unknown', readingDirection: null, reviewedAt: null,
    suggestion: { publicationKind: 'book', seriesName: null, seriesNumber: null, needsReview: false, reason: 'No series clue found; review the publication type' },
  })),
  ...Array.from({ length: 100 }, (_, index) => ({
    id: index + 301, title: `Fictional Comic Vol. ${index + 1}`, fileName: `comic-${index + 1}.cbz`, format: 'cbz',
    legacySeriesText: null, seriesId: null, seriesName: null, seriesNumber: null,
    seriesSortOrder: null, publicationKind: 'unknown', readingDirection: null, reviewedAt: null,
    suggestion: { publicationKind: 'comic', seriesName: 'Fictional Comic', seriesNumber: String(index + 1), needsReview: true, reason: 'Volume marker in title; verify the series grouping' },
  })),
]

export default async function classificationReview(page, { base }) {
  const submissions = []
  await page.route('**/api/**', (route) => {
    const url = new URL(route.request().url())
    const path = url.pathname
    let result = []
    if (path === '/api/auth/me') result = { user }
    else if (path === '/api/profile/onboarding') result = { onboarded: true, interests: [] }
    else if (path === '/api/notifications') result = { items: [], unread: 0, pendingRequests: 0, pendingRequestItems: [] }
    else if (path === '/api/admin/series') result = []
    else if (path === '/api/admin/books/classification-review' && route.request().method() === 'POST') {
      const decisions = JSON.parse(route.request().postData()).decisions
      submissions.push(decisions)
      for (const choice of decisions) {
        const item = items.find((entry) => entry.id === choice.bookId)
        item.reviewedAt = 1
        if (choice.action === 'apply') item.publicationKind = choice.publicationKind
      }
      result = { updated: decisions.length }
    } else if (path === '/api/admin/books/classification-review') {
      const status = url.searchParams.get('status') ?? 'pending'
      const kind = url.searchParams.get('kind') ?? 'all'
      const attention = url.searchParams.get('attention') ?? 'all'
      const pageNumber = Number(url.searchParams.get('page') ?? '1')
      const pageSize = Number(url.searchParams.get('pageSize') ?? '25')
      const scope = items.filter((item) => status === 'all' || item.reviewedAt === null)
      const matching = scope.filter((item) => (kind === 'all' || item.suggestion.publicationKind === kind)
        && (attention === 'all' || (attention === 'review') === item.suggestion.needsReview))
      result = {
        items: matching.slice((pageNumber - 1) * pageSize, pageNumber * pageSize),
        total: matching.length, page: pageNumber, pageSize,
        counts: {
          total: scope.length,
          book: scope.filter((item) => item.suggestion.publicationKind === 'book').length,
          comic: scope.filter((item) => item.suggestion.publicationKind === 'comic').length,
          manga: 0,
          simple: scope.filter((item) => !item.suggestion.needsReview).length,
          needsReview: scope.filter((item) => item.suggestion.needsReview).length,
        },
      }
    }
    return route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(result) })
  })

  await page.goto(`${base}/library/review`, { waitUntil: 'networkidle' })
  await page.getByRole('heading', { name: 'Review imports' }).waitFor()
  await page.getByRole('button', { name: /Books 300/ }).click()
  await page.getByLabel('Review priority').selectOption('simple')
  await page.getByText('300 matching awaiting review').waitFor()
  await page.getByRole('checkbox', { name: 'Select this page' }).check()
  await page.getByRole('button', { name: 'Next' }).click()
  await page.getByText('25 selected across this filter').waitFor()
  await page.getByRole('button', { name: 'Select all 300 matching' }).click()
  await page.getByText('300 selected across this filter').waitFor()
  await page.getByRole('button', { name: 'Preview acceptance' }).click()
  const dialog = page.getByRole('dialog', { name: 'Accept 300 files?' })
  await dialog.waitFor()
  expect(submissions.length === 0, 'preview must not save anything')
  expect(await dialog.getByText('300 book').isVisible(), 'preview must summarize all 300 decisions')
  await dialog.getByRole('button', { name: 'Cancel' }).click()
  expect(submissions.length === 0, 'cancelling preview must not save anything')
  await page.getByRole('button', { name: 'Preview acceptance' }).click()
  await page.getByRole('dialog', { name: 'Accept 300 files?' }).getByRole('button', { name: 'Confirm 300' }).click()
  await page.getByText('No files match these filters.').waitFor()
  expect(submissions.length === 1 && submissions[0].length === 300, 'one transaction must contain all selected files')
  expect(submissions[0].every((choice) => choice.publicationKind === 'book' && choice.onlyIfPending), 'bulk decisions must apply the book suggestion only to pending files')
  await page.getByRole('button', { name: /All 100/ }).click()
  await page.waitForURL((url) => !url.searchParams.has('kind'))
  await page.getByLabel('Review priority').selectOption('review')
  await page.waitForURL((url) => !url.searchParams.has('kind') && url.searchParams.get('attention') === 'review')
  await page.getByText('100 matching awaiting review').waitFor()
  expect(submissions.length === 1, 'comic clues should remain queued for individual review')

  // The selected-files toolbar must stay above the fixed mobile navigation,
  // including when revisiting an earlier decision from All files.
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto(`${base}/library/review?status=all&kind=book&attention=simple`)
  await page.getByText('300 matching library files').waitFor()
  await page.getByRole('button', { name: 'Select all 300 matching' }).click()
  await page.getByText('300 selected across this filter').waitFor()
  await page.getByRole('button', { name: 'Preview acceptance' }).click()
  await page.getByRole('dialog', { name: 'Accept 300 files?' }).waitFor()
  await page.getByRole('button', { name: 'Cancel' }).click()
  expect(submissions.length === 1, 'mobile preview must not save without confirmation')
}

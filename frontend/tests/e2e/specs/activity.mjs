import { expect } from '../support.mjs'

const row = (overrides) => ({
  id: '01a0b533-1111-7000-9000-00000000a1b2',
  bookId: 1,
  bookTitle: 'Mock Book',
  bookAuthors: ['Mock Author'],
  status: 'DOWNLOADING',
  preferredFormat: null,
  preferredLanguage: null,
  selectedReleaseName: 'Mock.Release.EPUB',
  selectedReleaseIndexer: 'mock',
  selectedReleaseScore: 90,
  selectedReleaseConfidence: 0.9,
  selectedReleaseSize: 4200000,
  selectedReleaseFormat: 'epub',
  selectedReleaseSeeders: 12,
  deliverOnReady: false,
  deliveryStatus: 'NONE',
  requestedByUserId: null,
  askBeforeDownload: false,
  downloadSpeed: 4200000,
  requestedBy: 'Mock User',
  downloadProvider: 'qbittorrent',
  errorCode: null,
  errorMessage: null,
  progress: 0,
  createdAt: 1,
  updatedAt: 2,
  ...overrides,
})

/** Activity tells the truth: percent, stage names and delivery state. */
export default async function activity(page, { base }) {
  await page.route('**/api/acquisitions**', (route) => {
    if (route.request().method() !== 'GET') {
      return route.continue()
    }
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify([
        row({ status: 'DOWNLOADING', progress: 64 }),
        row({
          id: '01a0b534-2222-7000-9000-00000000c3d4',
          bookTitle: 'Missing Book',
          status: 'NO_RELEASE_FOUND',
          progress: 0,
        }),
        row({
          id: '01a0b535-3333-7000-9000-00000000e5f6',
          bookTitle: 'Ready Book',
          status: 'READY',
          progress: 100,
          deliverOnReady: true,
          deliveryStatus: 'SENT',
        }),
      ]),
    })
  })

  await page.route('**/api/admin/attention', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        items: [
          {
            id: 'att-1',
            bookId: 9,
            title: 'Collection Book',
            authors: ['Attention Author'],
            kind: 'review',
            errorCode: null,
            errorMessage: null,
            updatedAt: 3,
          },
          {
            id: 'att-2',
            bookId: 10,
            title: 'Broken Book',
            authors: ['Attention Author'],
            kind: 'failed_import',
            errorCode: 'corrupt_archive',
            errorMessage: 'Archive could not be read',
            updatedAt: 2,
          },
        ],
        count: 2,
      }),
    }),
  )

  await page.goto(`${base}/activity`, { waitUntil: 'networkidle' })
  await page.waitForSelector('text=Mock Book', { timeout: 15000 })

  const text = await page.locator('main').innerText()
  expect(text.includes('64%'), `progress should be 64%, got: ${text.slice(0, 200)}`)
  expect(!text.includes('6400%'), 'progress must not be multiplied twice')

  // NO_RELEASE_FOUND is a FINDING outcome, not FOUND.
  expect(/No copy found|no copy found/.test(text), 'no-release should say no copy found')
  const findingRows = await page.getByText('FINDING', { exact: true }).count()
  expect(findingRows > 0, 'no-release should render the FINDING stage')

  // A sent delivery is knowable from the delivery record, not the intent.
  expect(text.includes('DELIVERED'), 'ready+sent should show the DELIVERED stage')
  expect(/delivered to your reader/i.test(text), 'sent delivery detail should render')

  // The admin inbox aggregates only decisions, with the right actions.
  expect(text.includes('Needs attention'), 'admins get the attention inbox')
  expect(
    text.includes('Collection Book') && text.includes('Broken Book'),
    'attention items render with titles',
  )
  expect(
    (await page.getByRole('button', { name: 'Review files' }).count()) > 0,
    'review items offer the file choice',
  )
  expect(
    (await page.getByRole('button', { name: 'Choose file' }).count()) > 0,
    'failed imports offer manual inspection',
  )
}

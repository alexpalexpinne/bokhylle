function expect(condition, message) {
  if (!condition) {
    throw new Error(message)
  }
}

const CHILD = {
  id: 2,
  username: 'kid',
  displayName: 'Kid',
  role: 'user',
  preferredFormat: null,
  preferredLanguage: null,
  acquisitionMode: 'automatic',
  notificationEmail: null,
  emailNotifications: false,
  profileType: 'child',
  defaultLanguage: 'en',
  preferredLanguages: ['en'],
  canRequest: true,
}

const HIT = {
  provider: 'openlibrary',
  providerKey: '/works/OLREQW',
  title: 'Requested Catalogue Book',
  authors: ['Catalogue Author'],
  year: 2020,
  language: 'en',
  series: null,
  seriesNumber: null,
  coverId: null,
}

// The request catalogue must never show a previous query's results, and a
// failed list load must not read as "no requests".
export default async function requests(page, { base }) {
  await page.route('**/api/auth/me', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ user: CHILD }),
    }),
  )
  await page.route('**/api/requests', (route) =>
    route.fulfill({ status: 200, contentType: 'application/json', body: '{"items":[]}' }),
  )
  let searchCount = 0
  await page.route('**/api/requests/search**', (route) => {
    searchCount += 1
    if (searchCount > 1) {
      return route.fulfill({
        status: 500,
        contentType: 'application/json',
        body: JSON.stringify({ code: 'internal_error', message: 'Search is down' }),
      })
    }
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ items: [HIT] }),
    })
  })

  await page.goto(`${base}/requests`, { waitUntil: 'networkidle' })
  const search = page.getByLabel('Search the request catalogue')
  await search.fill('requested catalogue')
  await page.getByText(HIT.title).waitFor({ timeout: 8000 })

  // A later failing query must clear the stale hits and surface the failure.
  await search.fill('different query')
  await page.getByText(/Search is down/).waitFor({ timeout: 8000 })
  expect(
    (await page.getByText(HIT.title).count()) === 0,
    'results from the previous query must not stay visible after a failed search',
  )

  // A failed request-list load is an error, not an empty state.
  await page.route('**/api/requests', (route) =>
    route.fulfill({
      status: 500,
      contentType: 'application/json',
      body: JSON.stringify({ code: 'internal_error', message: 'Requests are down' }),
    }),
  )
  await page.reload({ waitUntil: 'networkidle' })
  await page.getByText(/Requests are down/).waitFor({ timeout: 8000 })
  await page.getByRole('button', { name: 'Try again' }).waitFor({ timeout: 8000 })
  expect(
    (await page.getByText('You have not asked for anything yet.').count()) === 0,
    'a failed load must not masquerade as an empty request list',
  )

  // Adults approve from the notification menu; the tab stays child-only.
  const ADULT = { ...CHILD, id: 1, username: 'admin', displayName: 'Admin', role: 'admin', profileType: 'adult' }
  await page.route('**/api/auth/me', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ user: ADULT }),
    }),
  )
  let approved = false
  const pendingItem = {
    id: 1,
    bookId: 7,
    title: 'Requested Book',
    authors: ['Request Author'],
    requester: 'Kid',
    requesterUserId: 2,
    status: 'requested',
    phase: 'requested',
    acquisitionId: null,
    keepLooking: false,
    errorCode: null,
    createdAt: 1,
    updatedAt: 1,
  }
  await page.route('**/api/notifications', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        items: [],
        unread: 0,
        pendingRequests: approved ? 0 : 1,
        pendingRequestItems: approved ? [] : [pendingItem],
      }),
    }),
  )
  await page.route('**/api/requests/1/approve', (route) => {
    approved = true
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ request: { ...pendingItem, status: 'approved', phase: 'looking' } }),
    })
  })

  await page.goto(`${base}/library`, { waitUntil: 'networkidle' })
  expect(
    (await page.getByRole('link', { name: 'Requests' }).count()) === 0,
    'adults never get a Requests tab; approvals live in the bell',
  )
  await page.getByRole('button', { name: /Notifications/ }).click()
  await page.getByText('1 request awaiting you').waitFor({ timeout: 8000 })
  await page.getByText('Requested Book').first().waitFor({ timeout: 8000 })
  await page.getByRole('button', { name: 'Approve' }).click()
  await page
    .getByText('1 request awaiting you')
    .waitFor({ state: 'detached', timeout: 8000 })
  expect(
    (await page.getByRole('button', { name: 'Approve' }).count()) === 0,
    'an approved request leaves the pinned block',
  )
}

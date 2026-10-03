function expect(condition, message) {
  if (!condition) {
    throw new Error(message)
  }
}

const CHILD_BOOK = {
  id: 9101,
  title: 'Child Shelf Book',
  authors: ['Child Author'],
  language: 'en',
  series: null,
  seriesNumber: null,
  hasCover: false,
  hasDescription: true,
  rating: null,
  ratingCount: null,
  ratingSource: null,
  addedAt: 1,
}

const ADMIN = {
  id: 1,
  username: 'admin',
  displayName: 'Admin',
  role: 'admin',
  preferredFormat: null,
  preferredLanguage: null,
  acquisitionMode: 'automatic',
  notificationEmail: null,
  emailNotifications: false,
  profileType: 'adult',
  defaultLanguage: null,
  preferredLanguages: [],
}

const CHILD = {
  ...ADMIN,
  id: 2,
  username: 'child',
  displayName: 'Child',
  role: 'user',
  profileType: 'child',
}

const MEMBER = {
  ...ADMIN,
  id: 3,
  username: 'member',
  displayName: 'Member',
  role: 'user',
  profileType: 'adult',
}

const POCKETBOOK = {
  id: 7,
  userId: 1,
  type: 'pocketbook',
  name: 'PocketBook',
  address: 'sample@pbsync.com',
  connector: 'email',
  enabled: true,
  isDefault: true,
  createdAt: 1,
  updatedAt: 1,
}

export default async function family(page, { base }) {
  // FE-37: onboarding can be skipped and lands on Home.
  let onboardedCalls = 0
  await page.route('**/api/profile/onboarded', (route) => {
    onboardedCalls += 1
    return route.fulfill({ status: 200, contentType: 'application/json', body: '{}' })
  })
  // The POST above is mocked, so the server never records completion; the
  // onboarding state Home reads must reflect the finished journey.
  await page.route('**/api/profile/onboarding', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ onboarded: true, interests: [] }),
    }),
  )
  let scopedUser = null
  await page.route('**/api/books**', (route) => {
    const url = new URL(route.request().url())
    if (route.request().method() !== 'GET' || url.pathname !== '/api/books') {
      return route.continue()
    }
    const mine = url.searchParams.get('mine') === 'true'
    const user = url.searchParams.get('user')
    if (user) {
      scopedUser = user
    }
    const items = mine || user === '2' ? [CHILD_BOOK] : []
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ items, total: items.length, letters: [], page: 1, pageSize: 50 }),
    })
  })

  await page.goto(`${base}/welcome`, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: 'Skip' }).click()
  await page.waitForURL(`${base}/`)
  expect(onboardedCalls === 1, 'Skip must complete onboarding')

  // FE-37: onboarding can complete with one reader type and address.
  let created = null
  await page.route('**/api/delivery-targets', (route) => {
    if (route.request().method() !== 'POST') {
      return route.continue()
    }
    created = JSON.parse(route.request().postData() ?? '{}')
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(POCKETBOOK),
    })
  })
  let profileSaved = null
  const profileSaves = []
  await page.route('**/api/profile', (route) => {
    if (route.request().method() !== 'PUT') {
      return route.continue()
    }
    profileSaved = JSON.parse(route.request().postData() ?? '{}')
    profileSaves.push(profileSaved)
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ user: ADMIN }),
    })
  })

  await page.goto(`${base}/welcome`, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: 'Next' }).click()

  // Languages are seeded and saved in order.
  await page.getByText('What languages do you read in?').waitFor({ state: 'visible', timeout: 8000 })
  await page.getByLabel('Add another language').selectOption('no')
  await page.getByRole('button', { name: 'Next' }).click()
  expect(
    profileSaves.some(
      (save) => Array.isArray(save.preferredLanguages) && save.preferredLanguages.includes('no'),
    ),
    `the language step must save the ordered list: ${JSON.stringify(profileSaves)}`,
  )

  // FE-37: a failed Follow rolls back on the onboarding step too.
  await page.route('**/api/discover/authors**', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        local: [{ authorId: 77, name: 'Risky Author', following: false, bookCount: 2 }],
        external: [],
      }),
    }),
  )
  await page.route('**/api/authors/*/follow', (route) =>
    route.fulfill({
      status: 500,
      contentType: 'application/json',
      body: JSON.stringify({ error: 'mock failure' }),
    }),
  )
  await page.getByPlaceholder('Search authors…').fill('Risky')
  await page.waitForSelector('text=Risky Author', { timeout: 8000 })
  await page.getByRole('button', { name: /^Follow$/ }).first().click()
  await page.waitForSelector('text=Could not update that follow. Try again.', { timeout: 8000 })
  await page.waitForFunction(
    () => {
      const labels = Array.from(document.querySelectorAll('button')).map((button) =>
        button.textContent?.trim(),
      )
      return labels.includes('Follow') && !labels.includes('Following')
    },
    undefined,
    { timeout: 8000 },
  )

  await page.getByRole('button', { name: 'Next' }).click()

  // FE-37: step 2 offers provider books alongside household ones, and liking
  // an external result stores taste without owning it.
  await page.route('**/api/discover/search**', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify([
        {
          provider: 'openlibrary',
          providerKey: '/works/OLHAILW',
          title: 'Project Hail Mary',
          authors: ['Andy Weir'],
          year: 2021,
          language: 'en',
          isbn10: null,
          isbn13: null,
          series: null,
          seriesNumber: null,
          coverId: null,
          status: 'NOT_IN_LIBRARY',
          ownedBookId: null,
          ownedFileId: null,
          onShelf: false,
        },
      ]),
    }),
  )
  let likedExternal = null
  await page.route('**/api/discover/like', (route) => {
    likedExternal = JSON.parse(route.request().postData() ?? '{}')
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ bookId: 9001, preference: 'liked' }),
    })
  })
  await page.getByPlaceholder('Search books…').fill('hail mary')
  await page.waitForSelector('text=Other books', { timeout: 8000 })
  await page.getByText('Project Hail Mary').first().waitFor({ state: 'visible', timeout: 8000 })
  const [likeResponse] = await Promise.all([
    page.waitForResponse(
      (candidate) => candidate.url().includes('/api/discover/like'),
      { timeout: 8000 },
    ),
    page.getByRole('button', { name: /^Love it$/ }).last().click(),
  ])
  expect(likeResponse.status() < 400, 'liking an external book must succeed')
  expect(
    likedExternal?.providerKey === '/works/OLHAILW',
    `liking must post the provider key: ${JSON.stringify(likedExternal)}`,
  )
  await page.getByRole('button', { name: /^Liked$/ }).last().waitFor({ state: 'visible', timeout: 8000 })

  await page.getByRole('button', { name: 'Next' }).click()
  await page.getByRole('button', { name: 'PocketBook' }).click()
  await page.getByPlaceholder('name@pbsync.com').fill('sample@pbsync.com')
  await page.getByRole('button', { name: 'Next' }).click()
  await page.waitForSelector('text=How automatic should Bokhylle be?', { timeout: 8000 })
  let releaseRecent
  const recentGate = new Promise((resolve) => { releaseRecent = resolve })
  await page.route('**/api/books/recent**', async (route) => {
    await recentGate
    return route.continue()
  })
  await page.getByRole('button', { name: 'Finish' }).click()
  await page.waitForURL(`${base}/`)
  await page.getByText('Preparing your library', { exact: true }).waitFor({ state: 'visible', timeout: 8000 })
  expect(await page.locator('.animate-pulse').count() > 0,
    'the setup handoff reserves space while the library loads')
  releaseRecent()
  await page.getByText('Preparing your library', { exact: true }).waitFor({ state: 'hidden', timeout: 8000 })
  expect(
    created !== null && created.deviceType === 'pocketbook' && created.address === 'sample@pbsync.com',
    `finishing must save the PocketBook reader: ${JSON.stringify(created)}`,
  )
  expect(
    profileSaved !== null && profileSaved.acquisitionMode === 'automatic',
    `finishing must save the acquisition mode: ${JSON.stringify(profileSaved)}`,
  )
  expect(onboardedCalls === 2, 'finishing must complete onboarding')

  // FE-37: Profile saves an ordinary preference.
  await page.route('**/api/profile/liked', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        items: [
          {
            bookId: 9101,
            title: 'Liked Wizard Book',
            authors: ['Wizard Author'],
            readable: false,
            onShelf: false,
            provider: 'openlibrary',
            providerKey: '/works/OLLIKEDW',
          },
        ],
      }),
    }),
  )
  let unlikedBody = null
  await page.route('**/api/books/9101/preference', (route) => {
    unlikedBody = JSON.parse(route.request().postData() ?? '{}')
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ ok: true }),
    })
  })
  await page.goto(`${base}/profile/preferences`, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: /Save preferences/ }).click()
  await page.waitForSelector('text=Preferences saved.', { timeout: 8000 })
  expect(
    profileSaved !== null && 'emailNotifications' in profileSaved,
    `profile save must post preferences: ${JSON.stringify(profileSaved)}`,
  )

  // FE-37: liked books are listed and removable without touching the shelf.
  await page.goto(`${base}/profile/taste`, { waitUntil: 'networkidle' })
  await page.getByText('Books you like').waitFor({ state: 'visible', timeout: 8000 })
  await page.getByText('Liked Wizard Book').waitFor({ state: 'visible', timeout: 8000 })
  await page.getByRole('listitem').filter({ hasText: 'Liked Wizard Book' })
    .getByRole('button', { name: 'Remove' }).click()
  await page.waitForFunction(
    () => !document.body.innerText.includes('Liked Wizard Book'),
    undefined,
    { timeout: 8000 },
  )
  expect(
    unlikedBody !== null && unlikedBody.preference === null,
    `removing a like must clear the preference: ${JSON.stringify(unlikedBody)}`,
  )

  // FE-37: a failed Follow request rolls back and shows readable feedback.
  await page.route('**/api/authors**', (route) => {
    if (route.request().url().includes('/follow')) {
      return route.fallback()
    }
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify([
        { id: 5, name: 'Risky Author', bookCount: 2, following: false, autoAcquire: false },
      ]),
    })
  })
  await page.route('**/api/authors/*/follow', (route) =>
    route.fulfill({
      status: 500,
      contentType: 'application/json',
      body: JSON.stringify({ error: 'mock failure' }),
    }),
  )

  await page.goto(`${base}/library?mode=authors`, { waitUntil: 'networkidle' })
  await page.waitForSelector('text=Risky Author', { timeout: 15000 })
  await page.getByRole('button', { name: /^Follow$/ }).first().click()
  await page.waitForSelector('text=Could not update that follow. Try again.', { timeout: 8000 })
  const rowText = await page.locator('div[data-letter]').first().innerText()
  expect(
    /Follow\b/.test(rowText) && !/Following/.test(rowText),
    `a failed follow must roll back to Follow: ${rowText}`,
  )

  // FE-35: a child profile keeps Home and Library. Discover needs its own permission.
  let childCanDiscover = false
  let childCanRequest = true
  await page.route('**/api/auth/me', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ user: { ...CHILD, canDiscover: childCanDiscover, canRequest: childCanRequest } }),
    }),
  )

  await page.route('**/api/books/recent**', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify([CHILD_BOOK]),
    }),
  )
  await page.route('**/api/books/highlights**', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify([CHILD_BOOK]),
    }),
  )
  let childPreference = null
  let childLikeBody = null
  await page.route('**/api/books/9102**', (route) => {
    const url = new URL(route.request().url())
    if (url.pathname.endsWith('/related')) {
      return route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ series: [], author: [], similar: [] }),
      })
    }
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        ...CHILD_BOOK,
        id: 9102,
        title: 'Child Detail Book',
        description: 'A child detail book.',
        publicationYear: 2020,
        onShelf: true,
        preference: childPreference,
        subjects: [{ name: 'Fantasy', normalized: 'fantasy' }],
        editions: [],
        files: [{ id: 71, editionId: 1, format: 'epub', size: 420000, filename: 'child.epub' }],
      }),
    })
  })
  await page.route('**/api/books/*/preference', (route) => {
    childLikeBody = JSON.parse(route.request().postData() ?? '{}')
    childPreference = childLikeBody.preference
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ ok: true }),
    })
  })
  await page.reload({ waitUntil: 'networkidle' })
  const navLabels = (await page.locator('header nav a').allInnerTexts()).map((label) =>
    label.toUpperCase(),
  )
  expect(
    navLabels.length === 3 &&
      navLabels.includes('HOME') &&
      navLabels.includes('LIBRARY') &&
      navLabels.includes('REQUESTS'),
    `child navigation must be Home, Library and Requests: ${JSON.stringify(navLabels)}`,
  )
  expect(
    !navLabels.includes('DISCOVER') && !navLabels.includes('ACTIVITY'),
    `Discover and Activity must be absent for children: ${JSON.stringify(navLabels)}`,
  )
  await page.getByRole('button', { name: 'Account menu' }).click()
  expect(
    (await page.getByRole('link', { name: 'My settings' }).count()) === 1,
    'the account menu must offer child settings',
  )
  await page.getByRole('link', { name: 'My settings' }).click()
  await page.getByRole('heading', { name: 'My settings' }).waitFor()
  expect((await page.getByRole('button', { name: 'Change picture' }).count()) === 1,
    'a child must be able to change their profile picture')
  expect((await page.getByRole('link', { name: 'Send to readers' }).count()) === 0,
    'child settings must not expose adult reader management')
  await page.getByRole('group', { name: 'Theme' }).getByRole('button', { name: 'Ink' }).click()
  expect(await page.getByRole('group', { name: 'Theme' }).getByRole('button', { name: 'Ink' }).getAttribute('aria-pressed') === 'true',
    'a child must be able to choose the Ink theme')
  await page.getByLabel('Book text size: 100%').focus()
  await page.getByLabel('Book text size: 100%').press('End')
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('bokhylle.readerAppearance:2') ?? '{}').textScale) === 200,
    'child text size must be saved for the browser reader')

  await page.goto(`${base}/`, { waitUntil: 'networkidle' })
  await page
    .locator('header nav a', { hasText: 'Home' })
    .first()
    .waitFor({ state: 'visible', timeout: 8000 })
  await page.getByText('Child Shelf Book').first().waitFor({ state: 'visible', timeout: 8000 })
  await page.goto(`${base}/library`, { waitUntil: 'networkidle' })
  await page.getByLabel('Search your shelf').waitFor({ state: 'visible', timeout: 8000 })
  expect(
    (await page.getByRole('button', { name: /^Authors$/ }).count()) === 0,
    'children must not see the Authors tab in Library',
  )
  await page.goto(`${base}/library/9102`, { waitUntil: 'networkidle' })
  await page.waitForSelector('text=Child Detail Book', { timeout: 8000 })
  const detailText = await page.locator('main').innerText()
  expect(!/Send to my reader/.test(detailText), `child detail must not offer sending: ${detailText.slice(0, 240)}`)
  expect(!/Download/.test(detailText), 'child detail must not offer downloads')
  expect(!/Manage collections/.test(detailText), 'child detail must not offer collection management')
  expect(!/On my shelf|Add to my shelf/.test(detailText), 'child detail must not offer shelf mutations')
  expect(
    (await page.locator('a[href*="/download"]').count()) === 0,
    'child detail must not render download links',
  )
  expect(
    (await page.locator('a[href*="/discover"]').count()) === 0,
    'child detail must not link to Discover',
  )
  expect(!/Find more in Discover/.test(detailText), 'child detail must not mention Discover')
  expect(
    !/Similar in your library/.test(detailText),
    'child detail must not show the similar-books rail',
  )
  await page.getByRole('button', { name: 'Like', exact: true }).waitFor()
  await page.getByRole('button', { name: 'Like', exact: true }).click()
  await page.getByRole('button', { name: 'Liked', exact: true }).waitFor({ timeout: 8000 })
  expect(
    childLikeBody?.preference === 'liked',
    `a child like must set the preference: ${JSON.stringify(childLikeBody)}`,
  )

  // Hidden navigation is not route protection: children are redirected to their shelf.
  await page.goto(`${base}/discover`, { waitUntil: 'networkidle' })
  await page.waitForURL(`${base}/library`, { timeout: 8000 })
  await page.goto(`${base}/activity`, { waitUntil: 'networkidle' })
  await page.waitForURL(`${base}/library`, { timeout: 8000 })
  await page.goto(`${base}/authors/5`, { waitUntil: 'networkidle' })
  await page.waitForURL(`${base}/library`, { timeout: 8000 })
  await page.goto(`${base}/profile`, { waitUntil: 'networkidle' })
  await page.getByRole('heading', { name: 'My settings' }).waitFor({ timeout: 8000 })
  await page.goto(`${base}/profile/readers`, { waitUntil: 'networkidle' })
  await page.waitForURL(`${base}/profile`, { timeout: 8000 })

  // A child can ask for a book from the safe, metadata-only catalogue.
  let childRequest = null
  let askedBody = null
  await page.route('**/api/requests/search**', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        items: [
          {
            provider: 'openlibrary',
            providerKey: '/works/OLCHILDREQW',
            title: 'Child Request Book',
            authors: ['Child Author'],
            year: 2021,
            language: 'en',
            series: null,
            seriesNumber: null,
            coverId: null,
          },
        ],
      }),
    }),
  )
  await page.route('**/api/requests', (route) => {
    if (route.request().method() === 'POST') {
      askedBody = JSON.parse(route.request().postData() ?? '{}')
      childRequest = {
        id: 1,
        bookId: 9010,
        title: 'Child Request Book',
        authors: ['Child Author'],
        requester: 'Child',
        requesterUserId: 2,
        status: 'requested',
        phase: 'requested',
        acquisitionId: null,
        keepLooking: false,
        errorCode: null,
        createdAt: 1,
        updatedAt: 1,
      }
      return route.fulfill({
        status: 201,
        contentType: 'application/json',
        body: JSON.stringify({ request: childRequest, duplicate: false }),
      })
    }
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ items: childRequest ? [childRequest] : [] }),
    })
  })

  await page.goto(`${base}/requests`, { waitUntil: 'networkidle' })
  await page.getByText('Ask for a book').waitFor({ timeout: 8000 })
  await page.getByLabel('Search the request catalogue').fill('child request')
  await page.getByText('Child Request Book').first().waitFor({ timeout: 8000 })
  await page.getByRole('button', { name: 'Ask an adult' }).click()
  await page.getByText('Asked for “Child Request Book” — an administrator can approve it.').waitFor({ timeout: 8000 })
  await page.getByRole('button', { name: 'Requested' }).waitFor({ timeout: 8000 })
  expect(
    askedBody?.providerKey === '/works/OLCHILDREQW',
    `asking must post the provider key: ${JSON.stringify(askedBody)}`,
  )

  // Discover grants browsing, while the existing request permission still
  // decides whether the child may submit the title for adult approval.
  childCanDiscover = true
  await page.route('**/api/requests/book**', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        provider: 'openlibrary',
        providerKey: '/works/OLCHILDREQW',
        title: 'Child Request Book',
        authors: ['Child Author'],
        year: 2021,
        language: 'en',
        languages: ['en'],
        series: null,
        seriesNumber: null,
        description: 'A book the child wants to explore.',
        coverId: null,
      }),
    }),
  )
  await page.route('**/api/home/spotlight*', (route) =>
    route.fulfill({ status: 200, contentType: 'application/json', body: '{"items":[],"recommendations":[{"title":"Suggested Catalogue Book","authors":["Sample Author"],"provider":"openlibrary","providerKey":"/works/OLSUGGESTEDW","source":"discover"}]}' }),
  )
  await page.goto(`${base}/discover`, { waitUntil: 'networkidle' })
  await page.getByText('Search for a book or author to start exploring.').waitFor()
  expect((await page.getByText('Picked for you').count()) === 0,
    'child Discover must stay a search screen')
  await page.getByLabel('Search Discover').fill('child request')
  await page.getByRole('button', { name: /Child Request Book/ }).click()
  await page.getByRole('dialog', { name: 'Child Request Book' }).waitFor({ timeout: 8000 })
  expect((await page.getByRole('button', { name: /Download|Send to/i }).count()) === 0,
    'child Discover must not show direct download or delivery actions')
  await page.getByRole('dialog').getByRole('button', { name: 'Ask an adult' }).click()
  await page.getByRole('dialog').getByRole('button', { name: 'Requested' }).waitFor({ timeout: 8000 })
  await page.getByRole('dialog').getByText('Waiting for an adult to approve.').waitFor({ timeout: 8000 })
  await page.getByRole('dialog').getByRole('button', { name: 'Close' }).last().click()
  await page.getByRole('button', { name: /Child Request Book/ }).click()
  await page.getByRole('dialog').getByRole('button', { name: 'Requested' }).waitFor({ timeout: 8000 })
  childCanRequest = false
  await page.goto(`${base}/discover?provider=openlibrary&providerKey=%2Fworks%2FOLCHILDREQW`, { waitUntil: 'networkidle' })
  await page.getByRole('dialog', { name: 'Child Request Book' }).waitFor({ timeout: 8000 })
  expect((await page.getByRole('dialog').getByRole('button', { name: 'Ask an adult' }).count()) === 0,
    'Discover may be browsed without request permission')
  childCanDiscover = false
  childCanRequest = true

  // FE-35: the child wizard only covers the assigned shelf and interests.
  // Likes belong to a profile. The adult's earlier taste fixture must not
  // appear as an existing like when the child opens setup.
  await page.route('**/api/profile/liked', (route) =>
    route.fulfill({ status: 200, contentType: 'application/json', body: '{"items":[]}' }),
  )
  let wizardLike = null
  await page.route('**/api/books/*/preference', (route) => {
    wizardLike = JSON.parse(route.request().postData() ?? '{}')
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ ok: true }),
    })
  })
  await page.route('**/api/profile/interests', (route) =>
    route.fulfill({ status: 200, contentType: 'application/json', body: '{}' }),
  )
  await page.goto(`${base}/welcome`, { waitUntil: 'networkidle' })
  await page.getByText('Hi, Child').waitFor({ state: 'visible', timeout: 8000 })
  await page.getByRole('button', { name: 'Next' }).click()
  await page.getByText('Pick books you like').waitFor({ state: 'visible', timeout: 8000 })
  await page.getByText('Child Shelf Book').waitFor({ state: 'visible', timeout: 8000 })
  const wizardText = await page.locator('main').innerText()
  expect(!/household/i.test(wizardText), 'the child wizard must not mention the household library')
  expect(!/Search authors/i.test(wizardText), 'the child wizard must not search authors')
  await page.getByRole('button', { name: /^Love it$/ }).first().click()
  await page.waitForFunction(() => document.body.innerText.includes('Liked'), undefined, {
    timeout: 8000,
  })
  expect(
    wizardLike !== null && wizardLike.preference === 'liked',
    `the child wizard must save the like: ${JSON.stringify(wizardLike)}`,
  )
  await page.getByRole('button', { name: 'Next' }).click()
  await page.getByText('What do you like reading?').waitFor({ state: 'visible', timeout: 8000 })
  await page.getByRole('button', { name: 'Fantasy' }).click()
  await page.getByRole('button', { name: 'Finish' }).click()
  await page.waitForURL(`${base}/`)
  expect(onboardedCalls === 3, `the child wizard must complete onboarding: ${onboardedCalls}`)

  // FE-35: at phone width the two entries fill the bottom nav without horizontal overflow.
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto(`${base}/`, { waitUntil: 'networkidle' })
  await page.locator('nav.fixed a').first().waitFor({ state: 'visible', timeout: 8000 })
  const bottomNav = page.locator('nav.fixed')
  const links = bottomNav.locator('a')
  expect((await links.count()) === 3, 'the child bottom nav must show three entries')
  const navBox = await bottomNav.boundingBox()
  const firstBox = await links.nth(0).boundingBox()
  const secondBox = await links.nth(1).boundingBox()
  const thirdBox = await links.nth(2).boundingBox()
  expect(
    navBox !== null && firstBox !== null && secondBox !== null && thirdBox !== null,
    'the bottom nav must have layout boxes',
  )
  expect(
    Math.abs(firstBox.width - navBox.width / 3) <= 2 &&
      Math.abs(secondBox.width - navBox.width / 3) <= 2 &&
      Math.abs(thirdBox.width - navBox.width / 3) <= 2,
    `all entries must fill the nav: ${JSON.stringify({ navBox, firstBox, secondBox, thirdBox })}`,
  )
  const overflow = await page.evaluate(() => {
    const nav = document.querySelector('nav.fixed')
    return {
      document: document.documentElement.scrollWidth - window.innerWidth,
      nav: nav ? nav.scrollWidth - nav.clientWidth : 0,
    }
  })
  expect(
    overflow.document <= 1 && overflow.nav <= 1,
    `no horizontal overflow at 390x844: ${JSON.stringify(overflow)}`,
  )

  // A child whose profile bars requests sees no catalogue: only assigned books.
  await page.route('**/api/auth/me', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ user: { ...CHILD, canRequest: false } }),
    }),
  )
  await page.route('**/api/requests', (route) =>
    route.fulfill({ status: 200, contentType: 'application/json', body: '{"items":[]}' }),
  )
  await page.goto(`${base}/requests`, { waitUntil: 'networkidle' })
  await page.getByText('Books an administrator approves for your reader will appear here.').waitFor({ timeout: 8000 })
  expect(
    (await page.getByLabel('Search the request catalogue').count()) === 0,
    'a child without request access must not see the catalogue search',
  )
  expect(
    (await page.getByText('An administrator can approve books for your reader.').count()) > 0,
    'the empty state must explain adult approval',
  )

  // M6: admins see actionable library-health counts, and each count links
  // into the matching Library view. (The child mock above still owns
  // `/api/auth/me`, so an admin mock has to be registered on top.)
  await page.route('**/api/auth/me', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ user: ADMIN }),
    }),
  )
  await page.route('**/api/admin/library-health', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        books: 12,
        files: 14,
        missingCovers: 3,
        missingDescriptions: 2,
        missingLanguages: 1,
        missingFiles: 1,
        missingFileSamples: [
          { bookId: 9001, title: 'Shelf Only Book', path: '/data/books/gone.epub' },
        ],
      }),
    }),
  )
  await page.goto(`${base}/settings/library`, { waitUntil: 'networkidle' })
  await page.getByText('Library health').first().waitFor({ timeout: 8000 })
  const settingsText = await page.locator('main').innerText()
  expect(
    /Missing covers/.test(settingsText) && /Missing descriptions/.test(settingsText),
    `the health panel must list the metadata gaps: ${settingsText.slice(0, 240)}`,
  )
  expect(
    /3/.test(settingsText) && /gone\.epub/.test(settingsText),
    `the health panel must show counts and missing-file paths: ${settingsText.slice(0, 240)}`,
  )
  expect(
    /Scan library/.test(settingsText),
    `the health panel must offer the scan action: ${settingsText.slice(0, 240)}`,
  )
  await page.getByRole('link', { name: 'Show books' }).first().click()
  await page.waitForURL(/missing=cover/)
  await page.getByText('Missing covers').first().waitFor({ timeout: 8000 })

  // Adults can scope the Library to one member's shelf.
  await page.route('**/api/household/members', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ members: [ADMIN, CHILD, MEMBER] }),
    }),
  )
  await page.goto(`${base}/library`, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: 'Child' }).click()
  await page.getByText('Child Shelf Book').first().waitFor({ timeout: 8000 })
  expect(
    scopedUser === '2',
    `the member scope must query the child's shelf (got ${scopedUser})`,
  )
  expect(
    (await page.getByText("Child's shelf").count()) > 0,
    'the member scope must label whose shelf is shown',
  )

  // An ordinary member cannot open Administration by URL.
  await page.route('**/api/auth/me', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ user: MEMBER }),
    }),
  )
  await page.goto(`${base}/settings`, { waitUntil: 'networkidle' })
  await page.waitForURL(`${base}/`, { timeout: 8000 })
}

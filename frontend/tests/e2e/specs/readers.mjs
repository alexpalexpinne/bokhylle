function expect(condition, message) {
  if (!condition) {
    throw new Error(message)
  }
}

const BOOK_ID = 4242
const FILE_ID = 88

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

const BOOK = {
  id: BOOK_ID,
  title: 'Pocketbook Journey Book',
  authors: ['Journey Author'],
  language: 'en',
  series: null,
  seriesNumber: null,
  hasCover: false,
  hasDescription: true,
  rating: null,
  ratingCount: null,
  ratingSource: null,
  addedAt: 1,
  description: 'A book for the reader journey.',
  publicationYear: 2020,
  onShelf: true,
  preference: null,
  authorRefs: [{ id: 777, name: 'Journey Author' }],
  subjects: [],
  editions: [],
  files: [{ id: FILE_ID, editionId: 1, format: 'epub', size: 420000, filename: 'journey.epub' }],
}

async function mockReaderJourney(page) {
  await page.route(`**/api/books/${BOOK_ID}**`, (route) => {
    const url = new URL(route.request().url())
    if (url.pathname.endsWith('/acquisitions')) {
      return route.fulfill({ json: [] })
    }
    if (url.pathname.endsWith('/related')) {
      return route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ series: [], author: [], similar: [] }),
      })
    }
    if (url.pathname.endsWith('/deliver')) {
      return route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          id: 1,
          bookId: BOOK_ID,
          fileId: FILE_ID,
          targetId: POCKETBOOK.id,
          userId: 1,
          address: POCKETBOOK.address,
          status: 'SENT',
          errorMessage: null,
          createdAt: 1,
          updatedAt: 1,
        }),
      })
    }
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(BOOK),
    })
  })

  await page.route('**/api/delivery-targets', (route) => {
    if (route.request().method() !== 'GET') {
      return route.continue()
    }
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify([POCKETBOOK]),
    })
  })

  await page.route('**/api/delivery-targets/default', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        address: POCKETBOOK.address,
        source: 'personal',
        senderAddress: 'library@bokhylle.local',
        amazonUrl: 'https://www.amazon.com/hz/mycd/digital-console/contentlist/pdocs',
      }),
    }),
  )
}

export default async function readers(page, { base }) {
  await mockReaderJourney(page)

  // FE-34: a PocketBook household never sees Kindle/Amazon approval instructions.
  await page.goto(`${base}/library/${BOOK_ID}`, { waitUntil: 'networkidle' })
  await page.waitForSelector('text=Pocketbook Journey Book', { timeout: 15000 })
  const mainText = await page.locator('main').innerText()
  expect(
    !/Kindle/.test(mainText),
    `book detail must not mention Kindle for a PocketBook reader: ${mainText.slice(0, 240)}`,
  )
  // The author line links to the author's page.
  const authorLink = page.getByRole('link', { name: 'Journey Author' }).first()
  expect(
    (await authorLink.getAttribute('href')) === '/authors/777',
    'the book detail author must link to the author page',
  )
  expect(
    !/Amazon/.test(mainText),
    `book detail must not mention Amazon for a PocketBook reader: ${mainText.slice(0, 240)}`,
  )

  await page.getByRole('button', { name: /Send to my reader/ }).first().click()
  const dialog = page.getByRole('dialog', { name: 'Send to your reader' })
  await dialog.waitFor({ state: 'visible', timeout: 8000 })
  await dialog.getByText(POCKETBOOK.address).waitFor({ timeout: 8000 })
  const dialogText = await dialog.innerText()
  expect(
    /PocketBook address/.test(dialogText),
    `dialog must name the PocketBook destination: ${dialogText}`,
  )
  expect(dialogText.includes(POCKETBOOK.address), 'dialog must show the PocketBook address')
  expect(
    /Send-to-PocketBook/.test(dialogText),
    `dialog must show PocketBook guidance: ${dialogText}`,
  )
  expect(
    !/Amazon|approved personal document list/i.test(dialogText),
    `dialog must not show Kindle approval instructions: ${dialogText}`,
  )

  // FE-34: adding a reader from the dialog creates it as the chosen device type.
  await dialog.getByRole('button', { name: 'PocketBook', exact: true }).click()
  await dialog.getByPlaceholder('name@pbsync.com').fill('e2e-inline@pbsync.example')
  const [createResponse] = await Promise.all([
    page.waitForResponse(
      (candidate) =>
        candidate.url().endsWith('/api/delivery-targets') &&
        candidate.request().method() === 'POST',
      { timeout: 8000 },
    ),
    dialog.getByRole('button', { name: /^Send$/ }).click(),
  ])
  expect(
    createResponse.status() === 201,
    `creating a reader must succeed, got ${createResponse.status()}`,
  )
  const created = await createResponse.json()
  expect(
    created.type === 'pocketbook' && created.name === 'PocketBook',
    `a created PocketBook must not be stored as Kindle: ${JSON.stringify(created)}`,
  )
  await page.context().request.delete(`${base}/api/delivery-targets/${created.id}`)

  // FE-36: at phone width the same dialog stays inside the viewport and its controls are reachable.
  await dialog.getByRole('button', { name: 'Close' }).click()
  await dialog.waitFor({ state: 'hidden', timeout: 8000 })
  await page.setViewportSize({ width: 390, height: 844 })
  await page.getByRole('button', { name: /Send to my reader/ }).first().click()
  const mobileDialog = page.getByRole('dialog', { name: 'Send to your reader' })
  await mobileDialog.waitFor({ state: 'visible', timeout: 8000 })
  await mobileDialog.getByText(POCKETBOOK.address).waitFor({ timeout: 8000 })
  const box = await mobileDialog.boundingBox()
  expect(box !== null, 'mobile dialog must have a bounding box')
  expect(box.x >= -0.5 && box.y >= -0.5, `dialog must start inside the viewport: ${JSON.stringify(box)}`)
  expect(
    box.x + box.width <= 390.5,
    `dialog must not exceed the viewport width: ${JSON.stringify(box)}`,
  )
  expect(
    box.y + box.height <= 844.5,
    `dialog must not exceed the viewport height: ${JSON.stringify(box)}`,
  )
  const sendButton = mobileDialog.getByRole('button', { name: /^Send$/ })
  expect(await sendButton.isVisible(), 'the primary Send action must be reachable')
  const sendBox = await sendButton.boundingBox()
  expect(
    sendBox !== null && sendBox.y + sendBox.height <= 844.5,
    `the primary action must sit inside the viewport: ${JSON.stringify(sendBox)}`,
  )
  await mobileDialog.getByRole('button', { name: 'Close' }).click()
  await mobileDialog.waitFor({ state: 'hidden', timeout: 8000 })

  // A new address wins over the selected existing target: the instructions
  // must follow the device actually being used, not the selected row.
  await page.setViewportSize({ width: 1440, height: 1000 })
  const KINDLE = {
    id: 9,
    userId: 1,
    type: 'kindle',
    name: 'Kindle',
    address: 'sample@kindle.com',
    connector: 'email',
    enabled: true,
    isDefault: true,
    createdAt: 1,
    updatedAt: 1,
  }
  await page.route('**/api/delivery-targets', (route) => {
    if (route.request().method() !== 'GET') {
      return route.continue()
    }
    return route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify([KINDLE]),
    })
  })
  await page.goto(`${base}/library/${BOOK_ID}`, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: /Send to my reader/ }).first().click()
  const mismatchDialog = page.getByRole('dialog', { name: 'Send to your reader' })
  await mismatchDialog.waitFor({ state: 'visible', timeout: 8000 })
  await mismatchDialog.getByRole('button', { name: 'PocketBook', exact: true }).click()
  await mismatchDialog.getByPlaceholder('name@pbsync.com').fill('new@pbsync.example')
  const mismatchText = await mismatchDialog.innerText()
  expect(
    /PocketBook address/.test(mismatchText),
    `a new PocketBook address must drive the wording: ${mismatchText}`,
  )
  expect(
    !/Amazon|approved personal document list|Kindle address/i.test(mismatchText),
    `a new PocketBook address must not show Kindle instructions: ${mismatchText}`,
  )
  await mismatchDialog.getByRole('button', { name: 'Close' }).click()
  await mismatchDialog.waitFor({ state: 'hidden', timeout: 8000 })

  // Selecting an existing reader clears a typed address, so the destination
  // shown as selected is always the one actually sent to.
  let deliveredBody = null
  let createdTargets = 0
  page.on('request', (request) => {
    if (request.method() === 'POST' && request.url().endsWith('/api/delivery-targets')) {
      createdTargets += 1
    }
  })
  await page.route(`**/api/books/${BOOK_ID}/files/**/deliver`, (route) => {
    deliveredBody = route.request().postDataJSON()
    return route.fulfill({
      status: 202,
      contentType: 'application/json',
      body: JSON.stringify({
        id: 1,
        bookId: BOOK_ID,
        fileId: FILE_ID,
        targetId: KINDLE.id,
        userId: 1,
        address: KINDLE.address,
        status: 'SENT',
        errorMessage: null,
        createdAt: 1,
        updatedAt: 1,
      }),
    })
  })
  await page.goto(`${base}/library/${BOOK_ID}`, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: /Send to my reader/ }).first().click()
  const destinationDialog = page.getByRole('dialog', { name: 'Send to your reader' })
  await destinationDialog.waitFor({ state: 'visible', timeout: 8000 })
  await destinationDialog.getByPlaceholder('name@kindle.com').fill('typed@kindle.example')
  await destinationDialog.getByRole('radio').first().check()
  expect(
    (await destinationDialog.getByPlaceholder('name@kindle.com').inputValue()) === '',
    'selecting an existing reader must clear the typed address',
  )
  await destinationDialog.getByRole('button', { name: /^Send$/ }).click()
  await destinationDialog.getByText(/Sent\. It should arrive/).waitFor({ timeout: 8000 })
  expect(
    deliveredBody !== null && deliveredBody.targetId === KINDLE.id,
    `the selected reader must be the destination: ${JSON.stringify(deliveredBody)}`,
  )
  expect(createdTargets === 0, 'selecting an existing reader must not create a new one')
  await destinationDialog.getByRole('button', { name: 'Done' }).click()
  await destinationDialog.waitFor({ state: 'hidden', timeout: 8000 })

  // Changing the reader type updates the untouched default name.
  await page.route('**/api/delivery-targets', (route) => {
    if (route.request().method() !== 'GET') {
      return route.continue()
    }
    return route.fulfill({ status: 200, contentType: 'application/json', body: '[]' })
  })
  await page.goto(`${base}/profile/readers`, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: /Add reader/ }).first().click()
  const editor = page.getByRole('dialog', { name: 'Add a reader' })
  await editor.waitFor({ state: 'visible', timeout: 8000 })
  await editor.locator('select').selectOption('pocketbook')
  expect(
    (await editor.locator('input').first().inputValue()) === 'PocketBook',
    'the automatic reader name must follow the device type',
  )
  await editor.locator('input').first().fill('My PocketBook')
  await editor.locator('select').selectOption('kindle')
  expect(
    (await editor.locator('input').first().inputValue()) === 'My PocketBook',
    'a custom reader name must survive a type change',
  )

  // The household fallback is specifically the configured Kindle address.
  await page.route('**/api/delivery-targets/default', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        address: 'household@kindle.com',
        source: 'household',
        senderAddress: 'library@bokhylle.local',
        amazonUrl: 'https://www.amazon.com/hz/mycd/digital-console/contentlist/pdocs',
      }),
    }),
  )
  await page.goto(`${base}/library/${BOOK_ID}`, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: /Send to my reader/ }).first().click()
  const householdDialog = page.getByRole('dialog', { name: 'Send to your reader' })
  await householdDialog.getByText('household@kindle.com').waitFor({ timeout: 8000 })
  const householdText = await householdDialog.innerText()
  expect(
    /Kindle address/.test(householdText),
    `the household fallback must read as a Kindle: ${householdText}`,
  )
  expect(
    /approved personal document list/.test(householdText),
    `the household fallback must show the Amazon warning: ${householdText}`,
  )
  await householdDialog.getByRole('button', { name: 'Close' }).click()
  await householdDialog.waitFor({ state: 'hidden', timeout: 8000 })

  // M5: the advanced reader setup explains OPDS access and KOReader progress sync.
  await page.goto(`${base}/profile/readers`, { waitUntil: 'networkidle' })
  await page.getByText('Advanced reader setup').click()
  const readerSetup = await page.locator('main').innerText()
  expect(
    /\/opds/.test(readerSetup),
    `the setup must name the OPDS address: ${readerSetup.slice(0, 240)}`,
  )
  expect(
    /sync reading progress/.test(readerSetup),
    `the setup must explain KOReader progress sync: ${readerSetup.slice(0, 240)}`,
  )
  expect(
    /Continue reading/.test(readerSetup),
    `the setup must mention the Continue reading rail: ${readerSetup.slice(0, 240)}`,
  )

  // MCP: an agent token is created once, shown once, and revocable.
  await page.goto(`${base}/profile/integrations`, { waitUntil: 'networkidle' })
  const agentSection = page.locator('details').filter({ hasText: 'AI & integrations' })
  await agentSection
    .getByPlaceholder('Token name (e.g. Claude on laptop)')
    .fill('E2E Agent')
  await agentSection.getByRole('button', { name: 'Create token' }).click()
  await agentSection
    .getByText('Copy this token now — it is shown once:')
    .waitFor({ timeout: 8000 })
  await agentSection.getByText('E2E Agent').waitFor({ timeout: 8000 })
  const agentText = await agentSection.innerText()
  expect(/E2E Agent/.test(agentText), `the token must be listed: ${agentText.slice(0, 240)}`)
  expect(/read only/i.test(agentText), `the scope must be shown: ${agentText.slice(0, 240)}`)
  const secret = await agentSection.locator('code').first().innerText()
  expect(secret.length >= 32, `the one-time secret must be shown: ${secret}`)
  await agentSection.getByRole('button', { name: 'Revoke' }).first().click()
  await agentSection
    .getByText('E2E Agent')
    .waitFor({ state: 'hidden', timeout: 8000 })
}

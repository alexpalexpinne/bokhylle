import { expect } from '../support.mjs'
import { check } from './accessibility.mjs'

const ADMIN = { id: 1, username: 'admin', displayName: 'Mira', role: 'admin', profileType: 'adult', canAcquire: true, preferredLanguages: ['en'] }
const CHILD = { id: 2, username: 'nora', displayName: 'Nora', role: 'user', profileType: 'child', canAcquire: false, preferredLanguages: ['en'] }
const BOOKS = [
  { id: 7, title: 'The Lantern Map', authors: ['Mara Quill'], hasCover: false, language: 'en' },
  { id: 8, title: 'River Letters', authors: ['Iris North'], hasCover: false, language: 'en' },
  { id: 9, title: 'Hidden Household Book', authors: ['Amos Ryd'], hasCover: false, language: 'en' },
]

export default async function childSetup(page, { base }) {
  let user = ADMIN
  let members = [
    { ...ADMIN, readerCount: 0, disabled: false },
    { ...CHILD, canDiscover: true, canRequest: false, readerCount: 0, disabled: false },
  ]
  let householdBooks = BOOKS
  let shelfBooks = []
  let savedInterests = ['fantasy', 'humour']
  let failBooks = false
  let failCreate = false
  let failFinish = false
  let creates = []
  let shelfWrites = 0
  let interestWrites = []
  let likeWrites = []
  let updates = []
  let bookQueries = []
  let releaseDelayed
  let delayed = Promise.resolve()

  await page.route('**/api/**', async (route) => {
    const request = route.request()
    const url = new URL(request.url())
    const path = url.pathname
    const method = request.method()
    let body = []
    let status = 200
    if (path === '/api/auth/me') body = { user }
    else if (path === '/api/demo') body = { enabled: false }
    else if (path === '/api/notifications') body = { items: [], unread: 0, pendingRequests: 0, pendingRequestItems: [] }
    else if (path === '/api/admin/users/profiles') body = { users: members.map((member) => ({ userId: member.id, profileType: member.profileType })) }
    else if (path === '/api/admin/users') {
      if (method === 'POST') {
        const input = request.postDataJSON()
        creates.push(input)
        if (failCreate) { failCreate = false; status = 500; body = { message: 'Could not create the profile', code: 'internal_error' } }
        else { body = { ...input, id: members.length + 1, readerCount: 0, disabled: false }; members = [...members, body] }
      } else body = members
    } else if (/^\/api\/admin\/users\/\d+$/.test(path)) {
      const input = request.postDataJSON()
      updates.push(input)
      const id = Number(path.split('/').at(-1))
      members = members.map((member) => member.id === id ? { ...member, ...input } : member)
      body = members.find((member) => member.id === id)
    } else if (/^\/api\/users\/\d+\/shelf\//.test(path)) { shelfWrites += 1; status = 204 }
    else if (path === '/api/books') {
      bookQueries.push(url.searchParams.toString())
      if (failBooks) { failBooks = false; status = 500; body = { message: 'Shelf is temporarily unavailable', code: 'internal_error' } }
      else { const books = user.profileType === 'child' ? shelfBooks : householdBooks; body = { items: books, total: books.length, page: 1, pageSize: 24, letters: [] } }
    } else if (path === '/api/books/search') {
      const query = url.searchParams.get('q')
      if (query === 'delayed') { await delayed; body = [BOOKS[2]] }
      else body = query === 'river' ? [BOOKS[1]] : query === 'lantern' ? [BOOKS[0]] : []
    } else if (/\/cover$/.test(path)) { status = 404 }
    else if (path === '/api/profile/onboarding') body = { onboarded: true, interests: savedInterests }
    else if (path === '/api/profile/rejected') body = { items: [] }
    else if (path === '/api/profile/liked') body = { items: [{ bookId: 7, onShelf: true, readable: true }] }
    else if (path === '/api/profile/interests') { const input = request.postDataJSON(); interestWrites.push(input.subjects); savedInterests = input.subjects; body = { ok: true } }
    else if (path === '/api/profile/onboarded') {
      if (failFinish) { failFinish = false; status = 500; body = { message: 'Could not finish setup', code: 'internal_error' } }
      else body = { ok: true }
    } else if (/\/preference$/.test(path)) { likeWrites.push(request.postDataJSON()); body = { ok: true } }
    else if (path === '/api/home/spotlight') body = { items: [], recommendations: [] }
    else if (path === '/api/home/updates') body = { library: [], discoveries: [], ready: [] }
    else if (path === '/api/books/facets') body = { formats: [], languages: [], series: [], subjects: [], publicationKinds: [] }
    else if (path === '/api/requests') body = { items: [] }
    else if (path === '/api/delivery-targets/default') body = { address: null, senderAddress: null, amazonUrl: '' }
    else if (path === '/api/profile') body = { user }
    return route.fulfill({ status, contentType: 'application/json', body: status === 204 ? '' : JSON.stringify(body) })
  })

  await page.goto(`${base}/settings/household?from=setup`, { waitUntil: 'networkidle' })
  await page.getByRole('navigation', { name: 'Settings sections' }).getByRole('link', { name: 'Household', exact: true }).click()
  const row = page.getByText('@nora', { exact: false }).locator('xpath=../..').locator('..')
  await row.getByRole('button', { name: 'Edit', exact: true }).click()
  await page.getByText(/Current access: Explore only/).waitFor()
  await check(page, 'existing child access', '[role=dialog]')
  expect(await page.getByRole('group', { name: 'How can this child find new books?' }).getByRole('radio', { checked: true }).count() === 0, 'legacy access must not be converted implicitly')
  await page.getByLabel('Display name', { exact: true }).fill('Nora Reader')
  await page.getByRole('button', { name: 'Save', exact: true }).click()
  expect(updates.at(-1).canDiscover === true && updates.at(-1).canRequest === false, 'unrelated edits preserve legacy access')

  for (const [label, discover, ask] of [['Assigned books only', false, false], ['Search and ask', false, true], ['Explore and ask', true, true]]) {
    await page.getByText('@nora', { exact: false }).locator('xpath=../..').locator('..').getByRole('button', { name: 'Edit', exact: true }).click()
    await page.getByRole('radio', { name: new RegExp('^' + label) }).check()
    await page.getByRole('button', { name: 'Save', exact: true }).click()
    expect(updates.at(-1).canDiscover === discover && updates.at(-1).canRequest === ask, label + ' must save the intended permissions')
  }

  async function beginChild(name) {
    await page.getByRole('button', { name: 'Add user', exact: true }).click()
    await page.getByLabel('Username', { exact: true }).fill(name)
    await page.getByRole('dialog').locator('input[type=password]').fill('482915')
    expect(await page.getByRole('radio', { name: 'Initials', exact: true }).isChecked(), 'new profiles default to initials')
    await page.getByRole('radio', { name: 'Owl', exact: true }).check()
    await page.getByRole('combobox', { name: /^Profile type/ }).selectOption('child')
    await page.getByRole('radio', { name: /^Assigned books only/ }).check()
    await page.getByRole('button', { name: 'Choose starting books', exact: true }).click()
    await page.getByRole('dialog', { name: 'Choose starting books' }).waitFor()
  }
  await beginChild('emma')
  expect(creates.length === 0, 'the account is not created before choosing starting books')
  expect(await page.getByRole('button', { name: 'Create child profile' }).isDisabled(), 'empty setup requires an explicit choice')
  await page.getByRole('checkbox', { name: /The Lantern Map/ }).check()
  await page.getByLabel('Search household books', { exact: true }).fill('river')
  await page.getByRole('checkbox', { name: /River Letters/ }).check()
  expect(await page.getByRole('list', { name: 'Selected starting books' }).getByText('The Lantern Map').count() === 1, 'selections survive searching')

  delayed = new Promise((resolve) => { releaseDelayed = resolve })
  const delayedRequest = page.waitForRequest((request) => new URL(request.url()).searchParams.get('q') === 'delayed')
  await page.getByLabel('Search household books', { exact: true }).fill('delayed')
  await delayedRequest
  await page.getByLabel('Search household books', { exact: true }).fill('river')
  await page.getByRole('checkbox', { name: /River Letters/ }).waitFor()
  const delayedResponse = page.waitForResponse((response) => new URL(response.url()).searchParams.get('q') === 'delayed')
  releaseDelayed()
  await delayedResponse
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))))
  expect(await page.getByRole('checkbox', { name: /Hidden Household Book/ }).count() === 0, 'late search responses must not replace current results')
  await page.getByRole('button', { name: 'Cancel', exact: true }).click()
  await page.getByRole('dialog', { name: 'Discard unsaved changes?' }).waitFor()
  await page.getByRole('button', { name: 'Keep editing' }).click()
  await page.getByText('2 books selected', { exact: true }).waitFor()
  await page.goBack()
  await page.getByRole('dialog', { name: 'Discard unsaved changes?' }).waitFor()
  await page.getByRole('button', { name: 'Keep editing' }).click()
  expect(new URL(page.url()).search === '', 'cancelled back navigation keeps the creation flow')
  for (const viewport of [{ width: 1280, height: 900 }, { width: 390, height: 844 }]) {
    await page.setViewportSize(viewport)
    await check(page, 'starting books ' + viewport.width, '[role=dialog]')
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), 'starting-book selection must fit the viewport')
  }
  failCreate = true
  await page.getByRole('button', { name: 'Create child profile' }).click()
  await page.getByRole('alert').getByText('Could not create the profile').waitFor()
  expect(await page.getByText('2 books selected', { exact: true }).count() === 1, 'failed creation retains the selection for retry')
  await page.getByRole('button', { name: 'Create child profile' }).click()
  await page.getByText('emma added with 2 starting books.', { exact: true }).waitFor()
  expect(creates.at(-1).avatarPreset === 'owl', 'the chosen mark uses the same creation request')
  expect(JSON.stringify(creates.at(-1).startingBookIds) === '[7,8]' && shelfWrites === 0, 'starting books use the atomic creation request')

  householdBooks = []
  await beginChild('empty')
  await page.getByText(/The household library is empty/).waitFor()
  await page.getByRole('checkbox', { name: /Set up books later/ }).check()
  await page.getByRole('button', { name: 'Create child profile' }).click()
  await page.getByText(/empty added with an empty shelf/).waitFor()
  expect(creates.at(-1).startingBookIds.length === 0, 'an empty shelf is allowed only when explicitly chosen')

  for (const [discover, ask, copy, destination] of [
    [false, false, 'An adult chooses books for your shelf.', '/'],
    [false, true, 'Heard about a book?', '/requests'],
    [true, true, 'Explore Discover and suggestions, open book details', '/discover'],
    [true, false, 'If you find a book you want, ask an adult', '/discover'],
  ]) {
    user = { ...CHILD, canDiscover: discover, canRequest: ask }
    await page.goto(`${base}/welcome`, { waitUntil: 'networkidle' })
    await page.getByText(copy, { exact: false }).waitFor()
    await page.getByText(/Your shelf is waiting for its first books/).waitFor()
    await page.getByRole('button', { name: 'Next', exact: true }).click()
    await page.getByRole('heading', { name: 'What do you like reading?' }).waitFor()
    expect(await page.getByRole('heading', { name: 'Pick books you like' }).count() === 0, 'empty shelves skip the favourites step')
    await page.getByRole('button', { name: 'Fantasy', exact: true }).waitFor()
    expect(await page.getByRole('button', { name: 'Fantasy', exact: true }).getAttribute('aria-pressed') === 'true', 'restart preloads normalized interests')
    const beforeWrites = interestWrites.length
    await page.getByRole('button', { name: destination === '/' ? 'Finish' : discover ? 'Explore books' : 'Search and ask', exact: true }).click()
    await page.waitForURL(`${base}${destination}`)
    expect(interestWrites.length === beforeWrites, 'finishing unchanged interests does not replace them')
  }

  user = { ...CHILD, canDiscover: false, canRequest: true }
  failBooks = true
  await page.goto(`${base}/welcome`, { waitUntil: 'networkidle' })
  await page.getByText('Shelf is temporarily unavailable').waitFor()
  expect(await page.getByText(/Your shelf is waiting/).count() === 0, 'load failure must not appear as an empty shelf')
  expect(await page.getByRole('button', { name: 'Next', exact: true }).isDisabled(), 'the wizard waits for the actual shelf')
  shelfBooks = [BOOKS[0]]
  await page.getByRole('button', { name: 'Try again' }).click()
  await page.getByRole('button', { name: 'Next', exact: true }).click()
  await page.getByRole('button', { name: 'Liked', exact: true }).waitFor()
  expect(likeWrites.length === 0, 'restart does not clear or recreate existing likes')
  await page.getByRole('button', { name: 'Next', exact: true }).click()
  await page.getByLabel('Search reading interests', { exact: true }).fill('Humour')
  expect(await page.getByRole('button', { name: 'Humor', exact: true }).getAttribute('aria-pressed') === 'true', 'the obvious spelling alias matches an existing selection')
  expect(await page.getByRole('button', { name: /^Add / }).count() === 0, 'the alias must not offer a duplicate custom topic')
  await page.getByLabel('Search reading interests', { exact: true }).fill('Nordic history')
  await page.getByRole('button', { name: 'Add “Nordic history”', exact: true }).click()
  await page.getByLabel('Search reading interests', { exact: true }).fill('Architecture')
  expect(await page.getByRole('button', { name: 'Remove interest Nordic history' }).count() === 1, 'selected interests stay visible while filtering')
  await page.setViewportSize({ width: 390, height: 844 })
  for (const theme of ['paper', 'ink']) {
    await page.evaluate((theme) => { document.documentElement.setAttribute('data-theme', theme); document.documentElement.style.colorScheme = theme === 'ink' ? 'dark' : 'light' }, theme)
    await page.evaluate(async () => {
      await new Promise((resolve) => requestAnimationFrame(resolve))
      await Promise.all(document.getAnimations().map((animation) => animation.finished.catch(() => {})))
    })
    await check(page, 'child interests ' + theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), 'the interest picker must fit narrow screens')
  }
  await page.getByRole('button', { name: 'Finish', exact: true }).click()
  await page.waitForURL(`${base}/`)
  expect(JSON.stringify(interestWrites.at(-1)) === '["fantasy","humour","Nordic history"]', 'custom topics preserve previous interests and order')
  expect(bookQueries.filter((query) => query.includes('mine=true')).length > 0, 'child favourites only fetch the assigned shelf')

  await page.goto(`${base}/welcome`, { waitUntil: 'networkidle' })
  failFinish = true
  const writes = interestWrites.length
  await page.getByRole('button', { name: 'Skip', exact: true }).click()
  await page.getByRole('alert').getByText('Could not finish setup').waitFor()
  expect(page.url().endsWith('/welcome'), 'failed skip must not pretend onboarding was completed')
  await page.getByRole('button', { name: 'Skip', exact: true }).click()
  await page.waitForURL(`${base}/`)
  expect(interestWrites.length === writes, 'skip preserves stored interests')

  user = ADMIN
  await page.goto(`${base}/welcome`, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: 'Fantasy', exact: true }).waitFor()
  expect(await page.getByRole('button', { name: 'Fantasy', exact: true }).getAttribute('aria-pressed') === 'true', 'adult setup also preloads existing interests')
  const adultWrites = interestWrites.length
  for (let step = 0; step < 3; step += 1) await page.getByRole('button', { name: 'Next', exact: true }).click()
  expect(interestWrites.length === adultWrites, 'moving through unchanged adult interests does not overwrite them')
  await page.getByRole('textbox', { name: 'Search books' }).fill('lantern')
  await page.getByRole('button', { name: 'Liked', exact: true }).waitFor()
  expect(likeWrites.length === 0, 'adult setup preloads likes without changing them')
  failFinish = true
  await page.getByRole('button', { name: 'Skip', exact: true }).click()
  await page.getByRole('alert').getByText('Could not finish setup').waitFor()
  expect(page.url().endsWith('/welcome'), 'failed adult setup completion remains recoverable')
  await page.getByRole('button', { name: 'Skip', exact: true }).click()
  await page.waitForURL(`${base}/`)
  expect(interestWrites.length === adultWrites, 'adult skip does not replace stored interests')

  savedInterests = Array.from({ length: 24 }, (_, index) => 'Custom topic ' + index)
  await page.goto(`${base}/welcome`, { waitUntil: 'networkidle' })
  await page.getByLabel('Search reading interests', { exact: true }).fill('Space')
  expect(await page.getByRole('button', { name: 'Space', exact: true }).isDisabled(), 'the picker respects the 24-interest cap')
  await page.getByLabel('Search reading interests', { exact: true }).fill('Architecture')
  expect(await page.getByRole('button', { name: 'Add “Architecture”', exact: true }).isDisabled(), 'custom topics respect the same cap')
  expect(await page.getByRole('button', { name: 'Remove interest Custom topic 0', exact: true }).isEnabled(), 'existing selections can still be removed at the cap')
}

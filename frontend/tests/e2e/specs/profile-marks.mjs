import { expect } from '../support.mjs'
import { check } from './accessibility.mjs'

const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/lQAAAABJRU5ErkJggg==', 'base64')
const MARKS = ['Fox', 'Owl', 'Cat', 'Bear', 'Whale', 'Book', 'Tree', 'Mountain', 'Moon', 'Leaf']

export default async function profileMarks(page, { base }) {
  let signedIn = true
  let user = {
    id: 1, username: 'mira', displayName: 'Mira', role: 'admin', profileType: 'adult',
    preferredLanguages: ['en'], defaultLanguage: 'en', acquisitionMode: 'automatic',
    canRequest: true, canDiscover: false, canAcquire: true, avatarPreset: null, avatarVersion: null,
  }
  let failMark = false
  let failPhoto = false
  const writes = []
  await page.route('**/api/**', (route) => {
    const path = new URL(route.request().url()).pathname
    const method = route.request().method()
    let body = []
    if (path === '/api/demo') body = { enabled: false }
    else if (path === '/api/auth/me') {
      if (!signedIn) return route.fulfill({ status: 401, json: { message: 'Sign in', code: 'unauthorized' } })
      body = { user }
    } else if (path === '/api/profile/onboarding') body = { onboarded: true, interests: [] }
    else if (path === '/api/profile/stats') body = { shelf: 0, authors: 0, liked: 0, booksSent: 0 }
    else if (path === '/api/profile/liked') body = { items: [] }
    else if (path === '/api/profile/hidden-subjects') body = { hidden: [] }
    else if (path.endsWith('/tokens')) body = { tokens: [] }
    else if (path === '/api/profile/avatar/preset' && method === 'PUT') {
      const payload = route.request().postDataJSON()
      writes.push(payload)
      if (failMark) { failMark = false; return route.fulfill({ status: 503, json: { message: 'Could not save mark', code: 'unavailable' } }) }
      user = { ...user, ...payload }
      body = { user }
    } else if (path === '/api/profile/avatar') {
      if (method === 'PUT') { user = { ...user, avatarVersion: 1 }; return route.fulfill({ status: 204 }) }
      if (method === 'DELETE') { user = { ...user, avatarVersion: null }; return route.fulfill({ status: 204 }) }
      return route.fulfill({ contentType: 'image/png', body: PNG })
    } else if (path === '/api/auth/users') body = { users: [
      { username: 'mira', displayName: 'Mira', role: 'admin', profileType: 'adult', authMode: 'password', avatarPreset: 'book', avatarUrl: null },
      { username: 'nora', displayName: 'Nora', role: 'user', profileType: 'child', authMode: 'pin', avatarPreset: 'fox', avatarUrl: null },
      { username: 'initial', displayName: 'Initial', role: 'user', profileType: 'adult', authMode: 'pin', avatarPreset: null, avatarUrl: null },
      { username: 'photo', displayName: 'Photo', role: 'user', profileType: 'adult', authMode: 'password', avatarPreset: 'leaf', avatarUrl: '/api/auth/users/4/avatar?v=1' },
    ] }
    else if (path === '/api/auth/users/4/avatar') return route.fulfill(failPhoto ? { status: 404 } : { contentType: 'image/png', body: PNG })
    else if (path === '/api/notifications') body = { items: [], unread: 0, pendingRequests: 0, pendingRequestItems: [] }
    else if (path === '/api/delivery-targets/default') body = { address: null, senderAddress: null, amazonUrl: '' }
    return route.fulfill({ json: body })
  })

  async function openPicture() {
    await page.getByRole('button', { name: 'Change picture', exact: true }).click()
    await page.getByRole('dialog', { name: 'Profile picture', exact: true }).waitFor()
  }
  await page.goto(`${base}/profile`, { waitUntil: 'networkidle' })
  await openPicture()
  expect(await page.getByRole('radio', { name: 'Initials', exact: true }).isChecked(), 'existing profiles retain initials')
  for (const mark of MARKS) expect(await page.getByRole('radio', { name: mark, exact: true }).count() === 1, 'all ten marks are available')
  await page.getByRole('radio', { name: 'Fox', exact: true }).click()
  await page.getByRole('status').getByText('Profile mark saved.', { exact: true }).waitFor()
  expect(writes.at(-1).avatarPreset === 'fox' && Object.keys(writes.at(-1)).length === 1, 'mark changes send only the preset ID')
  await page.getByRole('button', { name: 'Close', exact: true }).click()
  await page.reload({ waitUntil: 'networkidle' })
  await openPicture()
  expect(await page.getByRole('radio', { name: 'Fox', exact: true }).isChecked(), 'saved mark survives reloading')
  failMark = true
  await page.getByRole('radio', { name: 'Owl', exact: true }).click()
  await page.getByRole('alert').getByText('Could not save mark').waitFor()
  expect(await page.getByRole('radio', { name: 'Fox', exact: true }).isChecked(), 'failed saves retain the existing mark')
  await page.getByRole('radio', { name: 'Owl', exact: true }).click()
  await page.getByRole('status').getByText('Profile mark saved.', { exact: true }).waitFor()

  // Uploaded photos stay independent of the selected fallback mark.
  await page.getByLabel('Choose a profile picture', { exact: true }).setInputFiles({ name: 'portrait.png', mimeType: 'image/png', buffer: PNG })
  await page.getByRole('dialog', { name: 'Profile picture' }).waitFor({ state: 'hidden' })
  await page.locator('img[src="/api/profile/avatar?v=1"]').last().waitFor()
  await openPicture()
  expect(await page.getByRole('radio', { name: 'Owl', exact: true }).isChecked(), 'upload preserves the chosen mark')
  await page.getByRole('radio', { name: 'Book', exact: true }).click()
  await page.getByRole('status').getByText('Profile mark saved.', { exact: true }).waitFor()
  expect(user.avatarVersion === 1, 'choosing a mark does not remove a personal photo')
  await page.getByRole('button', { name: 'Remove picture', exact: true }).click()
  await page.getByRole('dialog', { name: 'Profile picture' }).waitFor({ state: 'hidden' })
  expect(await page.locator('img[src*="/api/profile/avatar"]').count() === 0, 'removing the photo reveals the bundled mark')
  expect(await page.locator('img[src="/profile-marks/book.svg"]').count() >= 1, 'the chosen fallback mark is displayed')

  await openPicture()
  for (const theme of ['paper', 'ink']) {
    await page.evaluate((value) => { document.documentElement.dataset.theme = value }, theme)
    for (const width of [320, 390, 1440]) {
      await page.setViewportSize({ width, height: 844 })
      await page.getByRole('dialog').evaluate((dialog) => Promise.all(dialog.getAnimations({ subtree: true }).map((animation) => animation.finished.catch(() => {}))))
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), 'profile marks fit narrow phones')
      await check(page, `profile marks ${theme} ${width}`, '[role=dialog]')
    }
  }
  await page.getByRole('radio', { name: 'Initials', exact: true }).click()
  await page.getByRole('status').getByText('Initials selected.', { exact: true }).waitFor()
  await page.getByRole('radio', { name: 'Initials', exact: true }).focus()
  await page.keyboard.press('ArrowRight')
  await page.getByRole('status').getByText('Profile mark saved.', { exact: true }).waitFor()
  await page.waitForFunction(() => document.activeElement?.getAttribute('value') === 'fox')
  expect(await page.getByRole('radio', { name: 'Fox', exact: true }).isChecked(), 'keyboard navigation saves a mark and retains focus')
  await page.getByRole('radio', { name: 'Initials', exact: true }).click()
  await page.getByRole('status').getByText('Initials selected.', { exact: true }).waitFor()
  await page.getByRole('button', { name: 'Close', exact: true }).click()
  expect(await page.locator('img[src*="/profile-marks/"]').count() === 0, 'initials restore the default rendering')

  // Children use the same set in My settings, without editing account permissions.
  user = { ...user, username: 'nora', displayName: 'Nora', role: 'user', profileType: 'child', canAcquire: false }
  await page.goto(`${base}/profile`, { waitUntil: 'networkidle' })
  await page.getByRole('heading', { name: 'My settings', exact: true }).waitFor()
  await openPicture()
  await page.getByRole('radio', { name: 'Leaf', exact: true }).click()
  await page.getByRole('status').getByText('Profile mark saved.', { exact: true }).waitFor()
  expect(user.avatarPreset === 'leaf' && user.canAcquire === false, 'children personalize their own picture without changing access')

  signedIn = false
  await page.goto(`${base}/login`, { waitUntil: 'networkidle' })
  await page.getByRole('heading', { name: 'Who’s reading?', exact: true }).waitFor()
  const adult = page.locator('button[data-profile-username="mira"]')
  const child = page.locator('button[data-profile-username="nora"]')
  const initial = page.locator('button[data-profile-username="initial"]')
  const photo = page.locator('button[data-profile-username="photo"]')
  expect(await adult.locator('img').getAttribute('src') === '/profile-marks/book.svg', 'adult sign-in displays the saved mark')
  expect(await child.locator('img').getAttribute('src') === '/profile-marks/fox.svg', 'child sign-in displays the saved mark')
  expect(await initial.locator('img').count() === 0, 'unconfigured sign-in profiles retain initials')
  expect(await photo.locator('img').last().getAttribute('src') === '/api/auth/users/4/avatar?v=1', 'sign-in photo takes precedence')
  await page.setViewportSize({ width: 390, height: 844 })
  await check(page, 'profile marks sign-in')
  failPhoto = true
  await page.reload({ waitUntil: 'networkidle' })
  await page.waitForFunction(() => document.querySelector('img[src="/api/auth/users/4/avatar?v=1"]')?.style.display === 'none')
  expect(await photo.locator('img[src="/profile-marks/leaf.svg"]').isVisible(), 'a failed sign-in photo falls back to the chosen mark')
}

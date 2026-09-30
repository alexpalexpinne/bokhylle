import { expect } from '../support.mjs'
import { check } from './accessibility.mjs'

const profiles = [
  { username: 'mira-account', displayName: 'Mira', role: 'admin', profileType: 'adult', authMode: 'password', avatarUrl: '/api/auth/users/1/avatar?v=1' },
  { username: 'oskar-account', displayName: 'Oskar', role: 'user', profileType: 'adult', authMode: 'password', avatarUrl: '/api/auth/users/2/avatar?v=1' },
  { username: 'nora-account', displayName: 'Nora', role: 'user', profileType: 'child', authMode: 'pin', avatarUrl: null },
]

export default async function login(page, { base }) {
  let users = profiles
  let sourceStatus = 200
  let submitted
  const fakeApi = (route) => {
    const path = new URL(route.request().url()).pathname
    if (path === '/api/auth/me') return route.fulfill({ status: 401, json: { code: 'unauthorized', message: 'Authentication required' } })
    if (path === '/api/demo') return route.fulfill({ json: { enabled: false } })
    if (path === '/api/auth/users') return route.fulfill({ status: sourceStatus, json: { users } })
    if (path === '/api/auth/users/1/avatar') return route.fulfill({ contentType: 'image/png', body: Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/lQAAAABJRU5ErkJggg==', 'base64') })
    if (path === '/api/auth/users/2/avatar') return route.fulfill({ status: 404 })
    if (path === '/api/auth/login') {
      submitted = route.request().postDataJSON()
      return route.fulfill({ status: 401, json: { code: 'unauthorized', message: 'Bad credential' } })
    }
    return route.continue()
  }
  await page.route('**/api/**', fakeApi)
  for (const theme of ['paper', 'ink']) {
    for (const width of [1440, 390, 320]) {
      await page.setViewportSize({ width, height: width === 1440 ? 1000 : 844 })
      await page.goto(`${base}/login`, { waitUntil: 'networkidle' })
      await page.evaluate((value) => { document.documentElement.dataset.theme = value }, theme)
      await page.getByRole('heading', { name: 'Who’s reading?', exact: true }).waitFor()
      const mira = page.getByRole('button', { name: 'Mira', exact: true })
      const oskar = page.getByRole('button', { name: 'Oskar', exact: true })
      const nora = page.getByRole('button', { name: 'Nora', exact: true })
      const first = await mira.boundingBox()
      const second = await oskar.boundingBox()
      expect(Math.abs(first.y - second.y) < 2, 'profiles should sit horizontally')
      const mark = await page.locator('svg').first().boundingBox()
      const wordmark = await page.locator('[style*="bokhylle-wordmark"]').boundingBox()
      expect(wordmark.y > mark.y + mark.height, 'wordmark should sit below the bookshelf')
      expect(Math.abs(mark.x + mark.width / 2 - width / 2) < 2, 'logo should be centered')
      expect(await mira.locator('img').evaluate((image) => image.complete && image.naturalWidth > 0), 'profile picture should load')
      expect(await oskar.locator('img').evaluate((image) => image.style.display === 'none'), 'broken picture should leave the initial visible')
      expect(await nora.locator('img').count() === 0, 'profiles without pictures should use initials')
      expect(await page.getByRole('button', { name: 'Sign in', exact: true }).count() === 0, 'closed panel should be inaccessible')
      await check(page, `${theme} ${width} profile picker`)

      await mira.focus()
      await page.keyboard.press('Enter')
      const password = page.getByLabel('Password', { exact: true })
      await password.waitFor()
      await page.waitForFunction(() => document.activeElement === document.querySelector('input[name="password"]'))
      expect(await password.evaluate((input) => input === document.activeElement), 'keyboard selection should focus password')
      expect(await page.getByRole('group', { name: 'Selected profile', exact: true }).getByRole('button').count() === 1, 'only the selected profile should remain visible')
      expect(!await oskar.isVisible() && !await nora.isVisible(), 'other profiles should be hidden during sign-in')
      expect(await page.getByRole('heading', { name: 'Sign in', exact: true }).isVisible(), 'selected screen should have a sign-in heading')
      expect(await page.getByLabel('Username', { exact: true }).count() === 0, 'profile sign-in should not ask for username')
      expect(await page.locator('input[name="username"]').inputValue() === 'mira-account', 'internal username should remain available to autofill')
      await check(page, `${theme} ${width} password panel`)
      await password.fill('previous secret')
      await page.getByRole('button', { name: 'Back to profiles', exact: true }).click()
      expect(await mira.evaluate((button) => button === document.activeElement), 'back should return focus to the selected profile')
      expect(await page.getByRole('group', { name: 'Household profiles', exact: true }).getByRole('button').count() === 3, 'back should restore all profile choices')
      await nora.click()
      const pin = page.getByLabel('PIN', { exact: true })
      await page.waitForFunction(() => document.activeElement === document.querySelector('input[name="password"]'))
      expect(await pin.inputValue() === '', 'switching profiles must clear the previous secret')
      expect(await pin.evaluate((input) => input === document.activeElement), 'PIN selection should focus the PIN')
      expect(await pin.getAttribute('inputmode') === 'numeric', 'PIN should use a numeric keyboard')
      expect(await nora.getAttribute('aria-pressed') === 'true', 'selected profile should be announced')
      await page.waitForFunction(() => document.querySelector('#sign-in-panel').getAttribute('aria-busy') !== 'true')
      await pin.fill('12a3456')
      expect(await pin.inputValue() === '123456', 'PIN should filter non-digits before applying the six-digit limit')
      await pin.fill('123456')
      await pin.press('7')
      expect(await pin.inputValue() === '123456', 'PIN should accept only six digits')
      await nora.click()
      expect(await pin.inputValue() === '123456', 'tapping the selected profile again should preserve the entered credential')
      await page.getByLabel('Remember this device').uncheck()
      await check(page, `${theme} ${width} PIN panel`)
      await page.getByRole('button', { name: 'Sign in', exact: true }).click()
      await page.getByRole('alert').filter({ hasText: 'Incorrect PIN' }).waitFor()
      expect(submitted.username === 'nora-account' && submitted.password === '123456' && submitted.remember === false, 'selected account, credential, and remember choice should reach login')
      expect(await pin.getAttribute('aria-invalid') === 'true', 'credential error should be associated with input')
      await check(page, `${theme} ${width} credential error`)
      await page.getByRole('button', { name: 'Back to profiles', exact: true }).click()
      await mira.click()
      expect(await page.getByRole('alert').count() === 0, 'changing account should clear stale errors')
      await page.getByRole('button', { name: 'Sign in another way', exact: true }).click()
      const username = page.getByLabel('Username', { exact: true })
      await page.waitForFunction(() => document.activeElement === document.querySelector('input[name="username"]'))
      expect(await username.evaluate((input) => input === document.activeElement), 'manual fallback should focus username')
      expect(await page.getByRole('group', { name: /profiles|profile/ }).count() === 0, 'manual sign-in should hide profile choices')
      expect(await username.inputValue() === '', 'manual fallback should clear previous account')
      await username.fill('legacy-account')
      await page.getByLabel('PIN or password', { exact: true }).fill('manual-secret')
      await page.getByRole('button', { name: 'Sign in', exact: true }).click()
      await page.getByRole('alert').waitFor()
      expect(submitted.username === 'legacy-account' && submitted.password === 'manual-secret', 'manual sign-in should use typed credentials')
      await check(page, `${theme} ${width} username fallback`)
      await page.getByRole('button', { name: 'Back to profiles', exact: true }).click()
      expect(await mira.evaluate((button) => button === document.activeElement), 'back should return focus to the previous profile')
      expect(await page.locator('#sign-in-panel').getAttribute('inert') !== null, 'closed form should not accept keyboard focus')
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), 'sign-in should not scroll horizontally')
    }
  }
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await page.getByRole('button', { name: 'Nora', exact: true }).click()
  const duration = await page.locator('#sign-in-panel').evaluate((panel) => Number.parseFloat(getComputedStyle(panel).transitionDuration))
  expect(duration <= 0.001, 'reduced motion should suppress expansion animation')

  const touchContext = await page.context().browser().newContext({
    viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true,
  })
  try {
    const touch = await touchContext.newPage()
    touch.setDefaultTimeout(15000)
    // Headless browsers do not display a native keyboard. Model its changing
    // visual viewport while leaving the layout viewport at the phone's height.
    await touch.addInitScript(() => {
      let height = null
      let offsetTop = 0
      let scale = 1
      const viewport = new EventTarget()
      Object.defineProperties(viewport, {
        height: { get: () => height ?? window.innerHeight },
        offsetTop: { get: () => offsetTop },
        scale: { get: () => scale },
      })
      Object.defineProperty(window, 'visualViewport', { get: () => viewport })
      window.setKeyboardViewport = (value, offset = 0, zoom = 1) => {
        height = value
        offsetTop = offset
        scale = zoom
        viewport.dispatchEvent(new Event('resize'))
      }
      // React handles the tap at its root before this document listener.
      // Focus must already be set inside that trusted event, not after animation.
      document.addEventListener('click', (event) => {
        const button = event.target.closest('button')
        if (button?.hasAttribute('data-profile-username')) {
          window.profileTapFocused = document.activeElement === document.querySelector('input[name="password"]')
          window.profileMotion = []
          const other = [...document.querySelectorAll('button[data-profile-username]')].find((profile) => profile !== button)
          const sample = () => {
            const profile = button.getBoundingClientRect()
            const panel = document.querySelector('#sign-in-panel')
            window.profileMotion.push({
              x: profile.x, y: profile.y, width: profile.width,
              otherOpacity: other ? Number(getComputedStyle(other).opacity) : 0,
              otherHidden: !other || other.hidden,
              panelHeight: panel.getBoundingClientRect().height,
              logoTop: document.querySelector('main svg').getBoundingClientRect().top + window.scrollY,
            })
            if (panel.getAttribute('aria-busy') === 'true') requestAnimationFrame(sample)
          }
          requestAnimationFrame(sample)
        } else if (button?.textContent === 'Sign in another way') {
          window.manualTapFocused = document.activeElement === document.querySelector('input[name="username"]')
        }
      })
    })
    await touch.route('**/api/**', fakeApi)
    for (const width of [390, 320]) {
      await touch.setViewportSize({ width, height: 844 })
      await touch.goto(`${base}/login`, { waitUntil: 'networkidle' })
      const original = await touch.getByRole('button', { name: 'Nora', exact: true }).boundingBox()
      await touch.getByRole('button', { name: 'Nora', exact: true }).tap()
      const pin = touch.getByLabel('PIN', { exact: true })
      expect(await touch.evaluate(() => window.profileTapFocused === true), 'profile tap should focus the credential before the trusted event ends')
      expect(await pin.evaluate((input) => input === document.activeElement), 'touch selection should immediately focus the PIN')
      expect(await touch.getByRole('group', { name: 'Selected profile', exact: true }).getByRole('button').count() === 1, 'touch sign-in should show only the selected profile')
      await touch.evaluate(() => new Promise((resolve) => {
        const start = performance.now()
        const resize = () => {
          const progress = Math.min(1, (performance.now() - start) / 180)
          window.setKeyboardViewport(window.innerHeight - (window.innerHeight - 430) * progress)
          if (progress < 1) requestAnimationFrame(resize)
          else resolve()
        }
        requestAnimationFrame(resize)
      }))
      await check(touch, `${width} touch profile expansion`)
      const motion = await touch.evaluate(() => window.profileMotion)
      const fading = motion.filter((frame) => !frame.otherHidden && frame.otherOpacity > 0.02 && frame.otherOpacity < 0.98)
      expect(fading.length > 0, 'other profiles should visibly fade rather than disappear at once')
      expect(fading.every((frame) => frame.panelHeight < 1 && Math.abs(frame.x - original.x) < 2 && Math.abs(frame.y - original.y) < 2), 'fade should precede both profile movement and form expansion')
      expect(motion.some((frame) => frame.otherHidden && frame.panelHeight < 1 && Math.hypot(frame.x - original.x, frame.y - original.y) > 2), 'selected profile should slide after the others fade')
      const expanding = motion.filter((frame) => frame.panelHeight > 1)
      expect(expanding.length > 2, 'form should expand over several frames')
      expect(expanding.every((frame) => frame.otherHidden && Math.abs(frame.x + frame.width / 2 - width / 2) < 2), 'form should open only after the selected profile reaches the centre')
      expect(expanding.every((frame, index) => index === 0 || frame.panelHeight >= expanding[index - 1].panelHeight - 1), 'form should expand once without collapsing and expanding again')
      expect(motion.every((frame) => Math.abs(frame.logoTop - motion[0].logoTop) < 2), 'keyboard and expansion should not recenter the page layout')
      const seconds = await touch.locator('#sign-in-panel').evaluate((panel) => Number.parseFloat(getComputedStyle(panel).transitionDuration))
      expect(seconds >= 0.4 && seconds <= 0.5, 'expansion should have a deliberate pace')
      for (const [height, offset] of [[430, 0], [360, 40]]) {
        await touch.evaluate(([value, top]) => window.setKeyboardViewport(value, top), [height, offset])
        await touch.waitForFunction(([value, top]) => {
          const input = document.querySelector('input[name="password"]').getBoundingClientRect()
          const button = document.querySelector('button[type="submit"]').getBoundingClientRect()
          return input.top >= top + 12 && button.bottom <= top + value - 12
        }, [height, offset])
        expect(await touch.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), 'keyboard layout should not scroll horizontally')
      }
      await pin.fill('123456')
      await touch.getByRole('button', { name: 'Sign in', exact: true }).tap()
      await touch.getByRole('alert').waitFor()
      await touch.waitForFunction(() => document.querySelector('button[type="submit"]').getBoundingClientRect().bottom <= window.visualViewport.offsetTop + window.visualViewport.height - 12)
      await check(touch, `${width} keyboard and credential error`)

      await touch.evaluate(() => {
        document.activeElement.blur()
        window.setKeyboardViewport(null)
      })
      await touch.waitForFunction(() => document.querySelector('main').style.getPropertyValue('--sign-in-keyboard-inset') === '0px')
      await touch.getByRole('button', { name: 'Sign in another way', exact: true }).tap()
      const username = touch.getByLabel('Username', { exact: true })
      expect(await touch.evaluate(() => window.manualTapFocused === true), 'manual sign-in should focus username before the trusted event ends')
      expect(await username.evaluate((input) => input === document.activeElement), 'touch fallback should immediately focus username')
      await touch.evaluate(() => window.setKeyboardViewport(240))
      await touch.waitForFunction(() => {
        const input = document.querySelector('input[name="username"]').getBoundingClientRect()
        return input.top >= 12 && input.bottom <= window.visualViewport.height - 12
      })
      // A short screen may need scrolling between two fields and the button.
      await touch.getByLabel('PIN or password', { exact: true }).tap()
      await touch.waitForFunction(() => {
        const input = document.querySelector('input[name="password"]').getBoundingClientRect()
        const button = document.querySelector('button[type="submit"]').getBoundingClientRect()
        return input.top >= 12 && button.bottom <= window.visualViewport.height - 12
      })
      await touch.evaluate(() => window.setKeyboardViewport(300, 0, 1.5))
      await touch.waitForFunction(() => document.querySelector('main').style.getPropertyValue('--sign-in-keyboard-inset') === '0px')
    }
    await touch.emulateMedia({ reducedMotion: 'reduce' })
    await touch.goto(`${base}/login`, { waitUntil: 'networkidle' })
    await touch.getByRole('button', { name: 'Mira', exact: true }).tap()
    expect(await touch.getByLabel('Password', { exact: true }).evaluate((input) => input === document.activeElement), 'reduced-motion touch selection should focus the password')
    expect(await touch.evaluate(() => window.profileTapFocused === true), 'reduced motion should preserve focus inside the tap event')
    await check(touch, 'reduced-motion touch picker')

    await touch.emulateMedia({ reducedMotion: 'no-preference' })
    await touch.goto(`${base}/login`, { waitUntil: 'networkidle' })
    await touch.getByRole('button', { name: 'Nora', exact: true }).tap()
    // Interrupt the fade before later phases can run; a stale animation must
    // not reopen the selected profile over the username form.
    await touch.getByRole('button', { name: 'Sign in another way', exact: true }).evaluate((button) => button.click())
    await check(touch, 'interrupted selection username fallback')
    expect(await touch.getByRole('group', { name: 'Selected profile', exact: true }).count() === 0, 'interrupted selection should keep the profile chooser hidden')
    expect(await touch.getByLabel('Username', { exact: true }).isVisible(), 'interrupted selection should leave the username form open')
    expect(await touch.getByLabel('PIN or password', { exact: true }).isVisible(), 'interrupted selection should retain manual credential mode')
  } finally {
    await touchContext.close()
  }

  for (const status of [200, 503]) {
    users = []
    sourceStatus = status
    await page.goto(`${base}/login`, { waitUntil: 'networkidle' })
    await page.getByLabel('Username', { exact: true }).waitFor()
    expect(await page.getByRole('button', { name: 'Sign in', exact: true }).isVisible(), 'empty or unavailable picker should offer manual sign-in')
    await check(page, `profile source ${status} fallback`)
  }
}

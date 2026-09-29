function expect(condition, message) {
  if (!condition) {
    throw new Error(message)
  }
}

// An expired or revoked session must invalidate the frontend auth state from
// any authenticated API call, not page by page.
export default async function sessionExpiry(page, { base }) {
  // 1. The real session authenticates and protected UI renders. /library is
  // used because Home may still route a fresh admin into onboarding.
  await page.goto(`${base}/library`, { waitUntil: 'networkidle' })
  await page.waitForSelector('a[href="/library"]', { timeout: 8000 })
  expect(
    (await page.locator('a[href="/library"]').count()) > 0,
    'protected navigation must be visible while authenticated',
  )

  // A failed sign-out must not pretend the session was destroyed.
  await page.route('**/api/auth/logout', (route) =>
    route.fulfill({
      status: 500,
      contentType: 'application/json',
      body: JSON.stringify({ code: 'internal_error', message: 'Logout is down' }),
    }),
  )
  await page.getByRole('button', { name: 'Account menu' }).click()
  await page.getByRole('button', { name: 'Sign out' }).click()
  await page.getByRole('alert').waitFor({ timeout: 8000 })
  expect(
    (await page.locator('a[href="/library"]').count()) > 0,
    'a failed sign-out must keep the current profile',
  )
  await page.keyboard.press('Escape')
  await page.unroute('**/api/auth/logout')

  // 2. A later authenticated call returns 401 (session revoked/expired).
  await page.route('**/api/books**', (route) =>
    route.fulfill({
      status: 401,
      contentType: 'application/json',
      body: JSON.stringify({ code: 'unauthorized', message: 'Session expired' }),
    }),
  )
  await page.goto(`${base}/library`, { waitUntil: 'domcontentloaded' })

  // 3-5. Auth state is invalidated and protected UI is gone.
  await page.waitForURL(`${base}/login`, { timeout: 8000 })
  expect(
    (await page.locator('a[href="/library"]').count()) === 0,
    'protected navigation must disappear after the session expires',
  )
  await page.getByRole('button', { name: /sign in/i }).first().waitFor({ timeout: 8000 })
}

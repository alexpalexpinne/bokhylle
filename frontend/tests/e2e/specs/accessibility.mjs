import axe from 'axe-core'

const tags = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa', 'best-practice']

export async function check(page, name, selector) {
  const signInPanel = page.locator('#sign-in-panel')
  if (await signInPanel.count()) {
    await page.waitForFunction(() => document.querySelector('#sign-in-panel')?.getAttribute('aria-busy') !== 'true')
    // A fading panel has intentionally transient colors; inspect its settled state.
    await signInPanel.evaluate((panel) => Promise.all(panel.getAnimations().map((animation) => animation.finished.catch(() => {}))))
  }
  // The app's CSP blocks inline script tags. Playwright evaluation runs axe
  // in the page's context without weakening that policy.
  await page.evaluate(axe.source)
  const violations = await page.evaluate(async ({ runOnly, selector }) => {
    const results = await window.axe.run(selector ? document.querySelector(selector) : document, { runOnly })
    return results.violations.map((violation) => ({
      rule: violation.id,
      impact: violation.impact,
      elements: violation.nodes.map((node) => node.target),
    }))
  }, { runOnly: { type: 'tag', values: tags }, selector })

  if (violations.length > 0) {
    throw new Error(`${name}: ${JSON.stringify(violations)}`)
  }
}

export async function accessibilityLogin(page, { base }) {
  await page.goto(`${base}/login`, { waitUntil: 'networkidle' })
  await check(page, 'login')
  const profile = page.locator('button[data-profile-username]').first()
  if (await profile.isVisible().catch(() => false)) {
    await profile.click()
    await check(page, 'login password panel')
    await page.getByRole('button', { name: 'Sign in another way', exact: true }).click()
    await check(page, 'login username fallback')
  }
}

export async function accessibilityApp(page, { base }) {
  for (const path of [
    '/',
    '/library',
    '/discover',
    '/activity',
    '/profile',
    '/settings',
    '/settings/household',
    '/settings/library',
    '/settings/getting-books',
    '/settings/getting-books/prowlarr',
    '/settings/metadata',
    '/settings/delivery',
    '/settings/server',
  ]) {
    await page.goto(`${base}${path}`, { waitUntil: 'networkidle' })
    await check(page, path)
  }
}

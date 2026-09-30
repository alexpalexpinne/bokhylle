import axe from 'axe-core'

const tags = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa', 'best-practice']

export async function check(page, name, selector) {
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

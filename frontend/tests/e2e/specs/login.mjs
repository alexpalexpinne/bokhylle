import { expect } from '../support.mjs'

/** The login page carries the bookshelf identity and offers accounts or a form. */
export default async function login(page, { base }) {
  await page.goto(`${base}/login`, { waitUntil: 'networkidle' })
  await page.getByRole('heading', { name: 'Come in and browse', exact: true }).waitFor()
  expect(await page.locator('svg').first().isVisible(), 'bookshelf mark should render')
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 1000 })
    const mark = await page.locator('svg').first().boundingBox()
    const wordmark = await page.locator('[style*="bokhylle-wordmark"]').boundingBox()
    expect(wordmark.y > mark.y + mark.height, 'brand name must be below the bookshelf mark')
    expect(Math.abs(mark.x + mark.width / 2 - width / 2) < 2, 'sign-in logo must be centered')
    expect(Math.abs(wordmark.x + wordmark.width / 2 - width / 2) < 2, 'brand name must be centered')
  }
  expect(
    await page.getByText('Private library', { exact: true }).isVisible(),
    'the sign-in screen should identify the private library',
  )
  const hasForm = await page.locator('input[autocomplete="username"]').isVisible().catch(() => false)
  const hasPicker = (await page.getByRole('button', { name: /reading|administrator|member/i }).count()) > 0
  expect(hasForm || hasPicker, 'either the account picker or the username form should render')
}

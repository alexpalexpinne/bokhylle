// Capture website previews from a locally running public demo and its sample books.
// Start the demo first; set BOKHYLLE_DEMO_BASE if it is not on port 8081.
import { existsSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chromium } from 'playwright-core'

const base = process.env.BOKHYLLE_DEMO_BASE ?? 'http://127.0.0.1:8081'
const output = fileURLToPath(new URL('../../website/assets/', import.meta.url))
const chrome = [
  process.env.PLAYWRIGHT_CHROME_PATH,
  '/usr/bin/google-chrome',
  '/usr/bin/google-chrome-stable',
].filter(Boolean).find(existsSync)

if (!chrome) throw new Error('Chrome is required to capture website screenshots')

const browser = await chromium.launch({
  executablePath: chrome,
  args: ['--no-sandbox', '--disable-dev-shm-usage'],
})
try {
  const page = await browser.newPage({
    viewport: { width: 1440, height: 720 },
    deviceScaleFactor: 1,
    colorScheme: 'light',
  })
  await page.goto(base, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: 'Enter as adult' }).click()
  await page.getByRole('link', { name: 'Library', exact: true }).waitFor()
  await page.goto(`${base}/library?scope=household`, { waitUntil: 'networkidle' })
  await page.getByRole('heading', { name: 'Household library' }).waitFor()
  await page.locator('[data-book-cover] img').first().waitFor()
  await page.locator('[data-book-cover] img').first().evaluate(async (image) => {
    await image.decode()
    if (!image.naturalWidth) throw new Error('Demo book cover did not load')
  })
  await page.screenshot({ path: join(output, 'demo-library.png') })

  await page.setViewportSize({ width: 390, height: 844 })
  await page.reload({ waitUntil: 'networkidle' })
  await page.getByRole('heading', { name: 'Household library' }).waitFor()
  await page.screenshot({ path: join(output, 'demo-library-mobile.png') })
  await page.close()
} finally {
  await browser.close()
}

// Shared README and website captures from an isolated demo, never a household.
import { existsSync, copyFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chromium } from 'playwright-core'

const root = fileURLToPath(new URL('../../', import.meta.url))

export async function captureDemo(views = ['home', 'library', 'reader']) {
  const base = process.env.BOKHYLLE_DEMO_BASE ?? 'http://127.0.0.1:8081'
  const chrome = [process.env.PLAYWRIGHT_CHROME_PATH, '/usr/bin/google-chrome', '/usr/bin/google-chrome-stable']
    .filter(Boolean).find(existsSync)
  if (!chrome) throw new Error('Chrome is required to capture demo screenshots')

  const browser = await chromium.launch({ executablePath: chrome, args: ['--no-sandbox', '--disable-dev-shm-usage'] })
  try {
    const context = await browser.newContext({ deviceScaleFactor: 1, colorScheme: 'light', reducedMotion: 'reduce' })
    // Refuse ordinary installations before creating a visitor or saving progress.
    const status = await context.request.get(`${base}/api/demo`)
    if (!status.ok() || (await status.json()).enabled !== true) throw new Error('Screenshots require an isolated Bokhylle demo')
    await context.addInitScript(() => localStorage.setItem('bokhylle.theme', 'paper'))
    const page = await context.newPage()
    await page.setViewportSize({ width: 1440, height: 1000 })
    await page.goto(base, { waitUntil: 'networkidle' })
    await page.getByRole('button', { name: 'Enter as adult', exact: true }).click()
    await page.getByRole('link', { name: 'Library', exact: true }).waitFor()

    async function save(view, mobile = false) {
      await page.evaluate(async () => {
        await document.fonts.ready
        for (const image of document.images) image.loading = 'eager'
      })
      await page.waitForFunction(() => Array.from(document.images).every((image) => image.complete && image.naturalWidth > 0))
      const filename = `demo-${view}${mobile ? '-mobile' : ''}.png`
      const destination = join(root, 'website/assets', filename)
      await page.mouse.move(0, 0)
      await page.screenshot({ path: destination, animations: 'disabled', caret: 'hide' })
      copyFileSync(destination, join(root, 'docs/media', `${view}-${mobile ? 'mobile' : 'desktop'}.png`))
      console.log(`Captured ${filename} and its README preview`)
    }

    if (views.includes('home')) {
      await page.getByRole('region', { name: 'Picked for you books', exact: true }).waitFor()
      await page.getByRole('region', { name: 'Books from authors you follow', exact: true }).waitFor()
      await page.getByRole('region', { name: 'Based on books you liked books', exact: true }).waitFor()
      await save('home')
      await page.setViewportSize({ width: 390, height: 1000 })
      await save('home', true)
    }

    if (views.includes('library')) {
      await page.setViewportSize({ width: 1440, height: 1000 })
      await page.goto(`${base}/library?scope=household`, { waitUntil: 'networkidle' })
      await page.getByRole('heading', { name: 'Household library', exact: true }).waitFor()
      await page.locator('[data-book-cover] img').first().waitFor()
      await save('library')
      await page.setViewportSize({ width: 390, height: 1000 })
      await save('library', true)
    }

    if (views.includes('reader')) {
      const catalogueResponse = await context.request.get(`${base}/api/books?scope=household&pageSize=100`)
      if (!catalogueResponse.ok()) throw new Error('Could not load the demo catalogue')
      const catalogue = await catalogueResponse.json()
      const sample = catalogue.items.find((book) => book.title === 'Dracula' && book.authors.includes('Bram Stoker'))
      if (!sample) throw new Error('The prepared demo must include Dracula by Bram Stoker')
      const detailResponse = await context.request.get(`${base}/api/books/${sample.id}`)
      if (!detailResponse.ok()) throw new Error('Could not load Dracula')
      const detail = await detailResponse.json()
      const file = detail.files.find((file) => file.format === 'epub')
      if (!file) throw new Error('The demo reader requires the prepared Dracula EPUB')

      // Separate tabs settle without another reader reporting a newer position.
      await page.close()
      for (const mobile of [false, true]) {
        const reader = await context.newPage()
        await reader.setViewportSize({ width: mobile ? 390 : 1440, height: mobile ? 1000 : 900 })
        await reader.goto(`${base}/read/${sample.id}/${file.id}`, { waitUntil: 'networkidle' })
        await reader.getByRole('button', { name: 'Contents', exact: true }).click()
        await reader.getByRole('navigation', { name: 'Book contents' }).getByRole('button', { name: 'I', exact: true }).click()
        const content = reader.frameLocator('main[aria-label="EPUB reading area"] iframe').first()
        await content.getByText('Jonathan Harker’s Journal', { exact: false }).waitFor()
        await reader.getByText('Turning page…', { exact: true }).waitFor({ state: 'hidden' })
        await reader.getByRole('button', { name: 'Display settings', exact: true }).click()
        await reader.getByRole('button', { name: 'Paper', exact: true }).click()
        await reader.getByText('Adjusting page…', { exact: true }).waitFor({ state: 'hidden' })
        if (mobile) {
          await reader.getByRole('button', { name: 'Display settings', exact: true }).click()
          await reader.locator('aside[aria-label="display"]').waitFor({ state: 'hidden' })
        }
        await reader.evaluate(() => document.fonts.ready)
        await content.locator('body').evaluate(async (body) => {
          await body.ownerDocument.fonts.ready
          if (getComputedStyle(body).backgroundColor !== 'rgb(247, 244, 238)') throw new Error('The reader must use Paper')
        })
        const destination = join(root, 'website/assets', `demo-reader${mobile ? '-mobile' : ''}.png`)
        await reader.mouse.move(0, 0)
        await reader.screenshot({ path: destination, animations: 'disabled', caret: 'hide' })
        copyFileSync(destination, join(root, 'docs/media', `reader-${mobile ? 'mobile' : 'desktop'}.png`))
        console.log(`Captured the ${mobile ? 'mobile' : 'desktop'} demo reader in Paper`)
        await reader.close()
      }
    }
  } finally {
    await browser.close()
  }
}

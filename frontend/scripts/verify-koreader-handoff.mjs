// Browser regression check for passive EPUB resume from a KOReader XPointer.
// Start `pnpm -C frontend preview --host 127.0.0.1 --port 4175` first.
import assert from 'node:assert/strict'
import { existsSync } from 'node:fs'
import { chromium } from 'playwright-core'
import JSZip from 'jszip'

const base = process.env.BOKHYLLE_READER_BASE ?? 'http://127.0.0.1:4175'
const chrome = [process.env.PLAYWRIGHT_CHROME_PATH, '/usr/bin/google-chrome', '/usr/bin/google-chrome-stable']
  .filter(Boolean).find(existsSync)
if (!chrome) throw new Error('Chrome is required for the KOReader handoff smoke test')

const zip = new JSZip()
zip.file('mimetype', 'application/epub+zip', { compression: 'STORE' })
zip.file('META-INF/container.xml', `<?xml version="1.0"?>
  <container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
    <rootfiles><rootfile full-path="OEBPS/package.opf" media-type="application/oebps-package+xml"/></rootfiles>
  </container>`)
zip.file('OEBPS/package.opf', `<?xml version="1.0"?>
  <package xmlns="http://www.idpf.org/2007/opf" unique-identifier="id" version="3.0">
    <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
      <dc:identifier id="id">urn:uuid:fictional-koreader-handoff</dc:identifier>
      <dc:title>The Paper Lighthouse</dc:title><dc:language>en</dc:language>
      <meta property="dcterms:modified">2026-01-01T00:00:00Z</meta>
    </metadata>
    <manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest>
    <spine><itemref idref="chapter"/></spine>
  </package>`)
const paragraph = 'The paper lighthouse stood beyond the narrow harbor. Nora followed the chart until the beacon came into view. '.repeat(5)
zip.file('OEBPS/chapter.xhtml', `<html xmlns="http://www.w3.org/1999/xhtml"><head><title>The Beacon</title></head><body>
  <h1>The Beacon</h1>${Array.from({ length: 20 }, (_, index) => `<p>Beacon ${String(index + 1).padStart(2, '0')}. ${paragraph}</p>`).join('')}
  </body></html>`)
const epub = await zip.generateAsync({ type: 'nodebuffer', compression: 'DEFLATE' })
const xpointer = '/body/DocFragment[1]/body/p[3]/text().0'
const koreaderPosition = { locator: xpointer, percentage: 0.15, revision: 1, updatedAt: 1790510400, source: 'koreader' }
let browserPosition = null
let externalPosition = koreaderPosition
const writes = []
const browser = await chromium.launch({ executablePath: chrome, args: ['--no-sandbox', '--disable-dev-shm-usage'] })
try {
  const page = await browser.newPage({ viewport: { width: 1100, height: 800 } })
  await page.addInitScript(() => localStorage.setItem('bokhylle-reader-font-1', '150'))
  await page.route('**/api/**', (route) => {
    const { pathname } = new URL(route.request().url())
    if (pathname.endsWith('/content')) return route.fulfill({ contentType: 'application/epub+zip', body: epub })
    if (pathname.endsWith('/position')) {
      if (route.request().method() === 'PUT') writes.push(route.request().postDataJSON())
      return route.fulfill({ json: {
        sha256: 'fictional-epub-sha', format: 'epub', position: browserPosition,
        external: externalPosition,
        direction: { bookDirection: null, seriesDirection: null, directionOverride: null },
      } })
    }
    if (pathname === '/api/auth/me') return route.fulfill({ json: { user: {
      id: 1, username: 'mira', displayName: 'Mira', role: 'user', profileType: 'adult',
      preferredFormat: 'epub', preferredLanguage: 'en', preferredLanguages: ['en'],
      defaultLanguage: 'en', acquisitionMode: 'automatic', canAcquire: true,
    } } })
    if (pathname === '/api/books/1') return route.fulfill({ json: {
      id: 1, title: 'The Paper Lighthouse', authors: ['Nora Vale'],
      files: [{ id: 1, format: 'epub', size: epub.length }],
    } })
    return route.fulfill({ json: {} })
  })

  await page.goto(`${base}/read/1/1`)
  const next = page.getByRole('button', { name: 'Next page' })
  await next.waitFor()
  await page.waitForFunction(() => !document.querySelector('button[aria-label="Next page"]')?.disabled)
  await page.waitForTimeout(2200)
  assert.equal(writes.length, 0, 'opening a KOReader location must not write a browser page start')
  assert.doesNotMatch(await page.locator('body').innerText(), /saved position could not be opened/i)

  await page.getByRole('button', { name: 'Display settings' }).click()
  assert.equal(await page.getByRole('group', { name: 'Reading colors' }).count(), 1)
  assert.equal(await page.locator('#reader-font').inputValue(), '150', 'the old text size is read into the new preference model')
  assert.ok(await page.frameLocator('main iframe').locator('body').evaluate((body) => parseFloat(getComputedStyle(body).fontSize)) >= 24, 'saved text size applies before first display')
  await page.getByRole('button', { name: 'Warm', exact: true }).click()
  await page.getByLabel('Font', { exact: true }).selectOption('sans')
  await page.locator('#reader-font').focus()
  await page.locator('#reader-font').press('End')
  await page.getByLabel('Line spacing').selectOption('spacious')
  await page.waitForFunction(() => {
    const value = JSON.parse(localStorage.getItem('bokhylle.readerAppearance:1') ?? '{}')
    return value.theme === 'warm' && value.fontFamily === 'sans' && value.textScale === 200 && value.lineSpacing === 'spacious'
  })
  await page.getByText('Adjusting page…').waitFor({ state: 'hidden' })
  await page.waitForTimeout(2200)
  assert.equal(writes.length, 0, 'appearance changes must not overwrite KOReader progress')
  const warmBackground = await page.frameLocator('main iframe').locator('body').evaluate((body) => getComputedStyle(body).backgroundColor)
  assert.equal(warmBackground, 'rgb(239, 226, 200)', 'warm background reaches EPUB content')
  const typeStyle = await page.frameLocator('main iframe').locator('body').evaluate((body) => {
    const style = getComputedStyle(body)
    return { family: style.fontFamily, size: parseFloat(style.fontSize), height: parseFloat(style.lineHeight) }
  })
  assert.match(typeStyle.family, /system-ui/, 'sans serif reaches EPUB content')
  assert.ok(typeStyle.size >= 30, '200% text size reaches EPUB content')
  assert.ok(Math.abs(typeStyle.height / typeStyle.size - 1.8) < 0.05, 'spacious line height reaches EPUB content')
  await page.setViewportSize({ width: 390, height: 844 })
  assert.ok(await page.locator('.bokhylle-reader').evaluate((root) => root.scrollWidth <= window.innerWidth + 1), 'the 200% display panel fits a phone viewport')
  await page.setViewportSize({ width: 1100, height: 800 })

  await page.getByRole('button', { name: 'Reset reading appearance' }).click()
  await page.waitForTimeout(2200)
  assert.equal(writes.length, 0, 'reset must not overwrite KOReader progress')
  assert.deepEqual(await page.evaluate(() => JSON.parse(localStorage.getItem('bokhylle.readerAppearance:1'))), {
    theme: 'app', fontFamily: 'serif', textScale: 100, lineSpacing: 'standard',
  })
  await page.evaluate(() => {
    localStorage.setItem('bokhylle.theme', 'paper')
    document.documentElement.dataset.theme = 'paper'
  })
  await page.emulateMedia({ colorScheme: 'dark' })
  assert.equal(await page.locator('.bokhylle-reader').evaluate((root) => getComputedStyle(root).backgroundColor), 'rgb(247, 244, 238)', 'an explicit Paper app theme wins over system dark')
  await page.evaluate(() => {
    localStorage.setItem('bokhylle.theme', 'ink')
    document.documentElement.dataset.theme = 'ink'
  })
  await page.waitForFunction(() => getComputedStyle(document.querySelector('.bokhylle-reader')).backgroundColor === 'rgb(26, 21, 17)')
  await page.getByText('Adjusting page…').waitFor({ state: 'hidden' })
  assert.equal(await page.frameLocator('main iframe').locator('body').evaluate((body) => getComputedStyle(body).backgroundColor), 'rgb(26, 21, 17)', 'Follow app uses resolved Ink')
  assert.equal(writes.length, 0, 'following an app theme change must not write progress')

  await page.reload()
  await next.waitFor()
  await page.waitForTimeout(2200)
  assert.equal(writes.length, 0, 'reloading a KOReader location must not replace it')

  await next.click()
  await page.waitForTimeout(2200)
  assert.ok(writes.length > 0, 'turning a page should save the browser position')
  assert.ok(writes[0].externalLocator?.startsWith('/body/DocFragment[1]/'), 'page turn should send an EPUB XPointer')
  const turnedLocator = writes[0].externalLocator

  const countAfterTurn = writes.length
  await page.getByRole('button', { name: 'Display settings' }).click()
  await page.waitForTimeout(2200)
  assert.equal(writes.length, countAfterTurn, 'opening display settings must not save a reflowed page')
  await page.getByRole('button', { name: 'Ink', exact: true }).click()
  await page.getByText('Adjusting page…').waitFor({ state: 'hidden' })
  await page.waitForTimeout(2200)
  assert.equal(writes.length, countAfterTurn, 'restyling a saved browser passage must not write progress')
  await page.getByRole('button', { name: 'Display settings' }).click()
  await next.click()
  await page.getByRole('button', { name: 'Display settings' }).click()
  await page.getByRole('button', { name: 'Warm', exact: true }).click()
  await page.getByText('Adjusting page…').waitFor({ state: 'hidden' })
  await page.waitForTimeout(2200)
  assert.equal(writes.length, countAfterTurn + 1, 'an immediate appearance change must retain the preceding page turn')

  browserPosition = { locator: writes[0].locator, percentage: writes[0].percentage,
    completed: false, revision: 2, updatedAt: 1790510500 }
  externalPosition = null
  writes.length = 0
  await page.reload()
  await next.waitFor()
  await page.getByRole('button', { name: 'Display settings' }).click()
  assert.equal(await page.getByRole('button', { name: 'Warm', exact: true }).getAttribute('aria-pressed'), 'true', 'appearance is restored from this browser')
  await page.getByRole('button', { name: 'Display settings' }).click()
  await page.waitForTimeout(2200)
  assert.equal(writes.length, 0, 'opening a browser CFI must not rewrite its precise saved location')

  browserPosition = null
  externalPosition = koreaderPosition
  writes.length = 0
  await page.reload()
  await next.waitFor()
  await page.waitForTimeout(2200)
  assert.equal(writes.length, 0)
  await next.click()
  await page.close()
  await new Promise((resolve) => setTimeout(resolve, 300))
  assert.ok(writes.every((update) => update.externalLocator === turnedLocator),
    'closing immediately after a page turn must not save the pre-navigation page')
  console.log(`KOReader EPUB handoff browser smoke test passed (quick-close writes: ${writes.length})`)
} finally {
  await browser.close()
}

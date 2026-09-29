// Browser smoke test for direction and format-aware display controls.
// Start `pnpm -C frontend preview --host 127.0.0.1 --port 4175` first.
import assert from 'node:assert/strict'
import { existsSync } from 'node:fs'
import { chromium } from 'playwright-core'
import JSZip from 'jszip'

const base = process.env.BOKHYLLE_READER_BASE ?? 'http://127.0.0.1:4175'
const chrome = [process.env.PLAYWRIGHT_CHROME_PATH, '/usr/bin/google-chrome', '/usr/bin/google-chrome-stable']
  .filter(Boolean).find(existsSync)
if (!chrome) throw new Error('Chrome is required for the reader direction smoke test')

const pixel = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScL/nwAAAABJRU5ErkJggg==', 'base64')
function fictionalPdf() {
  const stream = 'BT /F1 18 Tf 50 350 Td (The Lantern Atlas) Tj ET\n'
  const objects = [
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 400] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>',
    '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
    `<< /Length ${Buffer.byteLength(stream)} >>\nstream\n${stream}endstream`,
  ]
  let content = '%PDF-1.4\n'
  const offsets = [0]
  for (const [index, object] of objects.entries()) {
    offsets.push(Buffer.byteLength(content))
    content += `${index + 1} 0 obj\n${object}\nendobj\n`
  }
  const xref = Buffer.byteLength(content)
  content += `xref\n0 ${offsets.length}\n0000000000 65535 f \n`
  for (const offset of offsets.slice(1)) content += `${String(offset).padStart(10, '0')} 00000 n \n`
  content += `trailer\n<< /Size ${offsets.length} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`
  return Buffer.from(content)
}
let override = null
let position = null
const browser = await chromium.launch({ executablePath: chrome, args: ['--no-sandbox', '--disable-dev-shm-usage'] })
try {
  const page = await browser.newPage({ viewport: { width: 1100, height: 800 } })
  await page.route('**/api/**', (route) => {
    const { pathname } = new URL(route.request().url())
    if (/\/pages\/[1-4]$/.test(pathname)) return route.fulfill({ contentType: 'image/png', body: pixel })
    if (pathname.endsWith('/pages')) return route.fulfill({ json: { pages: 4 } })
    if (pathname.endsWith('/direction')) {
      override = route.request().postDataJSON().direction
      return route.fulfill({ json: { bookDirection: 'ltr', directionOverride: override } })
    }
    if (pathname.endsWith('/position')) {
      if (route.request().method() === 'PUT') {
        const update = route.request().postDataJSON()
        position = { locator: update.locator, percentage: update.percentage,
          completed: update.completed, revision: (position?.revision ?? 0) + 1, updatedAt: 1790510400 }
      }
      return route.fulfill({ json: { sha256: 'fictional-cbz-sha', format: 'cbz', position,
        external: null, direction: { bookDirection: 'ltr', directionOverride: override } } })
    }
    const response = pathname === '/api/auth/me' ? { user: {
      id: 1, username: 'mira', displayName: 'Mira', role: 'user', profileType: 'adult',
      preferredFormat: 'cbz', preferredLanguage: 'en', preferredLanguages: ['en'],
      defaultLanguage: 'en', acquisitionMode: 'automatic', notificationEmail: null,
      emailNotifications: false, canAcquire: true,
    } } : pathname === '/api/demo' ? { enabled: false }
      : pathname === '/api/books/1' ? { id: 1, title: 'The Lantern Atlas', authors: ['Nora Vale'],
        files: [{ id: 1, format: 'cbz', size: 1024 }] } : {}
    return route.fulfill({ json: response })
  })

  await page.goto(`${base}/read/1/1`)
  const pageInput = page.getByRole('spinbutton', { name: 'Page number' })
  await pageInput.waitFor()
  assert.equal(await pageInput.inputValue(), '1')
  await page.getByRole('button', { name: 'Display settings' }).click()
  assert.equal(await page.getByRole('group', { name: 'Reader surface' }).count(), 1)
  assert.equal(await page.getByLabel('Font', { exact: true }).count(), 0)
  assert.equal(await page.getByLabel('Line spacing').count(), 0)
  assert.equal(await page.locator('#reader-font').count(), 0)
  await page.getByRole('button', { name: 'Warm', exact: true }).click()
  assert.equal(await page.locator('.bokhylle-reader').evaluate((element) => getComputedStyle(element).backgroundColor), 'rgb(239, 226, 200)')
  await page.getByLabel('Reading direction').selectOption('rtl')
  await page.getByRole('button', { name: 'Next page' }).waitFor()
  assert.equal(override, 'rtl')
  await page.getByRole('button', { name: 'Display settings' }).click()
  const controls = page.locator('footer button[aria-label]')
  assert.equal(await controls.first().getAttribute('aria-label'), 'Next page')
  await controls.first().click()
  assert.equal(await pageInput.inputValue(), '2')
  await page.evaluate(() => document.activeElement?.blur())
  await page.keyboard.press('ArrowLeft')
  assert.equal(await pageInput.inputValue(), '3')
  await page.keyboard.press('ArrowRight')
  assert.equal(await pageInput.inputValue(), '2')
  await page.evaluate(() => {
    const main = document.querySelector('main')
    const start = new Touch({ identifier: 1, target: main, clientX: 100, clientY: 300 })
    const end = new Touch({ identifier: 1, target: main, clientX: 220, clientY: 300 })
    main.dispatchEvent(new TouchEvent('touchstart', { touches: [start], changedTouches: [start], bubbles: true }))
    main.dispatchEvent(new TouchEvent('touchend', { touches: [], changedTouches: [end], bubbles: true }))
  })
  await page.waitForFunction(() => document.querySelector('input[name="page"]')?.value === '3')
  await page.reload()
  await pageInput.waitFor()
  assert.equal(await controls.first().getAttribute('aria-label'), 'Next page')
  await page.getByRole('button', { name: 'Display settings' }).click()
  await page.getByLabel('Double spread').check()
  const images = page.locator('main img')
  const first = await images.nth(0).boundingBox()
  const second = await images.nth(1).boundingBox()
  assert.ok(first && second && first.x > second.x, 'first CBZ page belongs on the right in RTL spread')
  await page.getByRole('button', { name: 'Page thumbnails' }).click()
  const pageOne = await page.getByRole('button', { name: 'Go to page 1' }).boundingBox()
  const pageTwo = await page.getByRole('button', { name: 'Go to page 2' }).boundingBox()
  assert.ok(pageOne && pageTwo && pageOne.x > pageTwo.x, 'first thumbnail belongs on the right')

  // The EPUB spine requests RTL. With no profile, book, or series setting,
  // the shell and Epub.js must adopt it together. A profile choice then wins.
  const zip = new JSZip()
  zip.file('mimetype', 'application/epub+zip', { compression: 'STORE' })
  zip.file('META-INF/container.xml', `<?xml version="1.0"?>
    <container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
      <rootfiles><rootfile full-path="OEBPS/package.opf" media-type="application/oebps-package+xml"/></rootfiles>
    </container>`)
  zip.file('OEBPS/package.opf', `<?xml version="1.0"?>
    <package xmlns="http://www.idpf.org/2007/opf" unique-identifier="id" version="3.0">
      <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
        <dc:identifier id="id">urn:uuid:fictional-rtl-reader</dc:identifier>
        <dc:title>Fictional RTL Story</dc:title><dc:language>en</dc:language>
        <meta property="dcterms:modified">2026-01-01T00:00:00Z</meta>
      </metadata>
      <manifest>
        <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
        <item id="first" href="first.xhtml" media-type="application/xhtml+xml"/>
        <item id="second" href="second.xhtml" media-type="application/xhtml+xml"/>
      </manifest>
      <spine page-progression-direction="rtl"><itemref idref="first"/><itemref idref="second"/></spine>
    </package>`)
  zip.file('OEBPS/nav.xhtml', `<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Contents</title></head><body><nav xmlns:epub="http://www.idpf.org/2007/ops" epub:type="toc"><ol><li><a href="first.xhtml">First</a></li><li><a href="second.xhtml">Second</a></li></ol></nav></body></html>`)
  zip.file('OEBPS/first.xhtml', `<html xmlns="http://www.w3.org/1999/xhtml"><head><title>First</title></head><body><h1>First fictional chapter</h1></body></html>`)
  zip.file('OEBPS/second.xhtml', `<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Second</title></head><body><h1>Second fictional chapter</h1></body></html>`)
  const epub = await zip.generateAsync({ type: 'nodebuffer', compression: 'DEFLATE' })
  const epubPage = await browser.newPage({ viewport: { width: 1100, height: 800 } })
  let epubOverride = null
  await epubPage.route('**/api/**', (route) => {
    const { pathname } = new URL(route.request().url())
    if (pathname.endsWith('/content')) return route.fulfill({ contentType: 'application/epub+zip', body: epub })
    if (pathname.endsWith('/direction')) {
      epubOverride = route.request().postDataJSON().direction
      return route.fulfill({ json: { bookDirection: null, seriesDirection: null, directionOverride: epubOverride } })
    }
    if (pathname.endsWith('/position')) return route.fulfill({ json: {
      sha256: 'fictional-epub-sha', format: 'epub', position: null, external: null,
      direction: { bookDirection: null, seriesDirection: null, directionOverride: epubOverride },
    } })
    if (pathname === '/api/auth/me') return route.fulfill({ json: { user: {
      id: 1, username: 'mira', displayName: 'Mira', role: 'user', profileType: 'adult',
      preferredFormat: 'epub', preferredLanguage: 'en', preferredLanguages: ['en'],
      defaultLanguage: 'en', acquisitionMode: 'automatic', canAcquire: true,
    } } })
    if (pathname === '/api/books/2') return route.fulfill({ json: {
      id: 2, title: 'Fictional RTL Story', authors: ['Mara Quill'],
      files: [{ id: 2, format: 'epub', size: epub.length }],
    } })
    return route.fulfill({ json: {} })
  })
  await epubPage.goto(`${base}/read/2/2`)
  await epubPage.locator('.epub-container[dir="rtl"]').waitFor()
  const epubControls = epubPage.locator('footer button[aria-label]')
  assert.equal(await epubControls.first().getAttribute('aria-label'), 'Next page')
  await epubPage.getByRole('button', { name: 'Display settings' }).click()
  await epubPage.getByLabel('Reading direction').selectOption('ltr')
  await epubPage.locator('.epub-container[dir="ltr"]').waitFor()
  assert.equal(epubOverride, 'ltr')
  assert.equal(await epubControls.first().getAttribute('aria-label'), 'Previous page')

  const fixedZip = await JSZip.loadAsync(epub)
  const fixedPackage = await fixedZip.file('OEBPS/package.opf').async('string')
  fixedZip.file('OEBPS/package.opf', fixedPackage.replace('</metadata>', '<meta property="rendition:layout">pre-paginated</meta></metadata>'))
  const fixedEpub = await fixedZip.generateAsync({ type: 'nodebuffer', compression: 'DEFLATE' })
  const fixedPage = await browser.newPage({ viewport: { width: 1100, height: 800 } })
  await fixedPage.route('**/api/**', (route) => {
    const { pathname } = new URL(route.request().url())
    if (pathname.endsWith('/content')) return route.fulfill({ contentType: 'application/epub+zip', body: fixedEpub })
    if (pathname.endsWith('/position')) return route.fulfill({ json: {
      sha256: 'fictional-fixed-epub-sha', format: 'epub', position: null, external: null,
      direction: { bookDirection: null, seriesDirection: null, directionOverride: null },
    } })
    if (pathname === '/api/auth/me') return route.fulfill({ json: { user: {
      id: 1, username: 'mira', displayName: 'Mira', role: 'user', profileType: 'adult',
      preferredFormat: 'epub', preferredLanguage: 'en', preferredLanguages: ['en'],
      defaultLanguage: 'en', acquisitionMode: 'automatic', canAcquire: true,
    } } })
    if (pathname === '/api/books/4') return route.fulfill({ json: {
      id: 4, title: 'Fixed Lantern Atlas', authors: ['Mara Quill'],
      files: [{ id: 4, format: 'epub', size: fixedEpub.length }],
    } })
    return route.fulfill({ json: {} })
  })
  await fixedPage.goto(`${base}/read/4/4`)
  await fixedPage.getByRole('button', { name: 'Next page' }).waitFor()
  await fixedPage.getByRole('button', { name: 'Display settings' }).click()
  assert.equal(await fixedPage.getByRole('group', { name: 'Reader surface' }).count(), 1)
  assert.equal(await fixedPage.getByLabel('Font', { exact: true }).count(), 0)
  assert.equal(await fixedPage.locator('#reader-font').count(), 0)
  assert.equal(await fixedPage.getByLabel('Line spacing').count(), 0)

  const pdf = fictionalPdf()
  const pdfPage = await browser.newPage({ viewport: { width: 1100, height: 800 } })
  await pdfPage.route('**/api/**', (route) => {
    const { pathname } = new URL(route.request().url())
    if (pathname.endsWith('/content')) return route.fulfill({ contentType: 'application/pdf', body: pdf })
    if (pathname.endsWith('/position')) return route.fulfill({ json: {
      sha256: 'fictional-pdf-sha', format: 'pdf', position: null, external: null,
      direction: { bookDirection: null, seriesDirection: null, directionOverride: null },
    } })
    if (pathname === '/api/auth/me') return route.fulfill({ json: { user: {
      id: 1, username: 'mira', displayName: 'Mira', role: 'user', profileType: 'adult',
      preferredFormat: 'pdf', preferredLanguage: 'en', preferredLanguages: ['en'],
      defaultLanguage: 'en', acquisitionMode: 'automatic', canAcquire: true,
    } } })
    if (pathname === '/api/books/3') return route.fulfill({ json: {
      id: 3, title: 'The Lantern Atlas', authors: ['Mara Quill'],
      files: [{ id: 3, format: 'pdf', size: pdf.length }],
    } })
    return route.fulfill({ json: {} })
  })
  await pdfPage.goto(`${base}/read/3/3`)
  await pdfPage.getByRole('spinbutton', { name: 'Page number' }).waitFor()
  await pdfPage.getByRole('button', { name: 'Display settings' }).click()
  assert.equal(await pdfPage.getByRole('group', { name: 'Reader surface' }).count(), 1)
  assert.equal(await pdfPage.getByLabel('Font', { exact: true }).count(), 0)
  assert.equal(await pdfPage.getByLabel('Line spacing').count(), 0)
  assert.equal(await pdfPage.locator('#reader-font').count(), 0)
  assert.equal(await pdfPage.getByLabel('Zoom: 100%').count(), 1)
  await pdfPage.getByRole('button', { name: 'Ink', exact: true }).click()
  assert.equal(await pdfPage.locator('.bokhylle-reader').evaluate((root) => getComputedStyle(root).backgroundColor), 'rgb(26, 21, 17)')
  assert.deepEqual(await pdfPage.locator('main canvas').first().evaluate((canvas) => [...canvas.getContext('2d').getImageData(0, 0, 1, 1).data]), [255, 255, 255, 255], 'the PDF page retains its white paper')
  console.log('Reader direction and format display browser smoke test passed')
} finally {
  await browser.close()
}

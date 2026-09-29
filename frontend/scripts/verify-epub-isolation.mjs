// Browser smoke test for untrusted EPUB content against the Rust server's CSP.
// Start the server with BOKHYLLE_WEB_ROOT=frontend/dist, then set
// BOKHYLLE_READER_BASE to its URL if it is not running on port 8180.
import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { existsSync } from 'node:fs'
import { once } from 'node:events'
import { chromium, firefox, webkit } from 'playwright-core'
import JSZip from 'jszip'

const base = process.env.BOKHYLLE_READER_BASE ?? 'http://127.0.0.1:8180'
const chrome = [process.env.PLAYWRIGHT_CHROME_PATH, '/usr/bin/google-chrome', '/usr/bin/google-chrome-stable']
  .filter(Boolean).find(existsSync)
if (!chrome) throw new Error('Chrome is required for the EPUB isolation smoke test')

const probe = createServer((request, response) => {
  probeRequests.push(new URL(request.url, 'http://127.0.0.1').pathname)
  response.writeHead(200, { 'Content-Type': 'text/plain' })
  response.end('probe')
})
let probeRequests = []
probe.listen(0, '127.0.0.1')
await once(probe, 'listening')
const probeBase = `http://127.0.0.1:${probe.address().port}`

const zip = new JSZip()
zip.file('mimetype', 'application/epub+zip', { compression: 'STORE' })
zip.file('META-INF/container.xml', `<?xml version="1.0"?>
  <container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
    <rootfiles><rootfile full-path="OEBPS/package.opf" media-type="application/oebps-package+xml"/></rootfiles>
  </container>`)
zip.file('OEBPS/package.opf', `<?xml version="1.0"?>
  <package xmlns="http://www.idpf.org/2007/opf" unique-identifier="id" version="3.0">
    <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
      <dc:identifier id="id">urn:uuid:fictional-epub-isolation</dc:identifier>
      <dc:title>Fictional Isolation Test</dc:title><dc:language>en</dc:language>
    </metadata>
    <manifest>
      <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
      <item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/>
    </manifest>
    <spine><itemref idref="chapter"/></spine>
  </package>`)
zip.file('OEBPS/nav.xhtml', `<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Contents</title></head><body>
  <nav xmlns:epub="http://www.idpf.org/2007/ops" epub:type="toc"><ol><li><a href="chapter.xhtml">Test chapter</a></li></ol></nav>
  </body></html>`)
zip.file('OEBPS/chapter.xhtml', `<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Test chapter</title>
  <style>body { background-image: url('${probeBase}/css-image'); }</style>
  <script src="${probeBase}/external-script"></script>
  </head><body><h1>Fictional isolation chapter</h1>
  <p>The chapter must remain readable.</p>
  <script>document.body.dataset.epubScript = 'ran'; parent.document.body.dataset.epubScript = 'ran'; fetch('${probeBase}/fetch');</script>
  <img src="${probeBase}/image" alt="External image"/>
  <img src="data:image/png;base64,invalid" onerror="document.body.dataset.epubHandler = 'ran'; parent.document.body.dataset.epubHandler = 'ran'" alt="Broken image"/>
  </body></html>`)
const epub = await zip.generateAsync({ type: 'nodebuffer', compression: 'DEFLATE' })

try {
  for (const [name, engine, options] of [
    ['Chromium', chromium, { executablePath: chrome, args: ['--no-sandbox', '--disable-dev-shm-usage'] }],
    ['Firefox', firefox, {}],
    ['WebKit', webkit, {}],
  ]) {
    probeRequests = []
    const browser = await engine.launch(options)
    try {
      const controlPage = await browser.newPage()
      await controlPage.goto(`${probeBase}/control`)
      await controlPage.close()
      assert.ok(probeRequests.includes('/control'), `${name}: local probe was unreachable`)

      const page = await browser.newPage()
      const documentResponse = await page.request.get(base)
      assert.equal(documentResponse.status(), 200)
      const csp = documentResponse.headers()['content-security-policy']
      assert.ok(csp?.includes("script-src 'self'"), 'expected Rust server CSP')
      assert.ok(csp?.includes("img-src 'self' data: blob:"), 'expected restricted images')

      await page.route('**/api/**', (route) => {
        const path = new URL(route.request().url()).pathname
        if (path.endsWith('/content')) return route.fulfill({ contentType: 'application/epub+zip', body: epub })
        if (path.endsWith('/position')) return route.fulfill({ json: {
          sha256: 'fictional-isolation-sha', format: 'epub', position: null, external: null,
          direction: { bookDirection: null, seriesDirection: null, directionOverride: null },
        } })
        if (path === '/api/auth/me') return route.fulfill({ json: { user: {
          id: 1, username: 'mira', displayName: 'Mira', role: 'user', profileType: 'adult',
          preferredFormat: 'epub', preferredLanguage: 'en', preferredLanguages: ['en'],
          defaultLanguage: 'en', acquisitionMode: 'automatic', canAcquire: true,
        } } })
        if (path === '/api/books/1') return route.fulfill({ json: {
          id: 1, title: 'Fictional Isolation Test', authors: ['Mara Quill'],
          files: [{ id: 1, format: 'epub', size: epub.length }],
        } })
        return route.fulfill({ json: {} })
      })

      await page.goto(`${base}/read/1/1`)
      const iframe = page.locator('main[aria-label="EPUB reading area"] iframe')
      await page.frameLocator('main[aria-label="EPUB reading area"] iframe')
        .getByText('Fictional isolation chapter').waitFor()
      assert.equal(await iframe.getAttribute('sandbox'), 'allow-same-origin')
      await page.waitForTimeout(500)
      assert.deepEqual(await page.evaluate(() => ({
        script: document.body.dataset.epubScript,
        handler: document.body.dataset.epubHandler,
      })), { script: undefined, handler: undefined })
      assert.deepEqual(await page.frameLocator('main[aria-label="EPUB reading area"] iframe')
        .locator('body').evaluate((body) => ({
          script: body.dataset.epubScript,
          handler: body.dataset.epubHandler,
        })), { script: undefined, handler: undefined })
      assert.deepEqual(
        probeRequests.filter((path) => path !== '/control' && path !== '/favicon.ico'),
        [], `${name}: EPUB contacted an external host`,
      )
      console.log(`${name} EPUB isolation passed: chapter rendered; scripts, handlers, and external requests blocked`)
    } finally {
      await browser.close()
    }
  }
} finally {
  probe.close()
  await once(probe, 'close')
}

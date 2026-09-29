// Capture reader previews from a fictional EPUB and mocked local API responses.
// Start `pnpm -C frontend preview --host 127.0.0.1 --port 4175` first.
import { existsSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import JSZip from 'jszip'
import { chromium } from 'playwright-core'

const base = process.env.BOKHYLLE_READER_BASE ?? 'http://127.0.0.1:4175'
const root = fileURLToPath(new URL('../../', import.meta.url))
const chrome = [process.env.PLAYWRIGHT_CHROME_PATH, '/usr/bin/google-chrome', '/usr/bin/google-chrome-stable']
  .filter(Boolean).find(existsSync)
if (!chrome) throw new Error('Chrome is required to capture reader screenshots')

const zip = new JSZip()
zip.file('mimetype', 'application/epub+zip', { compression: 'STORE' })
zip.file('META-INF/container.xml', `<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/package.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>`)
zip.file('OEBPS/package.opf', `<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="id" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="id">urn:uuid:0e12f47a-80a2-4cb7-989b-c91effc704a0</dc:identifier>
    <dc:title>Where Maps End</dc:title><dc:creator>Nora Vale</dc:creator><dc:language>en</dc:language>
    <meta property="dcterms:modified">2026-01-01T00:00:00Z</meta>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/>
    <item id="chapter2" href="chapter2.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="chapter"/><itemref idref="chapter2"/></spine>
</package>`)
zip.file('OEBPS/nav.xhtml', `<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Contents</title></head><body>
<nav xmlns:epub="http://www.idpf.org/2007/ops" epub:type="toc"><ol>
<li><a href="chapter.xhtml">Chapter One: The Northern Road</a></li>
<li><a href="chapter2.xhtml">Chapter Two: The Lighthouse</a></li>
</ol></nav></body></html>`)
zip.file('OEBPS/chapter.xhtml', `<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>The Northern Road</title>
<style>body{font-family:Georgia,serif;line-height:1.65;margin:3em auto;max-width:38em}h1{font-weight:normal;font-size:2em;margin-bottom:1.5em}p{text-indent:1.2em;margin:.7em 0}p:first-of-type{text-indent:0}</style>
</head><body><h1>Chapter One<br/>The Northern Road</h1>
<p>At dawn, Mira unfolded the map on the kitchen table and found a road that had not been there the night before. It ran north from the harbour, past a lighthouse marked only with a small blue circle.</p>
<p>The town was still asleep. From the window she could see the fishing boats resting against the pier, their ropes drawing loose lines in the water. She folded the map along its oldest crease and put it in her coat.</p>
<p>By the time she reached the square, the first bakery had opened. Its windows glowed against the rain. Beyond the last row of houses, the road turned into a narrow path between low stone walls.</p>
<p>She had walked this way a hundred times and knew every gate. Today, one of them stood open. On its post was a hand-painted sign: <em>For those who are looking.</em></p>
<p>Mira stopped and listened. Somewhere ahead, a bell rang once. She stepped through the gate, and the harbour disappeared behind the trees.</p>
</body></html>`)
zip.file('OEBPS/chapter2.xhtml', `<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>The Lighthouse</title></head><body>
<h1>Chapter Two: The Lighthouse</h1><p>By evening, the path had reached the sea.</p>
</body></html>`)
const epub = await zip.generateAsync({ type: 'nodebuffer', compression: 'DEFLATE' })

const user = {
  id: 1, username: 'mira', displayName: 'Mira', role: 'user', profileType: 'adult',
  preferredFormat: 'epub', preferredLanguage: 'en', preferredLanguages: ['en'],
  defaultLanguage: 'en', acquisitionMode: 'automatic', notificationEmail: null,
  emailNotifications: false, canAcquire: true,
}
const book = {
  id: 1, title: 'Where Maps End', authors: ['Nora Vale'], files: [
    { id: 1, format: 'epub', sha256: 'fictional-epub-sha', size: epub.length },
  ],
}

const browser = await chromium.launch({ executablePath: chrome, args: ['--no-sandbox', '--disable-dev-shm-usage'] })
try {
  for (const [width, height, destination] of [
    [1440, 900, join(root, 'website/assets/demo-reader.png')],
    [390, 844, join(root, 'website/assets/demo-reader-mobile.png')],
    [390, 844, join(root, 'docs/media/reader-mobile.png')],
  ]) {
    const page = await browser.newPage({ viewport: { width, height }, deviceScaleFactor: 1, colorScheme: 'light' })
    let position = null
    await page.route('**/api/**', (route) => {
      const url = new URL(route.request().url())
      if (url.pathname === '/api/books/1/files/1/content') {
        return route.fulfill({ status: 200, contentType: 'application/epub+zip', body: epub })
      }
      if (url.pathname === '/api/books/1/files/1/position') {
        if (route.request().method() === 'PUT') {
          const update = route.request().postDataJSON()
          position = { locator: update.locator, percentage: update.percentage,
            completed: update.completed, revision: (position?.revision ?? 0) + 1, updatedAt: 1790510400 }
        }
        return route.fulfill({ json: { sha256: 'fictional-epub-sha', format: 'epub', position, external: null,
          direction: { bookDirection: 'ltr', directionOverride: null } } })
      }
      const response = url.pathname === '/api/auth/me' ? { user }
        : url.pathname === '/api/demo' ? { enabled: false }
          : url.pathname === '/api/books/1' ? book : {}
      return route.fulfill({ json: response })
    })
    await page.goto(`${base}/read/1/1`, { waitUntil: 'networkidle' })
    await page.locator('main[aria-label="EPUB reading area"] iframe').waitFor()
    await page.frameLocator('main[aria-label="EPUB reading area"] iframe').getByText('The Northern Road').waitFor()
    if (width > 700) {
      await page.getByRole('button', { name: 'Display settings' }).click()
      await page.getByRole('button', { name: 'Warm', exact: true }).click()
      await page.getByText('Adjusting page…').waitFor({ state: 'hidden' })
    }
    await page.screenshot({ path: destination })
    await page.close()
  }
} finally {
  await browser.close()
}

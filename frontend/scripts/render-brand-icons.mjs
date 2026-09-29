// Rasterize the approved vector icon with Chromium. ImageMagick's SVG renderer
// misplaces the rotated terracotta book in this artwork.
import { existsSync } from 'node:fs'
import { readFile } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chromium } from 'playwright-core'

const icons = join(dirname(fileURLToPath(import.meta.url)), '..', 'public', 'icons')
const chrome = [
  process.env.PLAYWRIGHT_CHROME_PATH,
  '/usr/bin/google-chrome',
  '/usr/bin/google-chrome-stable',
].filter(Boolean).find(existsSync)

if (!chrome) throw new Error('Chrome is required to render brand icons')

const browser = await chromium.launch({
  executablePath: chrome,
  args: ['--no-sandbox', '--disable-dev-shm-usage'],
})

try {
  for (const [name, size, source] of [
    ['icon-180.png', 180, 'icon.svg'],
    ['icon-192.png', 192, 'icon.svg'],
    ['icon-512.png', 512, 'icon.svg'],
    ['icon-maskable-512.png', 512, 'icon-maskable.svg'],
  ]) {
    const page = await browser.newPage({
      viewport: { width: size, height: size },
      deviceScaleFactor: 1,
    })
    const svg = await readFile(join(icons, source), 'utf8')
    await page.setContent(`<!doctype html><style>
      html, body { margin: 0; width: 100%; height: 100%; }
      body > svg { display: block; width: 100%; height: 100%; }
    </style>${svg}`)
    await page.screenshot({ path: join(icons, name) })
    await page.close()
  }
} finally {
  await browser.close()
}

// Capture README previews from the current app with fictional local data.
// Start `pnpm -C frontend preview --host 127.0.0.1 --port 4173` first.
import { existsSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chromium } from 'playwright-core'

const base = process.env.BOKHYLLE_DOCS_BASE ?? 'http://127.0.0.1:4173'
const output = fileURLToPath(new URL('../../docs/media/', import.meta.url))
const chrome = [
  process.env.PLAYWRIGHT_CHROME_PATH,
  '/usr/bin/google-chrome',
  '/usr/bin/google-chrome-stable',
].filter(Boolean).find(existsSync)

if (!chrome) throw new Error('Chrome is required to capture documentation screenshots')

const titles = [
  ['Where Maps End', 'Nora Vale'],
  ['Tides of Ink', 'Oskar Lind'],
  ['The Wild Index', 'Amos Ryd'],
  ['The Quiet Machine', 'Elias Venn'],
  ['The Paper Atlas', 'Mira Solberg'],
  ['The Last Lightkeeper', 'Freya Dahl'],
  ['The Glass Orchard', 'Nora Vale'],
  ['The Fourth Garden', 'Mads Holm'],
  ['The City Beneath Rain', 'Erik Lund'],
  ['Notes from the Fjord', 'Ingrid Vale'],
  ['North of Nowhere', 'Selma Rehn'],
  ['Moss and Mercury', 'Karl Voss'],
  ['A Map of Quiet Places', 'Ingrid Vale'],
  ['A Winter Catalogue', 'Selma Rehn'],
  ['Archive of Stars', 'Selma Rehn'],
  ['The Harbour Clock', 'Freya Dahl'],
  ['The Blue Lantern', 'Amos Ryd'],
  ['An Atlas of Small Things', 'Mira Solberg'],
  ['The Glass Compass · Volume 1', 'Mara Quill'],
  ['The Glass Compass · Volume 2', 'Mara Quill'],
  ['The Lantern Papers', 'Iris North'],
]
const books = titles.map(([title, author], index) => ({
  id: index + 1, title, authors: [author], language: 'en',
  series: index === 18 || index === 19 ? 'The Glass Compass' : null,
  seriesId: index === 18 || index === 19 ? 42 : null,
  seriesNumber: index === 18 ? '1' : index === 19 ? '2' : null,
  seriesSortOrder: index === 18 ? 1 : index === 19 ? 2 : null,
  publicationKind: index === 18 || index === 19 ? 'manga' : index === 20 ? 'comic' : 'book',
  hasCover: false, hasDescription: false, rating: null,
  ratingCount: null, ratingSource: null, addedAt: 1760000000 - index * 86400,
}))
const spotlight = [
  {
    source: 'shelf', ownership: 'shelf', reasonType: 'rediscover',
    reasonLabel: 'From your shelf', title: books[0].title,
    authors: books[0].authors, bookId: books[0].id, language: 'en',
    subjects: ['Maps', 'Adventure'], cta: 'explore',
    blurb: 'An unexpected letter sends a cartographer across old roads and unfamiliar coastlines, tracing a place that may exist only in someone else’s memory.',
  },
  {
    source: 'shelf', ownership: 'shelf', reasonType: 'rediscover',
    reasonLabel: 'From your shelf', title: books[1].title,
    authors: books[1].authors, bookId: books[1].id, language: 'en',
    subjects: ['Mystery'], cta: 'explore',
    blurb: 'A missing manuscript draws its keeper into a city of private archives, where every catalogue entry points to a different version of the truth.',
  },
]
const coverColours = ['#4f5d46', '#574936', '#243542', '#594a36', '#a3461f', '#253543', '#a3461f', '#653943', '#253543', '#253543', '#653943', '#594a36']
function coverSvg(book) {
  const words = book.title.split(' ')
  const lines = []
  for (const word of words) {
    const last = lines.length - 1
    if (last >= 0 && `${lines[last]} ${word}`.length <= 16) lines[last] += ` ${word}`
    else lines.push(word)
  }
  const start = 295 - (lines.length - 1) * 24
  const title = lines.map((line, index) =>
    `<tspan x="200" y="${start + index * 48}">${line}</tspan>`).join('')
  const colour = coverColours[(book.id - 1) % coverColours.length]
  return `<svg xmlns="http://www.w3.org/2000/svg" width="400" height="600" viewBox="0 0 400 600">
    <rect width="400" height="600" fill="${colour}"/>
    <rect x="16" y="16" width="368" height="568" fill="none" stroke="#f1e9da" stroke-opacity=".35"/>
    <text x="40" y="44" fill="#f1e9da" font-family="Arial,sans-serif" font-size="12" letter-spacing="6">BOKHYLLE</text>
    <line x1="40" y1="58" x2="360" y2="58" stroke="#f1e9da" stroke-opacity=".35"/>
    <text text-anchor="middle" fill="#fff8ed" font-family="Georgia,serif" font-size="43">${title}</text>
    <text x="200" y="535" text-anchor="middle" fill="#f1e9da" font-family="Arial,sans-serif" font-size="16" letter-spacing="1">${book.authors[0]}</text>
    <line x1="40" y1="553" x2="360" y2="553" stroke="#f1e9da" stroke-opacity=".35"/>
    <text x="40" y="575" fill="#f1e9da" font-family="Arial,sans-serif" font-size="9" letter-spacing="1">PRIVATE LIBRARY</text>
  </svg>`
}
const user = {
  id: 1, username: 'mira', displayName: 'Mira', role: 'user',
  profileType: 'adult', preferredFormat: 'epub', preferredLanguage: 'en',
  preferredLanguages: ['en'], defaultLanguage: 'en', acquisitionMode: 'automatic',
  notificationEmail: null, emailNotifications: false,
  canAcquire: true,
}

function responseFor(url, adminPreview = false) {
  const path = url.pathname
  if (path === '/api/auth/me') return { user: adminPreview ? { ...user, role: 'admin' } : user }
  if (path === '/api/profile/onboarding') return { onboarded: true, interests: [] }
  if (path === '/api/catalogues') return [{ id: 'open-shelf', name: 'Open shelf', url: 'https://catalogue.example.test/opds' }]
  if (path === '/api/catalogues/open-shelf/feed') return {
    title: 'The open reading room', pageUrl: 'https://catalogue.example.test/opds',
    navigation: [], next: null, searchAvailable: true,
    entries: books.slice(0, 4).map((book) => ({
      id: `publication-${book.id}`, title: book.title, authors: book.authors, language: 'en',
      files: [{ index: 0, format: 'epub', label: 'Open access EPUB' }],
    })),
  }
  if (path === '/api/admin/settings') return {
    settings: { 'imports.watch_enabled': true, 'imports.watch_folder': '/config/ingest' },
    envOverrides: {}, secretsConfigured: {},
  }
  if (path === '/api/admin/integrations/status') {
    const unused = { configured: false, ok: false, version: null, error: null }
    return {
      prowlarr: unused, torznab: unused, newznab: unused, sabnzbd: unused, qbittorrent: unused,
      smtp: { configured: false }, library: { path: '/library', exists: true, writable: true },
      hardlinks: { supported: true, message: 'Supported' },
    }
  }
  if (path === '/api/admin/maintenance/watch') return {
    enabled: true, path: '/config/ingest', pending: 0, cleanupPending: 0, reviewFiles: 0, lastError: null,
  }
  if (path === '/api/admin/maintenance/imports') return {
    running: false, startedAt: null, finishedAt: null, imported: 0,
    already: 0, skipped: 0, failed: 0, files: [], failures: [], error: null,
  }
  if (path === '/api/books') return {
    items: books, total: books.length, letters: [...new Set(books.map((book) => book.title[0]))],
    page: 1, pageSize: 24,
  }
  if (path === '/api/books/recent') return books.slice(0, 12)
  if (path === '/api/books/highlights') return books.slice(11, 18).concat(books.slice(0, 5))
  if (path === '/api/books/continue') return []
  if (path === '/api/books/facets') return { formats: [], languages: [], series: [], subjects: [], publicationKinds: [{ value: 'book', count: 18 }, { value: 'manga', count: 2 }, { value: 'comic', count: 1 }] }
  if (path === '/api/library/comics') return { items: [
    { type: 'series', value: { id: 42, name: 'The Glass Compass', sortName: null, defaultReadingDirection: 'rtl', volumeCount: 2, coverBookId: 20, addedAt: 1760000000 } },
    { type: 'book', value: books[20] },
  ], total: 2, page: 1, pageSize: 24 }
  if (path === '/api/series/42') return { id: 42, name: 'The Glass Compass', sortName: null, defaultReadingDirection: 'rtl', volumes: [books[18], books[19]], reading: { finishedBookIds: [19], current: null, nextBookId: 20, missingNextVolume: null } }
  if (path === '/api/admin/series') return [{ id: 42, name: 'The Glass Compass', sortName: null, defaultReadingDirection: 'rtl' }]
  if (path === '/api/admin/books/classification-review') {
    const all = [
      { id: 19, title: books[18].title, fileName: 'The Glass Compass - Volume 1.cbz', format: 'cbz', legacySeriesText: null, seriesId: null, seriesName: null, seriesNumber: null, seriesSortOrder: null, publicationKind: 'unknown', readingDirection: null, reviewedAt: null, suggestion: { publicationKind: 'comic', seriesName: 'The Glass Compass', seriesNumber: '1', reason: 'Volume marker in title; verify the series grouping', needsReview: true } },
      { id: 20, title: books[19].title, fileName: 'The Glass Compass - Volume 2.cbz', format: 'cbz', legacySeriesText: null, seriesId: null, seriesName: null, seriesNumber: null, seriesSortOrder: null, publicationKind: 'unknown', readingDirection: null, reviewedAt: null, suggestion: { publicationKind: 'comic', seriesName: 'The Glass Compass', seriesNumber: '2', reason: 'Volume marker in title; verify the series grouping', needsReview: true } },
      { id: 21, title: books[20].title, fileName: 'The Lantern Papers.cbz', format: 'cbz', legacySeriesText: null, seriesId: null, seriesName: null, seriesNumber: null, seriesSortOrder: null, publicationKind: 'unknown', readingDirection: null, reviewedAt: null, suggestion: { publicationKind: 'comic', seriesName: null, seriesNumber: null, reason: 'CBZ file; verify whether this is a comic or manga', needsReview: true } },
    ]
    const kind = url.searchParams.get('kind') ?? 'all'
    const attention = url.searchParams.get('attention') ?? 'all'
    const page = Number(url.searchParams.get('page') ?? '1')
    const pageSize = Number(url.searchParams.get('pageSize') ?? '25')
    const matches = all.filter((item) => (kind === 'all' || item.suggestion.publicationKind === kind)
      && (attention === 'all' || (attention === 'review') === item.suggestion.needsReview))
    return {
      items: matches.slice((page - 1) * pageSize, page * pageSize),
      total: matches.length, page, pageSize,
      counts: { total: 3, book: 0, comic: 3, manga: 0, simple: 0, needsReview: 3 },
    }
  }
  if (path === '/api/authors') return []
  if (path === '/api/collections') return []
  if (path === '/api/household/members') return { members: [] }
  if (path === '/api/home/rails') return []
  if (path === '/api/home/spotlight') return { items: spotlight, recommendations: [] }
  if (path === '/api/home/updates') return { library: [], discoveries: [], ready: [] }
  if (path === '/api/notifications') return {
    items: [], unread: 0, pendingRequests: 0, pendingRequestItems: [],
  }
  return []
}

const browser = await chromium.launch({
  executablePath: chrome,
  args: ['--no-sandbox', '--disable-dev-shm-usage'],
})
try {
  for (const [path, name, width, height, readyText] of [
    ['/library', 'library-desktop.png', 1440, 900, 'Where Maps End'],
    ['/library', 'library-mobile.png', 390, 844, 'Where Maps End'],
    ['/library?category=comics', 'comics-desktop.png', 1440, 900, 'The Glass Compass'],
    ['/series/42', 'series-desktop.png', 1440, 900, 'The Glass Compass'],
    ['/library/review', 'import-review-desktop.png', 1440, 900, 'The Glass Compass · Volume 1'],
    ['/library/review', 'import-review-preview-desktop.png', 1440, 900, 'The Glass Compass · Volume 1'],
    ['/', 'home-desktop.png', 1440, 900, 'Recently Added'],
    ['/catalogues', 'catalogues-desktop.png', 1440, 900, 'The open reading room'],
    ['/catalogues', 'catalogues-mobile.png', 390, 844, 'The open reading room'],
    ['/settings/getting-books', 'acquisition-settings-desktop.png', 1440, 900, 'Import folder'],
  ]) {
    if (process.env.BOKHYLLE_DOCS_SELECT && !process.env.BOKHYLLE_DOCS_SELECT.split(',').includes(name)) continue
    const page = await browser.newPage({
      viewport: { width, height }, deviceScaleFactor: 1, colorScheme: 'light', timezoneId: 'UTC',
    })
    await page.clock.setFixedTime(new Date('2026-01-15T10:00:00Z'))
    const reviewPreview = path === '/library/review'
    const adminPreview = reviewPreview || path.startsWith('/settings')
    await page.route('**/api/**', (route) => {
      const url = new URL(route.request().url())
      const coverId = /^\/api\/books\/(\d+)\/cover$/.exec(url.pathname)?.[1]
      if (coverId) {
        const book = books.find((item) => item.id === Number(coverId))
        return route.fulfill({
          status: book ? 200 : 404, contentType: 'image/svg+xml',
          body: book ? coverSvg(book) : '',
        })
      }
      return route.fulfill({
        status: 200, contentType: 'application/json',
        body: JSON.stringify(responseFor(url, adminPreview)),
      })
    })
    await page.goto(`${base}${path}`, { waitUntil: 'networkidle' })
    await page.getByText(readyText).first().waitFor()
    if (reviewPreview) {
      await page.getByText('Review details').first().click()
      await page.getByLabel('Select The Glass Compass · Volume 1').check()
      await page.getByLabel('Select The Glass Compass · Volume 2').check()
      await page.locator('div.sticky').evaluate((element) => { element.style.position = 'static' })
      if (name === 'import-review-preview-desktop.png') {
        await page.getByRole('button', { name: 'Preview acceptance' }).click()
        await page.getByRole('dialog', { name: 'Accept 2 files?' }).waitFor()
      }
    }
    await page.screenshot({ path: join(output, name), fullPage: path === '/' || path === '/series/42' || path.startsWith('/settings') || (reviewPreview && name !== 'import-review-preview-desktop.png') })
    await page.close()
  }
} finally {
  await browser.close()
}

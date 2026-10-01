// Capture detailed feature illustrations with fictional local data.
// Home, library, and reader previews come from capture-website.mjs instead.
// Start `pnpm -C frontend preview --host 127.0.0.1 --port 4173` first.
import { existsSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chromium } from 'playwright-core'
import { signInProfiles } from './sign-in-preview.mjs'
import { serverPreview } from './server-preview.mjs'

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
  canAcquire: true, avatarPreset: 'book', avatarVersion: null,
}
const serverState = serverPreview()

function responseFor(url, adminPreview = false) {
  const path = url.pathname
  if (path === '/api/auth/me') return { user: adminPreview ? { ...user, role: 'admin' } : user }
  if (path === '/api/profile/onboarding') return { onboarded: true, interests: [] }
  if (path === '/api/profile/stats') return { shelf: 5, authors: 2, liked: 3, booksSent: 2 }
  if (path === '/api/profile/liked') return { items: [] }
  if (path === '/api/admin/users') return [{ ...user, role: 'admin', disabled: false, readerCount: 0 }]
  if (path === '/api/admin/users/profiles') return { users: [{ userId: 1, profileType: 'adult' }] }
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
  if (path === '/api/admin/server') return serverState.server
  if (path === '/api/admin/server/restart') return []
  if (path === '/api/admin/maintenance/backups') return serverState.backups
  if (path === '/api/admin/server/updates') return serverState.updates
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
  const detailId = /^\/api\/books\/(\d+)$/.exec(path)?.[1]
  if (detailId) {
    const book = books.find((item) => item.id === Number(detailId))
    return { ...book, authorRefs: [], availableLanguages: ['en'], readingDirection: null,
      description: 'An unexpected letter sends a cartographer across old roads and unfamiliar coastlines.',
      publicationYear: 2024, onShelf: true, preference: null, browserFileId: null, legacySeriesText: null,
      subjects: [], files: [], editions: [{ id: 1, title: book.title, language: 'en', publicationYear: 2024,
        isbn10: null, isbn13: null, publisher: 'Paper Atlas Press', unknown: false,
        metadataSources: [{ field: 'publicationYear', source: 'epub', sourceKey: null, manual: false }] }],
      metadataSources: [
        { field: 'title', source: 'manual', sourceKey: null, manual: true },
        { field: 'authors', source: 'openlibrary', sourceKey: '/works/FICTIONAL', manual: false },
        { field: 'language', source: 'epub', sourceKey: null, manual: false },
        { field: 'description', source: 'manual', sourceKey: null, manual: true },
      ],
    }
  }
  if (/^\/api\/books\/\d+\/related$/.test(path)) return { series: [], author: [], similar: [] }
  if (path === '/api/deliveries') return []
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
  if (path === '/api/home/rails') return [
    { key: 'shelf-adventure', title: 'Adventure', subtitle: 'Matches your interests', subject: 'adventure', books: books.slice(5, 10) },
    { key: 'liked-adventure', title: 'Based on books you liked', subject: null, books: books.slice(12, 16) },
  ]
  if (path === '/api/home/spotlight') return { items: spotlight, recommendations: books.slice(5, 10).map((book) => ({
    ...spotlight[0], bookId: book.id, title: book.title, authors: book.authors,
    source: 'household', ownership: 'household', reasonLabel: 'Matches your interests',
  })) }
  if (path === '/api/home/updates') return { library: [], ready: [], discoveries: books.slice(12, 14).map((book) => ({
    authorId: 1, title: book.title, authors: book.authors, provider: 'local', providerKey: `local:${book.id}`, year: null, coverId: null,
  })) }
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
    ['/login', 'sign-in-desktop.png', 1280, 800, 'Who’s reading?'],
    ['/login', 'sign-in-mobile.png', 390, 844, 'Who’s reading?'],
    ['/library?category=comics', 'comics-desktop.png', 1440, 900, 'The Glass Compass'],
    ['/series/42', 'series-desktop.png', 1440, 900, 'The Glass Compass'],
    ['/library/review', 'import-review-desktop.png', 1440, 900, 'The Glass Compass · Volume 1'],
    ['/library/review', 'import-review-preview-desktop.png', 1440, 900, 'The Glass Compass · Volume 1'],
    ['/library/1', 'metadata-corrections-desktop.png', 1440, 1000, 'Where Maps End'],
    ['/library/1', 'metadata-corrections-mobile.png', 390, 844, 'Where Maps End'],
    ['/catalogues', 'catalogues-desktop.png', 1440, 900, 'The open reading room'],
    ['/catalogues', 'catalogues-mobile.png', 390, 844, 'The open reading room'],
    ['/settings/getting-books', 'acquisition-settings-desktop.png', 1440, 900, 'Import folder'],
    ['/settings/server', 'server-desktop.png', 1440, 1000, 'Diagnostics'],
    ['/settings/server', 'server-mobile.png', 390, 844, 'Diagnostics'],
    ['/profile', 'profile-marks-mobile.png', 390, 844, 'My profile'],
    ['/settings/household', 'child-access-desktop.png', 1280, 900, 'Household users'],
    ['/settings/household', 'child-starting-books-desktop.png', 1280, 900, 'Household users'],
    ['/welcome', 'reading-interests-mobile.png', 390, 844, 'What do you like to read?'],
  ]) {
    if (process.env.BOKHYLLE_DOCS_SELECT && !process.env.BOKHYLLE_DOCS_SELECT.split(',').includes(name)) continue
    const page = await browser.newPage({
      viewport: { width, height }, deviceScaleFactor: 1, colorScheme: 'light', timezoneId: 'UTC',
      hasTouch: path === '/login' && width < 700,
      isMobile: path === '/login' && width < 700,
    })
    await page.clock.setFixedTime(new Date('2026-01-15T10:00:00Z'))
    const reviewPreview = path === '/library/review'
    const correctionsPreview = name.startsWith('metadata-corrections-')
    const adminPreview = reviewPreview || correctionsPreview || path.startsWith('/settings')
    await page.route('**/api/**', (route) => {
      const url = new URL(route.request().url())
      if (path === '/login') {
        if (url.pathname === '/api/auth/me') return route.fulfill({ status: 401, json: { code: 'unauthorized', message: 'Authentication required' } })
        if (url.pathname === '/api/demo') return route.fulfill({ json: { enabled: false } })
        if (url.pathname === '/api/auth/users') return route.fulfill({ json: { users: signInProfiles } })
      }
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
    if (name === 'profile-marks-mobile.png') await page.getByRole('button', { name: 'Change picture', exact: true }).click()
    if (name.startsWith('child-')) {
      await page.getByRole('button', { name: 'Add user', exact: true }).click()
      await page.getByLabel('Username', { exact: true }).fill('nora')
      await page.getByLabel('Display name', { exact: true }).fill('Nora')
      await page.getByRole('dialog').locator('input[type="password"]').fill('482915')
      await page.getByRole('combobox', { name: /^Profile type/ }).selectOption('child')
      await page.getByRole('radio', { name: 'Owl', exact: true }).check()
      await page.getByRole('radio', { name: /^Explore and ask/ }).check()
      if (name === 'child-starting-books-desktop.png') {
        await page.getByRole('button', { name: 'Choose starting books', exact: true }).click()
        await page.getByRole('checkbox', { name: /Where Maps End/ }).check()
        await page.getByRole('list', { name: 'Available household books' }).getByRole('checkbox').nth(1).check()
      } else await page.getByRole('group', { name: 'How can this child find new books?' }).scrollIntoViewIfNeeded()
    }
    if (name === 'reading-interests-mobile.png') {
      await page.getByRole('button', { name: 'Fantasy', exact: true }).click()
      await page.getByRole('button', { name: 'Animals', exact: true }).click()
      await page.getByLabel('Search reading interests', { exact: true }).fill('Architecture')
      await page.getByRole('button', { name: 'Add “Architecture”', exact: true }).click()
      await page.getByLabel('Search reading interests', { exact: true }).fill('nature')
      await page.evaluate(() => window.scrollTo(0, 0))
    }
    if (name === 'sign-in-mobile.png') {
      await page.getByRole('button', { name: 'Nora', exact: true }).tap()
      await page.getByLabel('PIN', { exact: true }).waitFor()
    }
    await page.evaluate(() => document.fonts.ready)
    // Screenshots include covers beyond the viewport as well as profile marks.
    await page.evaluate(() => { for (const image of document.images) image.loading = 'eager' })
    await page.waitForFunction(() => Array.from(document.images).every((image) => image.complete))
    // Capture the settled state, including the short credential-panel transition.
    if (await page.locator('#sign-in-panel').count()) {
      await page.waitForFunction(() => document.querySelector('#sign-in-panel')?.getAttribute('aria-busy') !== 'true')
      await page.locator('#sign-in-panel').evaluate((panel) => Promise.all(panel.getAnimations().map((animation) => animation.finished.catch(() => {}))))
      // Selection can gently scroll the form into view after its expansion.
      await page.evaluate(() => new Promise((resolve) => {
        let last = window.scrollY
        let stable = 0
        const sample = () => {
          const current = window.scrollY
          stable = Math.abs(current - last) < 0.5 ? stable + 1 : 0
          last = current
          if (stable >= 4) resolve()
          else requestAnimationFrame(sample)
        }
        requestAnimationFrame(sample)
      }))
    }
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
    if (correctionsPreview) {
      await page.getByLabel('Book actions', { exact: true }).click()
      await page.getByRole('button', { name: 'Fix details', exact: true }).click()
      await page.getByRole('dialog', { name: 'Fix details', exact: true }).waitFor()
    }
    await page.screenshot({ path: join(output, name), fullPage: name !== 'server-mobile.png' && (path === '/' || path === '/series/42' || path.startsWith('/settings') || (reviewPreview && name !== 'import-review-preview-desktop.png')) })
    await page.close()
  }
} finally {
  await browser.close()
}

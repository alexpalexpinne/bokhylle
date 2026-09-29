import axe from 'axe-core'
import { expect } from '../support.mjs'

const COVERS = [
  [360, 560, '#ef4e16', 'A BRIGHT BOOK'],
  [340, 560, '#172534', 'AFTER DARK'],
  [80, 130, '#b7a785', 'OLD STORIES'],
  [280, 560, '#f5df39', 'WORDS ON PAPER'],
  [440, 440, '#31867d', 'A SQUARE COVER'],
  [150, 600, '#622434', 'THE LONG WAY'],
  null,
  [900, 300, '#597dba', 'A WIDE HORIZON'],
]

function coverSvg([width, height, colour, title]) {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">
    <rect width="${width}" height="${height}" fill="${colour}"/>
    <rect x="${width * 0.08}" y="${height * 0.06}" width="${width * 0.84}" height="${height * 0.88}" fill="none" stroke="#f7f4ee" stroke-width="2"/>
    <text x="${width / 2}" y="${height * 0.38}" fill="#fff" font-family="Georgia" font-size="${width * 0.08}" text-anchor="middle">${title}</text>
    <text x="${width / 2}" y="${height * 0.82}" fill="#fff" font-family="sans-serif" font-size="${width * 0.06}" text-anchor="middle">TEST AUTHOR</text>
  </svg>`
}

const BOOKS = COVERS.map((cover, index) => ({
  id: 9700 + index, title: cover?.[3] ?? 'A book without artwork', authors: ['Test Author'],
  language: 'en', series: null, seriesNumber: null, hasCover: Boolean(cover),
  hasDescription: true, rating: null, ratingCount: null, ratingSource: null, addedAt: 1,
}))

const SPOTLIGHT = [
  { bookId: 9900, title: 'The quiet library', authors: ['A. Reader'], subjects: ['Open Library Staff Picks', 'staff_picks', 'Literature'], language: 'ru', languages: ['ru', 'en'] },
  { bookId: 9901, title: 'A much longer title about all the books we have yet to read', authors: ['Alexandra Reader', 'Christopher Writer'], reasonLabel: 'Because you enjoy literary fiction', subjects: [], rating: 4.3, ratingCount: 145 },
  { bookId: 9902, title: 'A different shape', authors: [], subjects: [], languages: ['sv'], language: 'sv' },
].map((item) => ({
  source: 'shelf', ownership: 'shelf', reasonType: 'shelf', reasonLabel: 'From your shelf',
  blurb: 'A quiet story about finding something unexpected among familiar books. A little space to think, and a new place to begin.',
  language: 'en', cta: 'explore', ...item,
}))

const USER = {
  id: 4343, username: 'reader', displayName: 'Alex', role: 'user', profileType: 'adult',
  preferredFormat: 'epub', preferredLanguage: 'en', preferredLanguages: ['en'],
  defaultLanguage: 'en', acquisitionMode: 'automatic', notificationEmail: null, emailNotifications: false,
  shelfFinish: 'oak', shelfDecorations: true, spotlightRotation: true,
}

// Mock the existing contracts; this fixture never reads or changes a household's data.
export async function mockHomeDesign(page, { progress = false, child = false, appearance = {}, spotlightItems = SPOTLIGHT, highlights = [] } = {}) {
  const forbidden = []
  const user = { ...USER, profileType: child ? 'child' : 'adult', ...appearance }
  await page.route('**/api/**', (route) => {
    const url = new URL(route.request().url())
    const path = url.pathname
    let body = []
    if (child && ['/api/authors', '/api/collections', '/api/home/updates'].includes(path)) forbidden.push(path)
    if (path === '/api/auth/me') body = { user }
    else if (path === '/api/profile' && route.request().method() === 'PUT') {
      Object.assign(user, route.request().postDataJSON())
      body = { user }
    }
    else if (path === '/api/profile/liked') body = { items: [] }
    else if (path === '/api/profile/hidden-subjects') body = { hidden: [] }
    else if (path.endsWith('/tokens') || path.endsWith('/agent-tokens')) body = { tokens: [] }
    else if (path === '/api/demo') body = { enabled: false }
    else if (path === '/api/profile/onboarding') body = { onboarded: true, interests: [] }
    else if (path === '/api/notifications') body = { items: [], unread: 0, pendingRequests: 0, pendingRequestItems: [] }
    else if (path === '/api/home/spotlight') body = {
      items: spotlightItems,
      recommendations: child ? [] : BOOKS.map((book, index) => ({
        ...SPOTLIGHT[0], ...book, bookId: null, source: 'discover', ownership: 'discover',
        provider: 'openlibrary', providerKey: `fixture-${index}`, coverId: `fixture-${index}`, cta: 'discover',
      })),
    }
    else if (path === '/api/books/recent') body = BOOKS
    else if (path === '/api/books/continue') body = progress ? [{ book: { ...BOOKS[0], id: 9900, title: SPOTLIGHT[0].title }, percentage: 0.42, updatedAt: 1 }] : []
    else if (path === '/api/books/highlights') body = highlights
    else if (path === '/api/books') body = { items: BOOKS, total: BOOKS.length, letters: [], page: 1, pageSize: 24 }
    else if (path === '/api/home/rails') body = [{ key: 'fiction', title: 'Stories to explore', subtitle: 'From your library', subject: 'fiction', books: BOOKS }]
    else if (path === '/api/home/updates') body = { library: [], discoveries: [], ready: [] }
    else if (path.endsWith('/cover') || path.startsWith('/api/discover/cover/')) {
      const id = Number(path.split('/')[3])
      const index = path.startsWith('/api/discover/') ? Number(path.split('fixture-')[1]) : id >= 9900 ? [0, 7, 6][id - 9900] : id - 9700
      const cover = COVERS[index]
      return route.fulfill(cover
        ? { status: 200, contentType: 'image/svg+xml', body: coverSvg(cover) }
        : { status: 404 })
    }
    return route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(body) })
  })
  return forbidden
}

async function checkGeometry(page) {
  // The test clock advances timers; let native image/layout observers settle too.
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))))
  const failures = await page.evaluate(() => {
    const errors = []
    if (document.documentElement.scrollWidth > innerWidth + 1) errors.push('page overflow')
    for (const stage of document.querySelectorAll('.shelf-cover-stage')) {
      const image = stage.querySelector('img')
      const cover = image ?? stage.querySelector('span')
      if (!cover) continue
      const frame = stage.getBoundingClientRect()
      const box = cover.getBoundingClientRect()
      if (Math.abs(box.bottom - frame.bottom) > 1) errors.push('cover is not sitting on its shelf')
      if (box.width > frame.width + 1 || box.height > frame.height + 1) errors.push('cover exceeds its display area')
      if (image?.naturalWidth) {
        if (getComputedStyle(image).objectFit !== 'contain') errors.push('cover must use contain')
        if (Math.abs(box.width / box.height - image.naturalWidth / image.naturalHeight) > 0.01) errors.push('cover aspect ratio changed')
      }
    }
    for (const track of document.querySelectorAll('.shelf-track')) {
      const shelf = track.querySelector('.shelf-surface').getBoundingClientRect()
      if (Math.abs(shelf.width - track.getBoundingClientRect().width) > 1) errors.push('shelf does not span the scroll content')
      const heroShelf = document.querySelector('.spotlight-display > .shelf-surface')
      if (heroShelf && Math.abs(shelf.height - heroShelf.getBoundingClientRect().height) > 0.1) errors.push('hero and rails use different shelf thickness')
      if (Math.abs(shelf.height - (innerWidth < 640 ? 6 : 8)) > 0.1) errors.push('shelf is not using the thicker band')
    }
    for (const stage of document.querySelectorAll('.spotlight-display')) {
      const cover = stage.querySelector('.spotlight-cover > img, .spotlight-cover > span')
      const frame = stage.getBoundingClientRect()
      const box = cover?.getBoundingClientRect()
      if (box && (Math.abs(box.bottom - frame.bottom) > 1 || box.height > frame.height + 1 || box.left < frame.left - 1 || box.right > frame.right + 1)) errors.push('featured cover exceeds its shelf stage')
      if (cover?.naturalWidth) {
        if (getComputedStyle(cover).objectFit !== 'contain' || Math.abs(box.width / box.height - cover.naturalWidth / cover.naturalHeight) > 0.01) errors.push('featured cover aspect ratio changed')
      }
      const upright = stage.querySelector('.spotlight-upright').getBoundingClientRect()
      if (box && upright.left - box.right < 16) errors.push(`featured cover is crowded against the upright (${innerWidth}px, gap ${upright.left - box.right}px, decorated ${stage.dataset.decorated}, columns ${getComputedStyle(stage).gridTemplateColumns}, padding ${getComputedStyle(stage).paddingRight}, cover ${box.width}px)`)
      const decoration = stage.querySelector('.shelf-decoration')?.getBoundingClientRect()
      if (decoration?.height && Math.abs(decoration.bottom - frame.bottom) > 1) errors.push('decoration is not sitting on its shelf')
      const heading = document.querySelector('.spotlight-heading').getBoundingClientRect()
      const summary = document.querySelector('.spotlight-summary').getBoundingClientRect()
      const footer = document.querySelector('.spotlight-footer').getBoundingClientRect()
      const controls = document.querySelector('.spotlight-controls').getBoundingClientRect()
      const minimumGap = innerWidth >= 375 && innerWidth < 640 ? 12 : 16
      if (footer.top - summary.bottom < minimumGap - 1) errors.push(`summary overlaps the fixed action row at ${innerWidth}px`)
      if (innerWidth >= 375 && summary.bottom > document.querySelector('.spotlight-copy-main').getBoundingClientRect().bottom + 1) errors.push(`excerpt and metadata exceed the space above the button at ${innerWidth}px`)
      if (footer.bottom > controls.top + 1) errors.push('actions overlap the carousel controls')
      if (innerWidth >= 375 && footer.bottom > document.querySelector('.spotlight-copy').getBoundingClientRect().bottom + 1) errors.push(`content exceeds the stable text column at ${innerWidth}px`)
      if (innerWidth < 900) {
        if (heading.right > frame.left + 1) errors.push('featured cover is not to the right of the text')
        if (innerWidth < 375 && (summary.top < frame.bottom - 1 || summary.left > heading.left + 1 || summary.right < frame.right - 1)) errors.push('narrow-phone description is not below both columns')
        if (innerWidth >= 375 && summary.right > frame.left + 1) errors.push('description is not in the text column')
        if (controls.top < frame.bottom + (innerWidth < 640 ? 6 : 8)) errors.push('carousel controls overlap the shelf')
        for (const text of document.querySelectorAll('.spotlight-heading > *')) {
          const box = text.getBoundingClientRect()
          if (box.top < heading.top - 1 || box.bottom > heading.bottom + 1) errors.push('heading text exceeds the space beside the cover')
        }
      }
    }
    return errors
  })
  expect(failures.length === 0, `shelf geometry: ${failures.join(', ')}`)
}

async function spotlightFrame(hero) {
  return hero.evaluate((element) => {
    const frame = element.getBoundingClientRect()
    const button = element.querySelector('.spotlight-actions > a').getBoundingClientRect()
    return {
      height: frame.height,
      shelf: element.querySelector('.spotlight-display').getBoundingClientRect().top - frame.top,
      controls: element.querySelector('.spotlight-controls').getBoundingClientRect().top - frame.top,
      buttonX: button.left - frame.left,
      buttonY: button.top - frame.top,
    }
  })
}

function expectStableFrame(before, after, context) {
  for (const key of Object.keys(before)) {
    expect(Math.abs(before[key] - after[key]) < 1, `${key} stays stable ${context}`)
  }
}

async function checkAccessibility(page) {
  await page.evaluate(axe.source)
  const violations = await page.evaluate(async () => {
    const results = await window.axe.run(document, { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21aa', 'wcag22aa', 'best-practice'] } })
    return results.violations.map(({ id, nodes }) => ({ id, targets: nodes.map(({ target }) => target) }))
  })
  expect(violations.length === 0, `Home accessibility: ${JSON.stringify(violations)}`)
}

export default async function homeDesign(page, { base }) {
  await page.addInitScript(() => localStorage.setItem('bokhylle.theme', 'paper'))
  await page.clock.install()
  await mockHomeDesign(page, { highlights: BOOKS.slice(1, 4) })
  await page.goto(base, { waitUntil: 'networkidle' })
  const hero = page.getByRole('region', { name: 'Spotlight', exact: true })
  await hero.getByRole('heading', { name: SPOTLIGHT[0].title, exact: true }).waitFor()
  expect(await hero.getByText(/staff[ _-]+picks/i).count() === 0, 'provider staff-picks tags are hidden in Spotlight')
  expect(await hero.getByText('Literature', { exact: true }).count() === 1, 'useful subject metadata remains visible')
  expect(await hero.getByText('RU', { exact: true }).count() === 0, 'a multilingual work shows the preferred English language rather than its sampled Russian language')
  expect(await page.getByRole('heading', { name: 'Continue reading', exact: true }).count() === 0, 'no reported progress means no Continue reading section')
  const rediscover = page.getByRole('region', { name: 'Rediscover your library books' })
  await rediscover.waitFor()
  expect(await rediscover.locator('.shelf-book').count() === 3, 'Rediscover keeps its three curated books on the shared shelf')
  expect(await rediscover.locator('.shelf-surface').count() === 1, 'Rediscover has one continuous shelf line')
  expect(await hero.getByRole('link', { name: 'Spotlight appearance and automatic rotation settings' }).count() === 0, 'Spotlight has no options shortcut')
  expect(await page.getByText(/Good morning|Good afternoon|Good evening/).count() === 0, 'Home starts with Spotlight instead of a greeting row')

  await page.mouse.move(0, 0)
  await hero.locator('[aria-live="off"]').waitFor()
  await page.evaluate(() => document.fonts.ready)
  const firstFrame = await spotlightFrame(hero)
  await page.clock.runFor(10_100)
  await hero.getByRole('heading', { name: SPOTLIGHT[1].title, exact: true }).waitFor()
  await checkGeometry(page)
  expectStableFrame(firstFrame, await spotlightFrame(hero), 'during automatic rotation')
  expect(await hero.getByRole('button', { name: /Play spotlight|Pause spotlight/ }).count() === 0, 'carousel has no player controls')
  const dimensions = await hero.boundingBox()
  const railY = (await page.getByRole('region', { name: 'Picked for you books', exact: true }).boundingBox()).y
  await hero.getByRole('button', { name: 'Next spotlight book' }).click()
  await hero.getByRole('heading', { name: SPOTLIGHT[2].title, exact: true }).waitFor()
  expect(Math.abs((await hero.boundingBox()).height - dimensions.height) < 1, 'long titles, landscape covers and missing covers must keep Spotlight height stable')
  expect(Math.abs((await page.getByRole('region', { name: 'Picked for you books', exact: true }).boundingBox()).y - railY) < 1, 'changing Spotlight must not move the rails')
  await page.mouse.move(0, 0)
  await hero.locator('[aria-live="off"]').waitFor()
  await page.clock.runFor(9_900)
  expect(await hero.getByRole('heading', { name: SPOTLIGHT[2].title, exact: true }).count() === 1, 'manual navigation starts a fresh interval')
  await page.clock.runFor(200)
  await hero.getByRole('heading', { name: SPOTLIGHT[0].title, exact: true }).waitFor()

  await page.mouse.move(dimensions.x + 10, dimensions.y + 10)
  await hero.locator('[aria-live="polite"]').waitFor()
  await page.clock.runFor(20_100)
  expect(await hero.getByRole('heading', { name: SPOTLIGHT[0].title, exact: true }).count() === 1, 'hover pauses automatic rotation')
  await page.mouse.move(0, 0)
  await hero.locator('[aria-live="off"]').waitFor()
  await page.clock.runFor(10_100)
  await hero.getByRole('heading', { name: SPOTLIGHT[1].title, exact: true }).waitFor()

  await page.keyboard.press('Tab')
  await hero.getByRole('link', { name: 'Explore book', exact: true }).focus()
  await hero.locator('[aria-live="polite"]').waitFor()
  await page.clock.runFor(20_100)
  expect(await hero.getByRole('heading', { name: SPOTLIGHT[1].title, exact: true }).count() === 1, 'keyboard focus pauses automatic rotation')
  await page.getByRole('button', { name: 'Account menu', exact: true }).focus()
  await hero.locator('[aria-live="off"]').waitFor()
  await page.clock.runFor(10_100)
  await hero.getByRole('heading', { name: SPOTLIGHT[2].title, exact: true }).waitFor()

  await page.evaluate(() => {
    const dialog = document.createElement('div')
    dialog.setAttribute('role', 'dialog')
    dialog.id = 'test-open-dialog'
    document.body.append(dialog)
  })
  await hero.locator('[aria-live="polite"]').waitFor()
  await page.clock.runFor(20_100)
  expect(await hero.getByRole('heading', { name: SPOTLIGHT[2].title, exact: true }).count() === 1, 'an open dialog pauses the Spotlight behind it')
  await page.evaluate(() => document.getElementById('test-open-dialog').remove())
  await hero.locator('[aria-live="off"]').waitFor()
  await page.clock.runFor(10_100)
  await hero.getByRole('heading', { name: SPOTLIGHT[0].title, exact: true }).waitFor()

  await page.getByRole('heading', { name: 'Stories to explore', exact: true }).scrollIntoViewIfNeeded()
  await hero.locator('[aria-live="polite"]').waitFor()
  await page.clock.runFor(20_100)
  expect(await hero.getByRole('heading', { name: SPOTLIGHT[0].title, exact: true }).count() === 1, 'Spotlight does not advance while scrolled out of view')
  await page.evaluate(() => window.scrollTo(0, 0))
  await hero.locator('[aria-live="off"]').waitFor()
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await hero.locator('[aria-live="polite"]').waitFor()
  await page.clock.runFor(20_100)
  expect(await hero.getByRole('heading', { name: SPOTLIGHT[0].title, exact: true }).count() === 1, 'reduced motion stops a running automatic carousel')
  await hero.getByRole('button', { name: 'Next spotlight book' }).click()
  await hero.locator('.spotlight-display[data-decorated="false"]').waitFor()
  await checkGeometry(page)
  expect(await hero.locator('.shelf-decoration').count() === 0, 'a landscape cover gets the space reserved for decoration')
  await hero.getByRole('button', { name: 'Next spotlight book' }).click()
  await hero.locator('.spotlight-cover > span').waitFor()
  await checkGeometry(page)
  await hero.getByRole('button', { name: 'Next spotlight book' }).click()
  for (const theme of ['paper', 'ink']) {
    await page.evaluate((value) => { document.documentElement.dataset.theme = value }, theme)
    for (const width of [320, 375, 390, 768, 1440]) {
      await page.setViewportSize({ width, height: 1000 })
      await page.clock.runFor(100)
      // Bring lazy-loaded artwork into view before checking its dimensions.
      await page.getByRole('heading', { name: 'Recently Added', exact: true }).scrollIntoViewIfNeeded()
      await page.clock.runFor(100)
      await checkGeometry(page)
      await checkAccessibility(page)
      const frame = await spotlightFrame(hero)
      for (const item of [SPOTLIGHT[1], SPOTLIGHT[2], SPOTLIGHT[0]]) {
        await hero.getByRole('button', { name: 'Next spotlight book' }).click()
        await hero.getByRole('heading', { name: item.title, exact: true }).waitFor()
        await page.clock.runFor(100)
        await checkGeometry(page)
        expectStableFrame(frame, await spotlightFrame(hero), `at ${width}px with long titles and unusual covers`)
      }
    }
  }

  // Unowned books retain their discovery action without an ownership label or
  // empty metadata row. Household ownership still carries useful information.
  await page.unroute('**/api/**')
  await mockHomeDesign(page, { spotlightItems: [
    { ...SPOTLIGHT[0], bookId: null, title: 'A short story', blurb: 'A young reader finds a lost book and follows its story back home.', subjects: [], source: 'discover', ownership: 'discover', cta: 'discover', provider: 'openlibrary', providerKey: 'fixture-4', coverId: 'fixture-4' },
    { ...SPOTLIGHT[1], source: 'household', ownership: 'household' },
  ] })
  await page.setViewportSize({ width: 375, height: 844 })
  await page.reload({ waitUntil: 'networkidle' })
  await hero.getByRole('heading', { name: 'A short story', exact: true }).waitFor()
  expect(await hero.getByText('Not in your library', { exact: true }).count() === 0, 'unowned Spotlight books have no ownership label')
  expect(await hero.locator('.spotlight-metadata').count() === 0, 'missing metadata does not reserve an empty row')
  const availability = hero.getByRole('link', { name: 'Check availability', exact: true })
  expect((await availability.getAttribute('href')).includes('providerKey=fixture-4'), 'the discovery action keeps its provider identity')
  await checkGeometry(page)
  const stableFrame = await spotlightFrame(hero)
  await hero.getByRole('button', { name: 'Next spotlight book' }).click()
  await hero.getByText('In household library', { exact: true }).waitFor()
  await checkGeometry(page)
  expectStableFrame(stableFrame, await spotlightFrame(hero), 'when short copy changes to a long title and household label')

  // A synced book keeps its progress even when that same book is in Spotlight.
  await page.unroute('**/api/**')
  const forbidden = await mockHomeDesign(page, { progress: true, child: true })
  await page.setViewportSize({ width: 390, height: 844 })
  await page.reload({ waitUntil: 'networkidle' })
  await page.getByRole('heading', { name: 'Continue reading', exact: true }).waitFor()
  await page.getByRole('progressbar', { name: 'KOReader reading progress', exact: true }).waitFor()
  expect(await page.getByRole('progressbar', { name: 'KOReader reading progress', exact: true }).getAttribute('aria-valuenow') === '42', 'synced progress remains 42% when the book is also in Spotlight')
  expect(await page.locator('a[href="/discover"], a[href="/activity"]').count() === 0, 'children retain their restricted navigation')
  expect(await page.getByRole('heading', { name: 'Picked for you', exact: true }).count() === 0, 'children do not receive discovery recommendations')
  expect(forbidden.length === 0, `child Home must not fetch adult sections: ${forbidden.join(', ')}`)
  expect(await hero.getByRole('link', { name: 'Spotlight appearance and automatic rotation settings' }).count() === 0, 'children are not offered a restricted profile route')
  await checkGeometry(page)

  const touchContext = await page.context().browser().newContext({ viewport: { width: 390, height: 844 }, hasTouch: true })
  try {
    const touchPage = await touchContext.newPage()
    await touchPage.clock.install()
    await mockHomeDesign(touchPage)
    await touchPage.goto(base, { waitUntil: 'networkidle' })
    const touchHero = touchPage.getByRole('region', { name: 'Spotlight', exact: true })
    await touchHero.locator('[aria-live="off"]').waitFor()
    await touchHero.getByRole('button', { name: 'Next spotlight book' }).tap()
    await touchHero.getByRole('heading', { name: SPOTLIGHT[1].title, exact: true }).waitFor()
    await touchHero.locator('[aria-live="off"]').waitFor()
    await touchPage.clock.runFor(10_100)
    await touchHero.getByRole('heading', { name: SPOTLIGHT[2].title, exact: true }).waitFor()
    await checkGeometry(touchPage)
  } finally {
    await touchContext.close()
  }
}

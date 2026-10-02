import axe from 'axe-core'
import { expect } from '../support.mjs'
import { mockHomeDesign } from './home-design.mjs'

export default async function bookSharing(page, { base }) {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await mockHomeDesign(page, { appearance: { defaultBookSharing: 'shared', canAcquire: true } })
  await page.route('**/api/books/facets*', (route) => route.fulfill({ contentType: 'application/json', body: JSON.stringify({ formats: [], languages: [], series: [], subjects: [], publicationKinds: [{ value: 'book', count: 8 }] }) }))
  const writes = []
  let failBulk = true
  let book = {
    id: 9700, title: 'The lantern library', authors: ['Fictional Reader'], authorRefs: [],
    language: 'en', availableLanguages: [], description: 'A fictional book for testing sharing.',
    publicationYear: 2020, series: null, seriesNumber: null, rating: null, ratingCount: null,
    subjects: [], editions: [], files: [], onShelf: true, sharing: 'shared', sharedInHousehold: true,
    metadataSources: [], preference: null, hasCover: false,
  }
  await page.route('**/api/books/9700', (route) => route.fulfill({ contentType: 'application/json', body: JSON.stringify(book) }))
  await page.route('**/api/books/9700/related', (route) => route.fulfill({ contentType: 'application/json', body: JSON.stringify({ series: [], author: [], similar: [] }) }))
  await page.route('**/api/books/9700/sharing', (route) => {
    const input = route.request().postDataJSON()
    writes.push(input)
    book = { ...book, sharing: input.sharing, sharedInHousehold: input.sharing === 'shared' }
    return route.fulfill({ contentType: 'application/json', body: JSON.stringify({ sharing: book.sharing, sharedInHousehold: book.sharedInHousehold }) })
  })
  await page.route('**/api/books/sharing', (route) => {
    const input = route.request().postDataJSON()
    writes.push(input)
    if (failBulk) { failBulk = false; return route.fulfill({ status: 503, contentType: 'application/json', body: JSON.stringify({ code: 'unavailable', message: 'Sharing temporarily unavailable' }) }) }
    return route.fulfill({ status: 204 })
  })
  await page.goto(`${base}/profile/preferences`, { waitUntil: 'networkidle' })
  await page.getByLabel('Default for new books').selectOption('private')
  await page.getByRole('button', { name: 'Save sharing default' }).click()
  await page.getByRole('status').getByText(/Default saved/).waitFor()
  await page.reload({ waitUntil: 'networkidle' })
  expect(await page.getByLabel('Default for new books').inputValue() === 'private', 'sharing default survives reloading')

  await page.goto(`${base}/library/9700`, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: 'Shared · Change', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: 'Book sharing', exact: true })
  await dialog.getByRole('combobox', { name: /^Book sharing/ }).selectOption('private')
  await dialog.getByRole('button', { name: 'Save sharing', exact: true }).click()
  await page.getByRole('button', { name: 'Private · Change', exact: true }).waitFor()
  expect(writes[0].sharing === 'private', 'one book sends its explicit sharing choice')

  await page.goto(`${base}/library`, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: 'Select books', exact: true }).click()
  await page.getByRole('button', { name: 'Select visible books', exact: true }).click()
  expect(await page.getByRole('checkbox', { name: /^Select / }).count() === 8, 'bulk selection shows only shelf books')
  await page.getByRole('button', { name: 'Make private', exact: true }).click()
  await page.getByRole('alert').getByText('Sharing temporarily unavailable').waitFor()
  expect((await page.getByRole('checkbox', { name: /^Select / }).evaluateAll((inputs) => inputs.filter((input) => input.checked).length)) === 8, 'failed bulk save keeps the selected books')
  await page.getByRole('button', { name: 'Make private', exact: true }).click()
  await page.getByRole('status').getByText(/Selected books are now private/).waitFor()
  expect(writes.at(-1).bookIds.length === 8 && writes.at(-1).sharing === 'private', 'bulk sharing sends selected ids and choice')
  for (const theme of ['paper', 'ink']) {
    await page.evaluate((value) => { document.documentElement.dataset.theme = value }, theme)
    for (const width of [320, 390, 1440]) {
      await page.setViewportSize({ width, height: 1000 })
      await page.clock.runFor(200)
      await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))))
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), 'sharing controls fit the viewport')
      const geometry = await page.locator('.shelf-grid-cell').first().evaluate((cell) => {
        const shelf = cell.querySelector('.shelf-surface').getBoundingClientRect()
        const cover = cell.querySelector('.shelf-cover-stage').getBoundingClientRect()
        return Math.abs(shelf.top - cover.bottom)
      })
      expect(geometry < 2, 'selection controls keep the covers resting on the shelf line')
      await page.evaluate(axe.source)
      const violations = await page.evaluate(async () => (await window.axe.run(document, { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21aa', 'wcag22aa'] } })).violations.map(({ id, nodes }) => ({ id, nodes: nodes.map(({ html, failureSummary }) => ({ html, failureSummary })) })))
      expect(violations.length === 0, `Sharing accessibility (${theme}, ${width}): ${JSON.stringify(violations)}`)
    }
  }
}

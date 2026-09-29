import { expect } from '../support.mjs'

export default async function catalogues(page, { base }) {
  let acquired = null
  let sources = []
  await page.route('**/api/catalogues**', (route) => {
    const request = route.request()
    const url = new URL(request.url())
    const json = (body, status = 200) => route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) })
    if (url.pathname.endsWith('/acquisitions')) {
      acquired = request.postDataJSON()
      return json({ id: 'fictional-http', bookId: 1, status: 'REQUESTED', duplicate: false }, 202)
    }
    if (url.pathname.endsWith('/feed')) return json({ title: 'Open shelf', pageUrl: 'https://catalogue.example.test/opds', navigation: [], next: null, searchAvailable: true, entries: [{ id: 'paper-moon', title: 'The Paper Moon', authors: ['Mira Vale'], language: 'en', files: [{ index: 0, format: 'epub', label: 'Open access EPUB' }] }] })
    if (request.method() === 'POST') {
      sources = [{ id: 'fixture', ...request.postDataJSON() }]
      return json(sources[0], 201)
    }
    return json(sources)
  })
  await page.goto(`${base}/catalogues`, { waitUntil: 'networkidle' })
  await page.getByRole('heading', { name: 'Browse book catalogues' }).waitFor()
  await page.getByRole('textbox', { name: 'Name', exact: true }).fill('Open shelf')
  await page.getByRole('textbox', { name: 'OPDS feed URL' }).fill('https://catalogue.example.test/opds')
  await page.getByRole('button', { name: 'Add catalogue' }).click()
  await page.getByRole('heading', { name: 'The Paper Moon' }).waitFor()
  await page.getByRole('textbox', { name: 'Search this catalogue' }).fill('Paper Moon')
  await page.getByRole('button', { name: 'Search', exact: true }).click()
  await page.getByRole('button', { name: 'Get EPUB' }).click()
  await page.waitForURL(`${base}/activity`)
  expect(acquired.entryId === 'paper-moon' && acquired.fileIndex === 0, 'catalogue acquisition uses the chosen entry and format')
}

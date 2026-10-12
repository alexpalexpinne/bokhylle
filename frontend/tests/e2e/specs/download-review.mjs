import { expect } from '../support.mjs'
import { check } from './accessibility.mjs'
import { mockVersionChoices } from './version-choices.mjs'

export default async function downloadReview(page, { base }) {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  const state = await mockVersionChoices(page)
  const acquisition = state.newAcquisition({ status: 'NEEDS_REVIEW', errorCode: 'multiple_candidates', errorMessage: 'Several possible books were found in the download' })
  state.acquisitions = [acquisition]
  const candidates = ['epub', 'pdf'].map((format, index) => ({
    path: `/downloads/maps/Where Maps End - Nora Vale - Illustrated Coastline Edition.${format}`,
    format, size: 420000 + index, score: 60, confidence: 0.7,
    reasons: ['strong title match', 'author match'], title: 'Where Maps End', authors: ['Nora Vale'], isbn: null,
  }))
  let failChoose = true
  const writes = []
  await page.route('**/api/admin/attention', (route) => route.fulfill({ json: { items: [], count: 0 } }))
  await page.route('**/api/acquisitions/maps-version/review**', (route) => {
    const request = route.request()
    if (request.method() === 'GET') return route.fulfill({ json: { contentPath: '/downloads/maps', reason: 'multiple_candidates', candidates } })
    if (failChoose) {
      failChoose = false
      return route.fulfill({ status: 503, json: { code: 'unavailable', message: 'Could not import that file. Try again.' } })
    }
    writes.push(request.postDataJSON())
    acquisition.status = 'READY'
    return route.fulfill({ status: 202, json: { id: acquisition.id, status: acquisition.status } })
  })

  await page.goto(`${base}/activity`, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: 'Review files', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: 'Review download', exact: true })
  await dialog.getByText(/Several files could match this book/).waitFor()
  expect(await dialog.getByRole('button', { name: 'Import this file', exact: true }).count() === 2, 'adult requesters can pick from their downloaded files')
  expect(await page.getByText('An administrator needs to review this download.', { exact: true }).count() === 0, 'adult owners are offered the choice directly')
  for (const theme of ['paper', 'ink']) {
    await page.evaluate((value) => { document.documentElement.dataset.theme = value }, theme)
    for (const width of [320, 390, 1440]) {
      await page.setViewportSize({ width, height: 844 })
      await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))))
      expect(await dialog.evaluate((element) => element.scrollWidth <= element.clientWidth + 1), 'file choices fit the dialog')
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), 'download review fits the viewport')
      if (width < 640) {
        const button = await dialog.getByRole('button', { name: 'Import this file', exact: true }).first().boundingBox()
        expect(button.height >= 48, 'mobile file choices have usable touch targets')
      }
      await check(page, `Download review (${theme}, ${width})`)
    }
  }
  await dialog.getByRole('button', { name: 'Import this file', exact: true }).nth(1).click()
  await dialog.getByText('Could not import that file. Try again.', { exact: true }).waitFor()
  expect(writes.length === 0, 'a failed import preserves the choices for retry')
  await dialog.getByRole('button', { name: 'Import this file', exact: true }).nth(1).click()
  await dialog.waitFor({ state: 'detached' })
  expect(writes.length === 1 && writes[0].action === 'choose' && writes[0].path === candidates[1].path, 'the adult imports exactly the selected file')

  acquisition.status = 'NEEDS_REVIEW'
  acquisition.requestedByUserId = 99
  acquisition.requestedBy = 'Robin'
  acquisition.requestedByMe = true
  await page.goto(`${base}/activity`, { waitUntil: 'networkidle' })
  await page.getByText('Waiting for Robin or an administrator to review the downloaded files.', { exact: true }).waitFor()
  expect(await page.getByRole('button', { name: 'Review files', exact: true }).count() === 0, 'joining a shared download does not replace its original requester’s choice')

  acquisition.requestedByUserId = state.user.id
  state.user.canAcquire = false
  await page.reload({ waitUntil: 'networkidle' })
  await page.getByText('An administrator needs to review this download.', { exact: true }).waitFor()
  expect(await page.getByRole('button', { name: 'Review files', exact: true }).count() === 0, 'adults needing acquisition approval still need administrator review')

  state.user.role = 'admin'
  acquisition.requestedByUserId = 99
  await page.reload({ waitUntil: 'networkidle' })
  await page.getByRole('button', { name: 'Review files', exact: true }).click()
  await dialog.getByRole('button', { name: 'Import this file', exact: true }).first().waitFor()
  expect(writes.length === 1, 'administrators retain review access without automatically importing anything')
}

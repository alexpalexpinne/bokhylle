import axe from 'axe-core'
import { expect } from '../support.mjs'
import { mockVersionChoices } from './version-choices.mjs'

export default async function discoverActions(page, { base }) {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  const state = await mockVersionChoices(page)
  const url = `${base}/discover?provider=openlibrary&providerKey=%2Fworks%2FFICTIONAL`
  for (const reader of [false, true]) {
    state.targets = reader ? [{ id: 501, address: 'mira@reader.example', enabled: true, isDefault: true, kind: 'kindle' }] : []
    await page.goto(url, { waitUntil: 'networkidle' })
    const choices = page.getByRole('group', { name: 'Book choices', exact: true })
    const shelf = choices.getByRole('button', { name: 'Get for my shelf', exact: true })
    const send = choices.getByRole('button', { name: reader ? 'Get & Send to My Reader' : 'Get & send…', exact: true })
    await send.waitFor()
    for (const theme of ['paper', 'ink']) {
      await page.evaluate((value) => { document.documentElement.dataset.theme = value }, theme)
      for (const width of [320, 390]) {
        await page.setViewportSize({ width, height: 844 })
        await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))))
        const [group, shelfBox, sendBox, likeBox, rejectBox] = await Promise.all([
          choices.boundingBox(), shelf.boundingBox(), send.boundingBox(),
          choices.getByRole('button', { name: 'Like', exact: true }).boundingBox(),
          choices.getByRole('button', { name: 'Not for me', exact: true }).boundingBox(),
        ])
        expect(Math.abs(shelfBox.width - group.width) < 1 && Math.abs(sendBox.width - group.width) < 1, `both acquisition actions fill the row (${reader}, ${theme}, ${width})`)
        expect(Math.abs(shelfBox.x - sendBox.x) < 1 && Math.abs(shelfBox.y - sendBox.y) >= 48, 'acquisition actions stack with aligned edges')
        expect(Math.abs(likeBox.width - rejectBox.width) < 1 && Math.abs(likeBox.y - rejectBox.y) < 1, 'taste actions share an equal-width row')
        expect(likeBox.y >= Math.max(shelfBox.y + shelfBox.height, sendBox.y + sendBox.height), 'taste actions follow both acquisition actions')
        expect([shelfBox, sendBox, likeBox, rejectBox].every((box) => box.height >= 48), 'mobile actions have 48px touch targets')
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), 'book actions fit narrow phones')
        await page.evaluate(axe.source)
        const violations = await page.evaluate(async () => (await window.axe.run(document, { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21aa', 'wcag22aa'] } })).violations.map(({ id }) => id))
        expect(violations.length === 0, `Book actions accessibility (${theme}, ${width}): ${violations}`)
      }
    }
    expect(state.writes.length === 0, 'checking book actions starts no acquisition')
  }

  state.user.canAcquire = false
  await page.goto(url, { waitUntil: 'networkidle' })
  const choices = page.getByRole('group', { name: 'Book choices', exact: true })
  await choices.getByRole('button', { name: 'Ask to add', exact: true }).waitFor()
  expect(await choices.getByRole('button', { name: /send/i }).count() === 0, 'readers needing approval only see the request action')
  const [group, ask] = await Promise.all([choices.boundingBox(), choices.getByRole('button', { name: 'Ask to add', exact: true }).boundingBox()])
  expect(Math.abs(group.width - ask.width) < 1, 'the request action also fills the mobile row')

  state.user.profileType = 'child'
  state.user.canDiscover = true
  state.user.canRequest = true
  await page.route('**/api/requests/book**', (route) => route.fulfill({ json: {
    ...state.book, provider: 'openlibrary', providerKey: '/works/FICTIONAL', coverId: null, year: 2024,
  } }))
  await page.goto(url, { waitUntil: 'networkidle' })
  await page.getByRole('button', { name: 'Ask an adult', exact: true }).waitFor()
  expect(await page.getByRole('button', { name: /Get.*shelf|send/i }).count() === 0, 'children keep their own request flow')
}

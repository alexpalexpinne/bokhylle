import axe from 'axe-core'
import { expect } from '../support.mjs'
import { mockHomeDesign } from './home-design.mjs'

export default async function profileAppearance(page, { base }) {
  await page.addInitScript(() => localStorage.setItem('bokhylle.theme', 'paper'))
  await page.clock.install()
  await mockHomeDesign(page)
  await page.goto(base + '/profile/appearance', { waitUntil: 'networkidle' })
  const metal = page.getByRole('radio', { name: /Muted metal/ })
  await metal.check()
  await page.getByRole('checkbox', { name: /Decorate the featured shelf/ }).uncheck()
  await page.getByRole('checkbox', { name: /Automatically rotate Spotlight/ }).uncheck()
  expect(await page.locator('.shelf-preview .shelf-decoration').count() === 0, 'decoration setting updates the preview')
  expect(await page.locator('.shelf-preview').getAttribute('data-shelf-finish') === 'metal', 'finish updates the preview before saving')
  expect(await page.locator('[data-shelf-finish]').first().getAttribute('data-shelf-finish') === 'oak', 'draft changes do not overwrite saved appearance')
  await page.getByRole('link', { name: 'Home', exact: true }).first().click()
  const dialog = page.getByRole('dialog', { name: 'Discard unsaved appearance?' })
  await dialog.waitFor()
  await dialog.getByRole('button', { name: 'Keep editing' }).click()
  expect(page.url().endsWith('/profile/appearance'), 'keeping edits retains the current section')

  const fail = (route) => route.fulfill({ status: 503, contentType: 'application/json', body: JSON.stringify({ error: { code: 'unavailable', message: 'Could not save appearance' } }) })
  await page.route('**/api/profile', fail)
  await page.getByRole('button', { name: 'Save appearance' }).click()
  await page.getByRole('alert').waitFor()
  expect(await metal.isChecked(), 'a failed save keeps the draft')
  expect(await page.locator('[data-shelf-finish]').first().getAttribute('data-shelf-finish') === 'oak', 'a failed save keeps the saved appearance')
  await page.unroute('**/api/profile', fail)
  await page.getByRole('button', { name: 'Save appearance' }).click()
  await page.getByRole('status').getByText(/Appearance saved/).waitFor()
  await page.reload({ waitUntil: 'networkidle' })
  expect(await metal.isChecked(), 'the saved finish survives reloading')
  expect(!(await page.getByRole('checkbox', { name: /Automatically rotate Spotlight/ }).isChecked()), 'rotation preference survives reloading')
  await page.getByRole('link', { name: 'Home', exact: true }).first().click()
  const hero = page.getByRole('region', { name: 'Spotlight', exact: true })
  await hero.waitFor()
  const title = await hero.locator('h2').textContent()
  await page.mouse.move(0, 0)
  await page.clock.runFor(20_100)
  expect(await hero.locator('h2').textContent() === title, 'the saved rotation preference stops automatic changes')
  expect(await hero.locator('.shelf-decoration').count() === 0, 'the saved decoration preference applies to Home')
  expect(await page.locator('[data-shelf-finish]').first().getAttribute('data-shelf-finish') === 'metal', 'the saved finish applies to all shelves')

  await page.getByRole('button', { name: 'Account menu', exact: true }).click()
  await page.getByRole('link', { name: 'Profile', exact: true }).click()
  await page.getByRole('link', { name: 'Appearance', exact: true }).click()
  await page.getByRole('radio', { name: /Faded black/ }).check()
  await page.getByRole('checkbox', { name: /Decorate the featured shelf/ }).check()
  await page.getByRole('checkbox', { name: /Automatically rotate Spotlight/ }).check()
  await page.getByRole('button', { name: 'Save appearance' }).click()
  await page.getByRole('status').getByText(/Appearance saved/).waitFor()
  for (const theme of ['paper', 'ink']) {
    await page.evaluate((value) => { document.documentElement.dataset.theme = value }, theme)
    for (const width of [320, 390, 1440]) {
      await page.setViewportSize({ width, height: 1000 })
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), 'Appearance fits the viewport')
      await page.evaluate(axe.source)
      const violations = await page.evaluate(async () => (await window.axe.run(document, { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21aa', 'wcag22aa'] } })).violations.map(({ id }) => id))
      expect(violations.length === 0, `Appearance accessibility: ${violations.join(', ')}`)
    }
  }
}

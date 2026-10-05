import { expect } from '../support.mjs'
import { mockHomeDesign } from './home-design.mjs'

async function checkTheme(page, choice) {
  const actual = await page.evaluate(() => ({
    scheme: getComputedStyle(document.documentElement).colorScheme,
    canvas: getComputedStyle(document.body).backgroundColor,
    chrome: document.querySelector('meta[name="theme-color"]').content,
  }))
  const color = choice === 'ink' ? '#1a1511' : '#f7f4ee'
  expect(actual.scheme === (choice === 'ink' ? 'dark' : 'light'), `${choice}: native controls follow the theme`)
  expect(actual.chrome === color, `${choice}: browser chrome uses the canvas color`)
  expect(actual.canvas === (choice === 'ink' ? 'rgb(26, 21, 17)' : 'rgb(247, 244, 238)'), `${choice}: the page uses the canvas color`)
}

/** Paper and Ink are expressions of one theme, selectable and remembered. */
export default async function theme(page, { base }) {
  await mockHomeDesign(page, { appearance: { spotlightRotation: false } })
  await page.goto(base, { waitUntil: 'networkidle' })
  for (const choice of ['paper', 'ink']) {
    await page.evaluate((value) => localStorage.setItem('bokhylle.theme', value), choice)
    await page.reload({ waitUntil: 'networkidle' })
    const applied = await page.evaluate(() => document.documentElement.dataset.theme)
    expect(applied === choice, `data-theme should be ${choice}, got ${applied ?? 'none'}`)
    await checkTheme(page, choice)
  }

  // Exercise the real choice controls with fictional profile data, including
  // system changes while the app is open and a manual override after reload.
  await page.evaluate(() => localStorage.setItem('bokhylle.theme', 'system'))
  await page.emulateMedia({ colorScheme: 'light' })
  await page.reload({ waitUntil: 'networkidle' })
  await page.getByRole('button', { name: 'Account menu', exact: true }).click()
  await checkTheme(page, 'paper')
  await page.emulateMedia({ colorScheme: 'dark' })
  await page.waitForFunction(() => document.querySelector('meta[name="theme-color"]').content === '#1a1511')
  await checkTheme(page, 'ink')
  await page.getByRole('button', { name: 'Paper', exact: true }).click()
  await checkTheme(page, 'paper')
  const stored = await page.evaluate(() => localStorage.getItem('bokhylle.theme'))
  expect(stored === 'paper', 'a manual choice is remembered')
  await page.reload({ waitUntil: 'networkidle' })
  await checkTheme(page, 'paper')
  await page.getByRole('button', { name: 'Account menu', exact: true }).click()
  await page.getByRole('button', { name: 'System', exact: true }).click()
  await checkTheme(page, 'ink')
  await page.keyboard.press('Escape')
  expect(await page.getByRole('button', { name: 'Account menu', exact: true }).evaluate((button) => button === document.activeElement), 'Escape returns focus to the account trigger')
}

import { expect } from '../support.mjs'

/** Paper and Ink are expressions of one theme, selectable and remembered. */
export default async function theme(page, { base }) {
  for (const choice of ['paper', 'ink']) {
    await page.addInitScript((value) => {
      try {
        localStorage.setItem('bokhylle.theme', value)
      } catch {
        // Ignore storage restrictions.
      }
    }, choice)
    await page.goto(`${base}/login`, { waitUntil: 'networkidle' })
    const applied = await page.evaluate(() => document.documentElement.dataset.theme)
    expect(applied === choice, `data-theme should be ${choice}, got ${applied ?? 'none'}`)
  }
}

export async function login(page, base, username, password) {
  await page.goto(`${base}/login`, { waitUntil: 'networkidle' })
  const picker = page.getByRole('button', { name: new RegExp(username, 'i') }).first()
  if (await picker.isVisible().catch(() => false)) {
    await picker.click()
    await page.waitForTimeout(300)
  } else {
    const usernameOption = page.getByRole('button', { name: 'Sign in with a username instead' })
    if (await usernameOption.isVisible().catch(() => false)) await usernameOption.click()
  }
  await page.fill('input[autocomplete="username"]', username)
  await page.fill('input[autocomplete="current-password"]', password)
  await page.click('button[type="submit"]')
  await page.waitForURL(`${base}/`)
}

export function expect(condition, message) {
  if (!condition) {
    throw new Error(message)
  }
}

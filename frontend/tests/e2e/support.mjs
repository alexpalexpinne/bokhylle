export async function login(page, base, username, password) {
  await page.goto(`${base}/login`, { waitUntil: 'networkidle' })
  const picker = page.locator(`button[data-profile-username=${JSON.stringify(username)}]`)
  if (await picker.isVisible().catch(() => false)) {
    await picker.click()
  } else {
    const usernameOption = page.getByRole('button', { name: 'Sign in another way' })
    if (await usernameOption.isVisible().catch(() => false)) await usernameOption.click()
    await page.fill('input[autocomplete="username"]', username)
  }
  await page.fill('input[autocomplete="current-password"]', password)
  await page.click('button[type="submit"]')
  await page.waitForURL(`${base}/`)
}

export function expect(condition, message) {
  if (!condition) {
    throw new Error(message)
  }
}

import axe from 'axe-core'
import { expect } from '../support.mjs'
import { mockVersionChoices } from './version-choices.mjs'

export default async function notifications(page, { base }) {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await page.clock.install()
  const fixture = await mockVersionChoices(page)
  fixture.user.role = 'admin'
  await page.route('**/api/books**', (route) => {
    const path = new URL(route.request().url()).pathname
    if (path === '/api/books') return route.fulfill({ json: { items: [], total: 0, page: 1, pageSize: 24, letters: [] } })
    if (path === '/api/books/facets') return route.fulfill({ json: { formats: [], languages: [], subjects: [], series: [], publicationKinds: [] } })
    return route.fallback()
  })
  const pending = { id: 7, bookId: 77, title: 'Where Maps End', authors: ['Nora Vale'], requester: 'Robin',
    requesterUserId: 89, status: 'requested', phase: 'requested', acquisitionId: null, keepLooking: false, createdAt: 1768471200, updatedAt: 1768471200 }
  let items = [
    { id: 2, kind: 'request', title: 'Robin asked for Where Maps End', body: 'Please add this fictional book.', read: true, bookId: 77, acquisitionId: null, createdAt: Math.floor(Date.now() / 1000) },
    { id: 1, kind: 'ready', title: 'Where Maps End is ready', body: 'Your fictional book is on your shelf.', read: false, bookId: 77, acquisitionId: 'maps-version', createdAt: Math.floor(Date.now() / 1000) },
  ]
  let failDismiss = false
  let failRead = false
  let holdNext = false
  let releasePoll
  let pollStarted
  const deletes = []
  async function holdPoll() {
    holdNext = true
    const started = new Promise((resolve) => { pollStarted = resolve })
    await page.clock.runFor(60_000)
    await started
  }
  await page.route('**/api/notifications**', async (route) => {
    const request = route.request()
    const path = new URL(request.url()).pathname
    if (request.method() === 'DELETE') {
      if (failDismiss) { failDismiss = false; return route.fulfill({ status: 503, json: { code: 'unavailable', message: 'Could not dismiss that notification' } }) }
      const id = Number(path.split('/').at(-1))
      deletes.push(id)
      items = items.filter((item) => item.id !== id)
      return route.fulfill({ status: 204 })
    }
    if (path === '/api/notifications/read') {
      if (failRead) { failRead = false; return route.fulfill({ status: 503, json: { code: 'unavailable', message: 'Could not mark notifications read' } }) }
      items = items.map((item) => ({ ...item, read: true }))
      return route.fulfill({ json: { updated: 1 } })
    }
    const data = { items: structuredClone(items), unread: items.filter((item) => !item.read).length, pendingRequests: 1, pendingRequestItems: [pending] }
    if (holdNext) {
      holdNext = false
      const held = new Promise((resolve) => { releasePoll = resolve })
      pollStarted()
      await held
    }
    return route.fulfill({ json: data })
  })
  try {
    await page.goto(`${base}/library`, { waitUntil: 'networkidle' })
    const trigger = page.getByRole('button', { name: 'Notifications, 1 unread', exact: true })
    await trigger.click()
    const panel = page.getByRole('region', { name: 'Notifications', exact: true })
    await panel.getByRole('button', { name: 'Approve', exact: true }).waitFor()
    for (const theme of ['paper', 'ink']) {
      await page.evaluate((value) => { document.documentElement.dataset.theme = value }, theme)
      for (const width of [320, 390, 1440]) {
        await page.setViewportSize({ width, height: 844 })
        const box = await panel.getByRole('button', { name: 'Dismiss notification: Where Maps End is ready', exact: true }).boundingBox()
        expect(box.width >= (width < 640 ? 48 : 32) && box.height >= (width < 640 ? 48 : 32), 'dismissal keeps a usable touch target')
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), 'notifications fit the viewport')
        await page.evaluate(axe.source)
        const violations = await page.evaluate(async () => (await window.axe.run(document, { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21aa', 'wcag22aa'] } })).violations.map(({ id, nodes }) => ({ id, nodes: nodes.map(({ html, failureSummary }) => ({ html, failureSummary })) })))
        expect(violations.length === 0, `Notifications accessibility (${theme}, ${width}): ${JSON.stringify(violations)}`)
      }
    }

    const ready = panel.getByRole('button', { name: 'Dismiss notification: Where Maps End is ready', exact: true })
    await panel.getByRole('button', { name: 'Dismiss notification: Robin asked for Where Maps End', exact: true }).click()
    await ready.waitFor()
    await page.waitForFunction(() => document.activeElement?.getAttribute('aria-label') === 'Dismiss notification: Where Maps End is ready')
    expect(await trigger.count() === 1, 'dismissing a read alert keeps the unread count')
    expect(await panel.getByRole('button', { name: 'Approve', exact: true }).count() === 1, 'the pending request remains after its alert is dismissed')
    expect(page.url() === `${base}/library`, 'dismissal keeps the current page')

    await holdPoll()
    failDismiss = true
    await ready.click()
    await panel.getByRole('alert').getByText('Could not dismiss that notification', { exact: true }).waitFor()
    expect(await ready.count() === 1 && await trigger.count() === 1, 'failed dismissal keeps the alert and unread count')
    await ready.click()
    await ready.waitFor({ state: 'detached' })
    releasePoll()
    await page.waitForLoadState('networkidle')
    expect(await panel.getByRole('button', { name: /^Dismiss notification:/ }).count() === 0, 'an older poll cannot resurrect a dismissed notification')
    expect(await page.getByRole('button', { name: 'Notifications', exact: true }).count() === 1, 'dismissing an unread alert updates the badge')
    expect(await panel.getByRole('button', { name: 'Approve', exact: true }).count() === 1, 'approvals stay reachable with an empty alert list')
    await page.keyboard.press('Escape')
    await panel.waitFor({ state: 'detached' })
    expect(await page.getByRole('button', { name: 'Notifications', exact: true }).evaluate((button) => button === document.activeElement), 'Escape returns focus to the bell')
    await page.reload({ waitUntil: 'networkidle' })
    await page.getByRole('button', { name: 'Notifications', exact: true }).click()
    expect(await panel.getByRole('button', { name: /^Dismiss notification:/ }).count() === 0 && deletes.join(',') === '2,1', 'successful dismissal persists after reload')

    items = [{ id: 3, kind: 'ready', title: 'A new fictional book is ready', body: null, read: false, bookId: 77, acquisitionId: null, createdAt: Math.floor(Date.now() / 1000) }]
    await page.reload({ waitUntil: 'networkidle' })
    await page.getByRole('button', { name: 'Notifications, 1 unread', exact: true }).click()
    await holdPoll()
    failRead = true
    await panel.getByRole('button', { name: 'Mark all read', exact: true }).click()
    await panel.getByRole('alert').getByText('Could not mark notifications read', { exact: true }).waitFor()
    expect(await page.getByRole('button', { name: 'Notifications, 1 unread', exact: true }).count() === 1, 'failed Mark all read retains the unread count')
    await panel.getByRole('button', { name: 'Mark all read', exact: true }).click()
    await panel.getByRole('button', { name: 'Mark all read', exact: true }).waitFor({ state: 'detached' })
    releasePoll()
    await page.waitForLoadState('networkidle')
    expect(await page.getByRole('button', { name: 'Notifications', exact: true }).count() === 1, 'an older poll cannot undo Mark all read')
    expect(await panel.getByRole('button', { name: /^Dismiss notification:/ }).count() === 1, 'Mark all read keeps alerts available for dismissal')
    expect(await panel.getByRole('button', { name: /^Dismiss notification:/ }).evaluate((button) => button === document.activeElement), 'marking read preserves keyboard focus')
  } finally {
    releasePoll?.()
  }
}

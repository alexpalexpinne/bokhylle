// Fictional operational state for public screenshots and browser checks.
export function serverPreview() {
  const now = Date.parse('2026-01-15T10:00:00Z') / 1000
  const server = {
    build: { version: '0.1.0', commit: 'f2fe990327de732342a67ecfb2095ade36f8a9d1', dirty: false, builtAt: now - 604800, installation: 'docker' },
    startedAt: now - 284400, uptimeSeconds: 284400, databaseOk: true, restartRequired: [],
    storage: [{ locations: [
      { label: 'Config', path: '/config', writable: true, error: null },
      { label: 'Library', path: '/library', writable: true, error: null },
      { label: 'Downloads', path: '/downloads', writable: true, error: null },
    ], availableBytes: 412 * 1024 ** 3, totalBytes: 1.4 * 1024 ** 4, lowSpace: false, error: null }],
  }
  const backups = {
    intervalHours: 24, keep: 7, latest: { createdAt: now - 25200, size: 7340032 },
    lastSuccess: { createdAt: now - 25200, size: 7340032 }, inventoryError: null,
    outcome: 'succeeded', lastAttempt: { at: now - 25200, summary: null }, lastFailure: null,
    nextScheduledAt: now + 61200, schedulerEnabled: true,
  }
  const updates = {
    currentVersion: '0.1.0', state: 'update_available', latestVersion: '0.1.1',
    releaseUrl: 'https://github.com/alexpalexpinne/bokhylle/releases/tag/v0.1.1',
    checkedAt: now - 3600, lastSuccessAt: now - 3600, error: null, automaticChecks: true,
  }
  const diagnostics = {
    formatVersion: 1, generatedAt: now, build: server.build, startedAt: server.startedAt,
    uptimeSeconds: server.uptimeSeconds, platform: 'linux / x86_64', databaseOk: true,
    integrations: [{ name: 'Email delivery', configured: false }],
    storage: [{ locations: ['Config', 'Library', 'Downloads'], unwritableLocations: [],
      availableBytes: server.storage[0].availableBytes, totalBytes: server.storage[0].totalBytes,
      lowSpace: false, capacityAvailable: true }],
    backups, restartRequired: [], library: { books: 42, files: 42, missingFiles: 0, inaccessibleFiles: 0, booksWithoutFiles: 0 }, recentErrors: [],
  }
  return { server, backups, updates, diagnostics }
}

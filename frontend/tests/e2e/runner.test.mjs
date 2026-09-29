import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import test from 'node:test'

const runner = fileURLToPath(new URL('./run.mjs', import.meta.url))

function run(overrides) {
  return spawnSync(process.execPath, [runner], {
    env: { ...process.env, BOKHYLLE_E2E_BASE: 'http://127.0.0.1:0', BOKHYLLE_E2E_PASSWORD: '', BOKHYLLE_E2E_STRICT: '0', BOKHYLLE_E2E_SPECS: '', ...overrides },
    encoding: 'utf8', timeout: 10000,
  })
}

test('strict mode rejects missing credentials before starting the full suite', () => {
  const result = run({ BOKHYLLE_E2E_STRICT: '1' })
  assert.equal(result.status, 1)
  assert.match(result.stderr, /strict mode requires BOKHYLLE_E2E_PASSWORD/)
})

test('strict mode can select mocked specs without credentials, but requires their server', () => {
  const result = run({ BOKHYLLE_E2E_STRICT: '1', BOKHYLLE_E2E_SPECS: 'onboarding-search,book-detail' })
  assert.equal(result.status, 1)
  assert.match(result.stdout, /server not reachable/)
  assert.doesNotMatch(result.stderr, /requires BOKHYLLE_E2E_PASSWORD/)
})

test('an unavailable server in optional mode reports a skipped suite', () => {
  const result = run({})
  assert.equal(result.status, 0)
  assert.match(result.stdout, /suite skipped/)
  assert.doesNotMatch(result.stdout, /all specs passed/)
})

test('an unknown spec cannot silently turn a selected run into no tests', () => {
  const result = run({ BOKHYLLE_E2E_SPECS: 'misspelled-spec' })
  assert.equal(result.status, 1)
  assert.match(result.stderr, /unknown spec\(s\): misspelled-spec/)
})

import { existsSync } from 'node:fs'
import { chromium } from 'playwright-core'
import { login } from './support.mjs'
import loginSpec from './specs/login.mjs'
import sessionExpirySpec from './specs/session-expiry.mjs'
import themeSpec from './specs/theme.mjs'
import discoverSpec from './specs/discover.mjs'
import settingsSpec from './specs/settings.mjs'
import requestsSpec from './specs/requests.mjs'
import activitySpec from './specs/activity.mjs'
import librarySpec from './specs/library.mjs'
import readersSpec from './specs/readers.mjs'
import familySpec from './specs/family.mjs'
import mobileSpec from './specs/mobile.mjs'
import mobileChildSpec from './specs/mobile-child.mjs'
import stabilitySpec from './specs/stability.mjs'
import homeSheetSpec from './specs/home-sheet.mjs'
import homeDesignSpec from './specs/home-design.mjs'
import profileAppearanceSpec from './specs/profile-appearance.mjs'
import searchStatesSpec from './specs/search-states.mjs'
import profileHelpSpec from './specs/profile-help.mjs'
import chunkRecoverySpec from './specs/chunk-recovery.mjs'
import onboardingSearchSpec from './specs/onboarding-search.mjs'
import childSetupSpec from './specs/child-setup.mjs'
import profileMarksSpec from './specs/profile-marks.mjs'
import bookDetailSpec from './specs/book-detail.mjs'
import demoSpec from './specs/demo.mjs'
import discoverDeepLinkSpec from './specs/discover-deep-link.mjs'
import classificationReviewSpec from './specs/classification-review.mjs'
import seriesProgressSpec from './specs/series-progress.mjs'
import { accessibilityLogin, accessibilityApp } from './specs/accessibility.mjs'
import cataloguesSpec from './specs/catalogues.mjs'

const DESKTOP = { width: 1440, height: 1000 }
const MOBILE = { width: 390, height: 844 }

const base = process.env.BOKHYLLE_E2E_BASE ?? 'http://127.0.0.1:8099'
const user = process.env.BOKHYLLE_E2E_USER ?? 'admin'
const password = process.env.BOKHYLLE_E2E_PASSWORD ?? ''
const chrome =
  [
    process.env.PLAYWRIGHT_CHROME_PATH,
    '/usr/bin/google-chrome',
    '/usr/bin/google-chrome-stable',
    '/opt/google/chrome/chrome',
  ]
    .filter(Boolean)
    .find((candidate) => existsSync(candidate)) ?? '/usr/bin/google-chrome'
const strict = process.env.BOKHYLLE_E2E_STRICT === '1'

async function reachable() {
  try {
    const response = await fetch(`${base}/healthz`, { signal: AbortSignal.timeout(4000) })
    return response.ok
  } catch {
    return false
  }
}

const allSpecs = [
  { name: 'login', run: loginSpec, auth: false },
  { name: 'accessibility-login', run: accessibilityLogin, auth: false },
  { name: 'session-expiry', run: sessionExpirySpec, auth: true },
  { name: 'theme', run: themeSpec, auth: false },
  { name: 'discover', run: discoverSpec, auth: true },
  { name: 'settings', run: settingsSpec, auth: true },
  { name: 'catalogues', run: cataloguesSpec, auth: true },
  { name: 'requests', run: requestsSpec, auth: true },
  { name: 'activity', run: activitySpec, auth: true },
  { name: 'library', run: librarySpec, auth: true },
  { name: 'readers', run: readersSpec, auth: true },
  { name: 'family', run: familySpec, auth: true },
  { name: 'mobile', run: mobileSpec, auth: true, viewport: MOBILE },
  { name: 'mobile-child', run: mobileChildSpec, auth: true, viewport: MOBILE },
  { name: 'stability', run: stabilitySpec, auth: true },
  { name: 'home-sheet', run: homeSheetSpec, auth: false },
  { name: 'home-design', run: homeDesignSpec, auth: false },
  { name: 'profile-appearance', run: profileAppearanceSpec, auth: false },
  { name: 'search-states', run: searchStatesSpec, auth: false },
  { name: 'profile-help', run: profileHelpSpec, auth: false },
  { name: 'chunk-recovery', run: chunkRecoverySpec, auth: false },
  { name: 'onboarding-search', run: onboardingSearchSpec, auth: false },
  { name: 'child-setup', run: childSetupSpec, auth: false },
  { name: 'profile-marks', run: profileMarksSpec, auth: false },
  { name: 'book-detail', run: bookDetailSpec, auth: false },
  { name: 'demo', run: demoSpec, auth: false },
  { name: 'discover-deep-link', run: discoverDeepLinkSpec, auth: false },
  { name: 'classification-review', run: classificationReviewSpec, auth: false },
  { name: 'series-progress', run: seriesProgressSpec, auth: false },
  { name: 'accessibility-app', run: accessibilityApp, auth: true },
  { name: 'accessibility-mobile', run: accessibilityApp, auth: true, viewport: MOBILE },
]

const requested = (process.env.BOKHYLLE_E2E_SPECS ?? '').split(',').map((name) => name.trim()).filter(Boolean)
const unknown = requested.filter((name) => !allSpecs.some((spec) => spec.name === name))
if (unknown.length > 0) {
  console.error(`e2e: unknown spec(s): ${unknown.join(', ')}. Available: ${allSpecs.map((spec) => spec.name).join(', ')}`)
  process.exit(1)
}
const specs = requested.length > 0 ? allSpecs.filter((spec) => requested.includes(spec.name)) : allSpecs

if (strict && !password && specs.some((spec) => spec.auth)) {
  console.error('e2e: strict mode requires BOKHYLLE_E2E_PASSWORD for authenticated specs (BOKHYLLE_E2E_USER defaults to admin)')
  process.exit(1)
}

if (!(await reachable())) {
  console.log(`e2e: suite skipped — server not reachable at ${base}; start it or set BOKHYLLE_E2E_BASE`)
  process.exit(strict ? 1 : 0)
}

const browser = await chromium.launch({ executablePath: chrome, args: ['--no-sandbox', '--disable-dev-shm-usage'] })
let failed = 0
let passed = 0
let skipped = 0
for (const spec of specs) {
  if (spec.auth && !password) {
    skipped += 1
    console.log(`  skip ${spec.name}: set BOKHYLLE_E2E_PASSWORD (and BOKHYLLE_E2E_USER) to run this spec`)
    continue
  }
  const context = await browser.newContext({
    viewport: spec.viewport ?? DESKTOP,
    deviceScaleFactor: 1,
  })
  context.setDefaultTimeout(15000)
  context.setDefaultNavigationTimeout(30000)
  const page = await context.newPage()
  if (process.env.BOKHYLLE_E2E_VERBOSE === '1') {
    page.on('request', (request) => {
      if (request.url().includes('/api/')) {
        console.log(`   req ${request.method()} ${request.url().slice(0, 110)}`)
      }
    })
  }
  try {
    if (spec.auth) {
      await login(page, base, user, password)
    }
    await spec.run(page, { base, user })
    passed += 1
    console.log(`  ok   ${spec.name}`)
  } catch (error) {
    failed += 1
    console.error(`  FAIL ${spec.name}: ${error.stack ?? error.message}`)
  } finally {
    await context.close()
  }
}

await browser.close()
console.log(`e2e: ${passed} passed, ${failed} failed, ${skipped} skipped`)
if (failed > 0) {
  console.error(`e2e: ${failed} spec(s) failed`)
  process.exit(1)
}

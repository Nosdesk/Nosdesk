import { defineConfig, devices } from '@playwright/test'

/**
 * End-to-end tests against the running dev stack.
 *
 * These cover the things jsdom cannot: touch gesture arbitration, scroll
 * position, and real layout measurement. Several of the behaviours here were
 * regressions found by hand that would otherwise fail silently — a board whose
 * drag is stolen by the scroll container still renders perfectly.
 *
 * Prerequisites (not started by this config, deliberately — the stack is a
 * multi-container app with a database, and spinning it up per run would be
 * slower and flakier than pointing at the one that is already running):
 *
 *   make dev          # or: docker compose -f compose.yaml -f compose.dev.yaml up -d
 *   make seed-demo    # demo users + projects the specs navigate to
 *
 * Then: pnpm --filter nosdesk-frontend run test:e2e
 *
 * Override the target with E2E_BASE_URL when the stack is not on :8080.
 */
export default defineConfig({
  testDir: './e2e',
  // The specs drive one shared app instance and some of them move tickets
  // between columns, so they must not race each other.
  fullyParallel: false,
  workers: 1,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['github'], ['list']] : [['list']],
  // Generous on CI. Each spec logs in, waits for the sync pool to fill, and
  // then drives a multi-step gesture; on a shared runner that is comfortably
  // slower than a dev machine, and the first CI run timed out on the last and
  // slowest test at 60s while asserting nothing wrong.
  timeout: process.env.CI ? 150_000 : 60_000,
  expect: { timeout: process.env.CI ? 20_000 : 10_000 },
  use: {
    baseURL: process.env.E2E_BASE_URL ?? 'http://localhost:8080',
    // The dev stack may be served over a self-signed cert on some hosts.
    ignoreHTTPSErrors: true,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  projects: [
    {
      // Spelled out rather than using an `iPhone` preset: those imply WebKit,
      // and the touch specs drive real gestures through CDP
      // (`Input.dispatchTouchEvent`), which is Chromium-only. A preset would
      // silently switch engines and fail to launch. 390x844 is the iPhone 14
      // viewport, which is what the layout numbers in these specs refer to.
      name: 'phone',
      use: {
        ...devices['Desktop Chrome'],
        viewport: { width: 390, height: 844 },
        hasTouch: true,
        isMobile: true,
        deviceScaleFactor: 3,
      },
    },
    {
      name: 'desktop',
      use: { ...devices['Desktop Chrome'], viewport: { width: 1680, height: 1000 } },
    },
  ],
})

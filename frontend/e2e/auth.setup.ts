import { test as setup } from '@playwright/test'
import { AUTH_STATE, login } from './helpers'

/**
 * Log in once per run and save the session for every other project to reuse.
 *
 * Each spec used to drive the full login form itself. At 15 specs that is 15
 * round trips through a real auth flow, and it was the last remaining source of
 * flake in the suite: a single `page.waitForURL` exceeding the 60s local timeout
 * failed a spec that was asserting nothing about login.
 *
 * The app's session is HTTP cookies (access / refresh / CSRF) plus a couple of
 * localStorage keys, and `storageState` captures both, so a restored context is
 * authenticated exactly as a freshly logged-in one is.
 *
 * This runs before the seed contract, which then also gets to skip logging in.
 */
setup('authenticate', async ({ page }) => {
  await login(page)
  await page.context().storageState({ path: AUTH_STATE })
})

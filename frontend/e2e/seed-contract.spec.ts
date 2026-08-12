import { test, expect } from '@playwright/test'
import { PROJECT_TIMELINE, PROJECT_SPARSE, PROJECT_BOARD, login } from './helpers'

/**
 * The shape contract the rest of the suite is written against.
 *
 * `seed_demo` is a product demo dataset, not a test fixture — it never promised
 * any particular shape, and the suite quietly assumed one. When that assumption
 * broke, the symptom was a 30-second `waitForSelector` timeout inside an
 * unrelated reschedule spec, which reads like a product bug rather than missing
 * data. (It was missing data: the seed set no due or start dates at all, so the
 * timeline had nothing to draw and the spec could never pass on a clean DB.)
 *
 * These assertions turn that into one plain failure, up front. This runs as a
 * Playwright *setup project*, so if the seed does not hold up its end, the phone
 * and desktop suites are skipped rather than producing a cascade downstream.
 *
 * If one of these fails, fix the seed (`backend/seeds/demo.json` +
 * `backend/src/bin/seed_demo.rs`) or the constants in `helpers.ts` — not the
 * spec that happened to notice.
 *
 * Every assertion here is an auto-retrying `expect` on a locator rather than a
 * count taken after a fixed sleep: the sync pool hydrates asynchronously, and a
 * fixed settle makes the gate itself flaky, which is the one thing a gate must
 * never be.
 */
test.describe('seed contract', () => {
  test('PROJECT_BOARD has board cards to land on', async ({ page }) => {
    await login(page)
    await page.goto(`/projects/${PROJECT_BOARD}`, { waitUntil: 'domcontentloaded' })
    await expect(
      page.locator('[data-card-id]').first(),
      `project ${PROJECT_BOARD} must have tickets on its board — the landing-scroll spec needs a populated column`,
    ).toBeVisible()
  })

  test('PROJECT_TIMELINE has a planned timeline block', async ({ page }) => {
    await login(page)
    await page.goto(`/projects/${PROJECT_TIMELINE}/gantt`, { waitUntil: 'domcontentloaded' })
    // A *planned* block: start AND due. A due-only ticket renders as a day-tall
    // marker, which is too short to grab and drag.
    await expect(
      page.locator('[data-timeline-card-id][data-timeline-has-start-date="true"]').first(),
      `project ${PROJECT_TIMELINE} must have a ticket with both start_date and due_date — the reschedule spec drags one`,
    ).toBeVisible()
  })

  test('PROJECT_SPARSE has no scheduled work', async ({ page }) => {
    await login(page)
    await page.goto(`/projects/${PROJECT_SPARSE}/gantt`, { waitUntil: 'domcontentloaded' })
    // Asserted through the empty state rather than "count is 0", so this waits
    // for a positive signal. A zero count would also be satisfied by a page that
    // simply had not rendered yet.
    await expect(
      page.locator('p', { hasText: /scheduled|due date/i }).first(),
      `project ${PROJECT_SPARSE} must have NO scheduled tickets — the empty-state spec asserts this hint renders`,
    ).toBeVisible()
    await expect(page.locator('[data-timeline-card-id]')).toHaveCount(0)
  })
})

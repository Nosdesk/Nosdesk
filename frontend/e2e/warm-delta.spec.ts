import { test, expect } from '@playwright/test'

/**
 * The warm-launch transport contract (sync/lifecycle.ts).
 *
 * First visit in a fresh browser context has an empty IndexedDB, so it must
 * stream the workspace bootstrap. A reload of the same context has the
 * snapshot cached and watermarked, so it must catch up with a delta from the
 * commit-safe cursor and re-stream NO workspace snapshot. Playwright's
 * storageState carries cookies but never IndexedDB, so every test context
 * starts cold by construction and the reload is the warm case.
 *
 * Asserted at the network layer on purpose: rendering is covered elsewhere,
 * and this is the machinery that quietly regresses if a refactor makes
 * subscribe() bootstrap unconditionally again.
 */
test('warm relaunch catches up via delta, not a snapshot re-stream', async ({ page }) => {
  const calls: string[] = []
  page.on('request', (req) => {
    const u = new URL(req.url())
    if (u.pathname.endsWith('/api/sync/bootstrap') || u.pathname.endsWith('/api/sync/delta')) {
      calls.push(u.pathname + u.search)
    }
  })
  const workspaceBootstraps = () =>
    calls.filter((c) => c.includes('bootstrap') && c.includes('workspace%3A')).length

  await page.goto('/tickets', { waitUntil: 'domcontentloaded' })
  // Cold: the workspace snapshot streams.
  await expect.poll(workspaceBootstraps, { timeout: 15_000 }).toBeGreaterThan(0)
  // Let the stream finish so the watermark lands before the reload. The
  // commit-safe cursor (`from_xid8`) only seeds when the bootstrap's end
  // line is processed, so a delta carrying it proves the launch settled.
  await expect
    .poll(() => calls.some((c) => c.includes('delta') && c.includes('from_xid8=')), {
      timeout: 15_000,
    })
    .toBe(true)

  calls.length = 0
  await page.reload({ waitUntil: 'domcontentloaded' })
  // Warm: a delta from a real commit-safe cursor...
  await expect
    .poll(
      () => calls.filter((c) => c.includes('delta') && c.includes('from_xid8=')).length,
      { timeout: 15_000 },
    )
    .toBeGreaterThan(0)
  // ...and no workspace snapshot re-stream.
  expect(workspaceBootstraps()).toBe(0)
})

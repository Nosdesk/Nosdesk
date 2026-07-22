import { test, expect } from '@playwright/test';

// Loads the harness in two modes, waits for the async checks to settle, and
// asserts every reported row passed. Prints the full result set + any console
// errors so a failure says WHY (CSP block vs CORS vs opaque-origin module load).
//
// - cross-origin: the runtime is served from a separate origin (the hosted path).
// - same-origin:  the runtime is served from the app's OWN origin under /so, into
//   a sandbox="allow-scripts" iframe. This proves the same-origin src still gets
//   an opaque origin (M3a/b cookie+localStorage still throw) — the zero-config
//   self-host default rests on this.
const MODES = [
  { label: 'cross-origin (separate origin, hosted)', path: '/host.html' },
  { label: 'same-origin (self-host default)', path: '/host.html?mode=same' },
];

for (const { label, path } of MODES) {
  test(`opaque-origin sandbox mechanic — ${label}`, async ({ page }) => {
    const consoleErrors = [];
    page.on('console', (m) => {
      if (m.type() === 'error') consoleErrors.push(m.text());
    });
    page.on('pageerror', (e) => consoleErrors.push('pageerror: ' + e.message));

    await page.goto(path);
    await page.waitForFunction(() => window.__spike && window.__spike.done, undefined, {
      timeout: 15_000,
    });

    const results = await page.evaluate(() => window.__spike.results);
    // eslint-disable-next-line no-console
    console.log(`\n[${label}] results:\n` + JSON.stringify(results, null, 2));
    if (consoleErrors.length) {
      // eslint-disable-next-line no-console
      console.log('console errors:\n' + consoleErrors.join('\n'));
    }

    expect(results.length, 'expected all 5 checks to run').toBe(5);
    for (const r of results) {
      expect(r.ok, `${r.name} — ${r.detail || ''}`).toBe(true);
    }
  });
}

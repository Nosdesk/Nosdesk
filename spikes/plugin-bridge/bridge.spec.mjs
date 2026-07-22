import { test, expect } from '@playwright/test';

// The plugin, inside the opaque sandbox, calls `api.tickets.get(42)` (host ->
// plugin: a returned value) then `api.notify(title)` (plugin -> host: an
// argument). The host stub writes the notify arg into `#result` on the host
// origin. Seeing the host-minted title there proves a full Comlink round-trip
// crossed the sandbox boundary in both directions.
test('host<->plugin Comlink round-trip through the opaque sandbox', async ({ page }) => {
  await page.goto('/host.html');
  await expect(page.locator('#result')).toHaveText('ticket 42 from host');
  await expect(page.locator('#host-log')).toHaveText('host: tickets.get(42)');
});

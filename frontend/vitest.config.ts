import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vitest/config'

/**
 * Unit tests for the frontend's pure logic.
 *
 * Deliberately separate from `vite.config.ts` rather than merged into it: the
 * app config targets the browser and injects `@vite/env`, which needs a DOM at
 * import time, so running tests through it fails before collection. This keeps
 * the test setup to what tests actually need — the `@` alias and a DOM
 * environment for modules that touch `window` at import.
 *
 * Scope is unit-level. Anything that needs a real browser (touch gestures,
 * scroll, layout measurement) belongs in the Playwright suite under `e2e/`,
 * because jsdom does not lay out and would happily pass on a broken layout.
 */
export default defineConfig({
  test: {
    environment: 'jsdom',
    include: ['src/**/*.spec.ts'],
  },
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
      '@nosdesk/core': fileURLToPath(new URL('../packages/core/src', import.meta.url)),
    },
  },
})

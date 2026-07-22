import { defineConfig, devices } from '@playwright/test';

// Proves the host<->plugin Comlink round-trip using the REAL @nosdesk/plugin-sdk
// (createRemoteHostApi + connectToHost) and the REAL @nosdesk/plugin-runtime,
// through an opaque-origin sandboxed iframe. WebKit is the Safari/iOS signal.
export default defineConfig({
  testDir: '.',
  testMatch: /bridge\.spec\.mjs/,
  timeout: 30_000,
  reporter: [['list']],
  webServer: {
    command: 'node build.mjs && node serve.mjs',
    url: 'http://localhost:5320/host.html',
    reuseExistingServer: false,
    stdout: 'pipe',
    stderr: 'pipe',
  },
  use: { baseURL: 'http://localhost:5320' },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'webkit', use: { ...devices['Desktop Safari'] } },
  ],
});

import { defineConfig, devices } from '@playwright/test';

// Drives the throwaway sandbox harness across engine families. WebKit is the
// signal that matters for the Safari/iOS engine (not identical to the Tauri iOS
// WKWebView, but the same core engine, far better than Chromium-only).
export default defineConfig({
  testDir: '.',
  testMatch: /spike\.spec\.mjs/,
  timeout: 30_000,
  reporter: [['list']],
  webServer: {
    command: 'node serve.mjs',
    url: 'http://localhost:5310/host.html',
    reuseExistingServer: false,
    stdout: 'pipe',
    stderr: 'pipe',
  },
  use: { baseURL: 'http://localhost:5310' },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'webkit', use: { ...devices['Desktop Safari'] } },
  ],
});

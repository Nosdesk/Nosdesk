/**
 * Platform bootstrap: configures the `@nosdesk/core` seams for whichever host
 * the app runs in. Web (browser) uses cookies + localStorage; the Tauri shell
 * uses bearer auth + the native HTTP client. Only the matching branch's modules
 * load (the Tauri branch is a lazy chunk), so the web bundle never executes
 * Tauri code.
 */

import { isTauriRuntime } from './runtime'

export { isTauriRuntime }
export { saveFile, shareFile, type FileSource, type SaveResult } from './files'

async function setupWebPlatform(): Promise<void> {
  await import('./web')
}

async function setupTauriPlatform(): Promise<void> {
  const [{ bootstrapMobile, tauriSecureStore }, { initDeviceText }] = await Promise.all([
    import('@nosdesk/mobile'),
    import('./deviceText'),
  ])
  // The server (cloud or self-hosted) comes from the persisted choice / the
  // connect screen, not a build constant. bootstrapMobile resolves it. The
  // phone's text settings are read alongside, so the first screen already has
  // them.
  await Promise.all([
    bootstrapMobile({
      // OS keychain (iOS Keychain / macOS) for the refresh token, so the session
      // survives a cold app restart.
      secureStore: tauriSecureStore(),
      logger: { isProd: import.meta.env.PROD },
    }),
    initDeviceText(),
  ])
}

/** Configure the active platform. Call once at startup, before anything else. */
export async function configurePlatform(): Promise<void> {
  if (isTauriRuntime()) await setupTauriPlatform()
  else await setupWebPlatform()
  // Fetched together, after the platform's seams are set.
  const [
    { setSessionLostHandler },
    { clearStaleRedirect, redirectToLogin },
    { configureSessionSchedule },
    { browserSessionScheduleHost },
  ] = await Promise.all([
    import('@nosdesk/core/services/session'),
    import('@/services/sessionLost'),
    import('@nosdesk/core/services/sessionSchedule'),
    import('@/services/sessionScheduleHost'),
  ])
  // A rejected refresh ends the session on either platform: clear its state
  // and go to sign-in, or the next sign-in reuses the old sync runtime.
  clearStaleRedirect()
  setSessionLostHandler(redirectToLogin)
  // Both run in a browser engine: the refresh schedule's lock, shared expiry
  // and wake-ups are the same.
  configureSessionSchedule(browserSessionScheduleHost())
}

/**
 * Open a URL in the user's real browser. In the Tauri shell a plain
 * `window.open('_blank')` opens inside the webview (or no-ops), which strands
 * the user in a half-loaded web dashboard; the native branch hands the URL to
 * the system browser instead. Web keeps the new-tab behaviour. Best-effort: a
 * failed hand-off must not throw. The `@nosdesk/mobile` import stays dynamic so
 * the web bundle never pulls the native chunk.
 */
export async function openExternalUrl(url: string): Promise<void> {
  if (isTauriRuntime()) {
    const { openInBrowser } = await import('@nosdesk/mobile')
    await openInBrowser(url)
  } else {
    window.open(url, '_blank', 'noopener,noreferrer')
  }
}

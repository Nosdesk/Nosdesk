/**
 * Platform bootstrap: configures the `@nosdesk/core` seams for whichever host
 * the app runs in. Web (browser) uses cookies + localStorage; the Tauri shell
 * uses bearer auth + the native HTTP client. Only the matching branch's modules
 * load (the Tauri branch is a lazy chunk), so the web bundle never executes
 * Tauri code.
 */

/** True inside the Tauri webview (Tauri 2 injects `window.isTauri`). */
export function isTauriRuntime(): boolean {
  return typeof globalThis !== 'undefined' && !!(globalThis as { isTauri?: boolean }).isTauri
}

async function setupWebPlatform(): Promise<void> {
  // Side-effect modules: each configures one core seam on import.
  await import('@/utils/loggerSetup')
  await import('@/utils/storageSetup')
  await import('@/services/transport')
  await import('@/services/apiConfig')
}

async function setupTauriPlatform(): Promise<void> {
  const { bootstrapMobile, tauriSecureStore } = await import('@nosdesk/mobile')
  // The server (cloud or self-hosted) comes from the persisted choice / the
  // connect screen, not a build constant. bootstrapMobile resolves it.
  await bootstrapMobile({
    // OS keychain (iOS Keychain / macOS) for the refresh token, so the session
    // survives a cold app restart.
    secureStore: tauriSecureStore(),
    logger: { isProd: import.meta.env.PROD },
  })
}

/** Configure the active platform. Call once at startup, before anything else. */
export async function configurePlatform(): Promise<void> {
  if (isTauriRuntime()) await setupTauriPlatform()
  else await setupWebPlatform()
  // A rejected refresh ends the session on either platform: clear its state
  // and go to sign-in, or the next sign-in reuses the old sync runtime.
  const { setSessionLostHandler } = await import('@nosdesk/core/services/session')
  const { redirectToLogin } = await import('@/services/sessionLost')
  setSessionLostHandler(redirectToLogin)
  // Both run in a browser engine: the refresh schedule's lock, shared expiry
  // and wake-ups are the same.
  const { configureSessionSchedule } = await import('@nosdesk/core/services/sessionSchedule')
  const { browserSessionScheduleHost } = await import('@/services/sessionScheduleHost')
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
    window.open(url, '_blank', 'noopener')
  }
}

/**
 * Hand a file to the OS share sheet: Save to Files, AirDrop, Mail and Print on
 * iOS, the app chooser on Android. The native webviews can't do any of that
 * themselves (`<a download>` does nothing useful there and `window.print()` is
 * a no-op). Native only: outside the Tauri shell this throws, and the caller
 * keeps its web path (a download link, the browser's print). Resolves with
 * `completed: false` when the user dismisses the sheet. The `@nosdesk/mobile`
 * import stays dynamic so the web bundle never pulls the native chunk.
 */
export async function shareFile(
  bytes: Uint8Array,
  filename: string,
  mimeType: string,
): Promise<{ completed: boolean }> {
  if (!isTauriRuntime()) throw new Error('shareFile needs the native app')
  const { shareFile: nativeShareFile } = await import('@nosdesk/mobile')
  return nativeShareFile(bytes, filename, mimeType)
}

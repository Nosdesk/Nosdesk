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
  const { bootstrapMobile, memorySecureStore } = await import('@nosdesk/mobile')
  // The server (cloud or self-hosted) comes from the persisted choice / the
  // connect screen, not a build constant. bootstrapMobile resolves it.
  await bootstrapMobile({
    // TODO(tauri): swap memorySecureStore for a keychain-backed SecureStore once
    // chosen on-device (see mobile/src/secureStore.ts). memory = login each cold start.
    secureStore: memorySecureStore(),
    logger: { isProd: import.meta.env.PROD },
  })
}

/** Configure the active platform. Call once at startup, before anything else. */
export async function configurePlatform(): Promise<void> {
  if (isTauriRuntime()) await setupTauriPlatform()
  else await setupWebPlatform()
}

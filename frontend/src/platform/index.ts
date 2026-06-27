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
  await bootstrapMobile({
    // Tauri talks to the absolute hosted API; the web build uses relative `/api`.
    apiBaseUrl:
      (import.meta.env.VITE_TAURI_API_URL as string | undefined) ?? 'https://app.nosdesk.com/api',
    collabWsBaseUrl:
      (import.meta.env.VITE_TAURI_WS_URL as string | undefined) ??
      'wss://app.nosdesk.com/api/collaboration/ws',
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

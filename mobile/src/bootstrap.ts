/**
 * Single entry point that wires `@nosdesk/core` for the Tauri host. Call once
 * at app start, before anything uses a seam.
 *
 * Order: logger + storage are synchronous; the api-client interceptors +
 * native HTTP adapter are installed; then the transport is pointed at the
 * persisted server (or the default cloud) so a returning user lands on login.
 * A first-run user with no stored server gets the default; the connect /
 * settings screen calls `setServer()` to override (see serverConfig + transport).
 */
import { setupLogger, type MobileLoggerOptions } from './loggerSetup'
import { setupStorage } from './storageSetup'
import { configureServer, setSecureStore } from './transport'
import { setupApiClient } from './apiClient'
import { setupDeviceName } from './deviceName'
import { DEFAULT_SERVER, getStoredServer } from './serverConfig'
import type { SecureStore } from './secureStore'

export interface MobileBootstrapOptions {
  /** Keychain-backed store for the refresh token. */
  secureStore: SecureStore
  /** Logger config; defaults to dev (DEBUG level, no user id). */
  logger?: MobileLoggerOptions
}

export async function bootstrapMobile(opts: MobileBootstrapOptions): Promise<void> {
  setupLogger(opts.logger ?? { isProd: false })
  setupStorage()
  setSecureStore(opts.secureStore)
  setupApiClient()
  // Before configureServer: the header provider must be registered ahead of
  // the first request, which the transport can fire as soon as it's pointed.
  setupDeviceName()
  // Use the persisted server, else the default cloud. The connect/settings
  // screen can later override via setServer().
  await configureServer(getStoredServer() ?? DEFAULT_SERVER)
}

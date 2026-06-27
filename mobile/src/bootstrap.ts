/**
 * Single entry point that wires `@nosdesk/core` for the Tauri host. Call once
 * at app start, before any request or store access.
 *
 * Order matters: logger + storage are synchronous; transport loads the
 * persisted refresh token from the keychain (async) and registers the seam;
 * the api-client interceptors then read that seam per request.
 */
import { setupLogger, type MobileLoggerOptions } from './loggerSetup'
import { setupStorage } from './storageSetup'
import { setupTransport, type MobileTransportOptions } from './transport'
import { setupApiClient } from './apiClient'

export interface MobileBootstrapOptions extends MobileTransportOptions {
  /** Logger config; defaults to dev (DEBUG level, no user id). */
  logger?: MobileLoggerOptions
}

export async function bootstrapMobile(opts: MobileBootstrapOptions): Promise<void> {
  setupLogger(opts.logger ?? { isProd: false })
  setupStorage()
  await setupTransport(opts)
  setupApiClient()
}

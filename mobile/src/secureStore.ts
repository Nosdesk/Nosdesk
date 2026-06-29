/**
 * Secure, persistent storage for the long-lived refresh token.
 *
 * Unlike the general key/value seam (drafts, recent items, which the webview's
 * `localStorage` backs), the refresh token is a credential and must live in the
 * OS keychain (iOS Keychain / Android Keystore), never in `localStorage`. The
 * interface is async because native keychain access is.
 */
import { invoke } from '@tauri-apps/api/core'

export interface SecureStore {
  /** Read the persisted refresh token, or null if none. */
  load(): Promise<string | null>
  /** Persist (overwrite) the refresh token. */
  save(token: string): Promise<void>
  /** Remove the refresh token (sign-out / unrecoverable 401). */
  clear(): Promise<void>
}

/**
 * In-memory implementation for dev / tests. Tokens do NOT survive an app
 * restart, so a cold start always lands on the login screen.
 */
export function memorySecureStore(): SecureStore {
  let token: string | null = null
  return {
    load: async () => token,
    save: async (t) => {
      token = t
    },
    clear: async () => {
      token = null
    },
  }
}

/**
 * OS-secure-store-backed store, via the `secure-store` Tauri plugin
 * (iOS Keychain / Android Keystore; see mobile/tauri-plugin-secure-store). The
 * token is held device-only, non-synced, readable without a biometric prompt so
 * refresh stays silent. This is the production store; `memorySecureStore` is the
 * dev/test fallback (no cold-start persistence, and desktop `tauri dev`).
 */
export function tauriSecureStore(): SecureStore {
  return {
    load: () =>
      invoke<{ value: string | null }>('plugin:secure-store|load').then((r) => r.value),
    save: (token) => invoke<void>('plugin:secure-store|save', { payload: { token } }),
    clear: () => invoke<void>('plugin:secure-store|clear'),
  }
}

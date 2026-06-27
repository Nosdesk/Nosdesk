/**
 * Secure, persistent storage for the long-lived refresh token.
 *
 * Unlike the general key/value seam (drafts, recent items, which the webview's
 * `localStorage` backs), the refresh token is a credential and must live in the
 * OS keychain (iOS Keychain / Android Keystore), never in `localStorage`. The
 * interface is async because native keychain access is.
 */
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

// Production keychain implementation (to fill in once the Tauri shell exists):
//
//   export function tauriSecureStore(): SecureStore {
//     // Back with the OS keychain via a Tauri plugin (e.g. a keyring/stronghold
//     // plugin), invoked over `@tauri-apps/api`. Kept out of this module for now
//     // so the bootstrap layer carries no Tauri dependency before the native
//     // scaffold and plugin choice are settled.
//   }

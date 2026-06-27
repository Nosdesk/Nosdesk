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

// Production keychain implementation (the one piece that genuinely needs
// on-device evaluation before it's wired):
//
//   export function tauriSecureStore(): SecureStore {
//     // Back the refresh token with the OS keychain (iOS Keychain / Android
//     // Keystore). NOT Stronghold — the Tauri docs mark it deprecated/removed
//     // in v3; NOT `@tauri-apps/plugin-store`, which is plaintext.
//     //
//     // `@impierce/tauri-plugin-keystore` exposes store()/retrieve()/remove(),
//     // but it gates EVERY read behind biometrics — wrong for transparent
//     // token refresh (it would prompt Face ID on each silent 401-refresh).
//     // Pick a keychain plugin whose read is NOT biometric-gated (biometrics,
//     // if wanted, belong on an explicit app-unlock, not on token reads), and
//     // map it onto this interface.
//   }
//
// Until then `memorySecureStore` is the default: the app works (no cold-start
// persistence — a restart returns to login), which is fine for desktop dev and
// the simulator smoke test. The interface makes the keychain swap one file.

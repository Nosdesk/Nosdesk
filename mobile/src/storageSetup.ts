/**
 * General key/value storage wiring, the mobile twin of the web's
 * `frontend/src/utils/storageSetup.ts`.
 *
 * The Tauri webview provides a persistent `localStorage`, so the general KV
 * seam (drafts, recent items, UI prefs) uses the same guarded adapter as web.
 * Credentials do NOT go here: the refresh token lives in the OS keychain (see
 * `secureStore.ts`).
 */
import { configureStorage, type KeyValueStore } from '@nosdesk/core/storage'

const localStorageAdapter: KeyValueStore = {
  getItem(key) {
    try {
      return localStorage.getItem(key)
    } catch {
      return null
    }
  },
  setItem(key, value) {
    try {
      localStorage.setItem(key, value)
    } catch {
      // ignore quota / unavailable
    }
  },
  removeItem(key) {
    try {
      localStorage.removeItem(key)
    } catch {
      // ignore unavailable
    }
  },
}

export function setupStorage(): void {
  configureStorage(localStorageAdapter)
}

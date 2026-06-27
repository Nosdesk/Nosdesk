/**
 * Web wiring for @nosdesk/core's storage seam.
 *
 * Backs the KV seam with `localStorage`, guarding every call: access can throw
 * (private mode, disabled storage, quota), so failures degrade to a no-op /
 * null read rather than crashing, the same tolerance the stores used inline
 * before the seam. Imported at bootstrap; mobile ships its own adapter.
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

configureStorage(localStorageAdapter)

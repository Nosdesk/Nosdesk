/**
 * Key/value storage seam.
 *
 * Stores that persist small bits of state (drafts, recent items, UI prefs)
 * need a string KV store, but the backing store is platform-specific: the web
 * app uses `localStorage`, the Tauri app uses its own persistent store. Core
 * stays headless by depending on this interface and letting the host inject
 * the real implementation at bootstrap (web: frontend/src/utils/storageSetup).
 *
 * The default is in-memory, so a store works (without persistence) before the
 * host configures one, and in unit tests / SSR where no platform store exists.
 */
export interface KeyValueStore {
  getItem(key: string): string | null
  setItem(key: string, value: string): void
  removeItem(key: string): void
}

function createMemoryStore(): KeyValueStore {
  const map = new Map<string, string>()
  return {
    getItem: (key) => map.get(key) ?? null,
    setItem: (key, value) => {
      map.set(key, value)
    },
    removeItem: (key) => {
      map.delete(key)
    },
  }
}

let store: KeyValueStore = createMemoryStore()

/** Install the host's KV store. Called once at bootstrap. */
export function configureStorage(impl: KeyValueStore): void {
  store = impl
}

/** The active KV store. Always returns one (in-memory until configured). */
export function storage(): KeyValueStore {
  return store
}

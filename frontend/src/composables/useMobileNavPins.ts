import { computed, ref, watch } from 'vue';

/**
 * User-customisable selection of routes that get a primary slot in
 * the mobile bottom navigation. Stored in localStorage rather than
 * on the user row because pinning is a per-device preference: a
 * phone is thumb-reach while a tablet has more real estate, and an
 * admin who occasionally browses from a desktop in tablet mode
 * doesn't want their phone pins overriding the desktop's.
 *
 * Synchronisation across tabs of the same device flows through the
 * `storage` event so editing pins in one tab updates every other
 * open tab without a refresh.
 */

const STORAGE_KEY = 'nosdesk.mobileNavPins';

/** Hardcoded fallback. The previous (non-customisable) Navbar
 *  defaulted to these three; we keep them so an upgrade from the
 *  old build doesn't surprise a returning user with a different
 *  set of tiles. */
export const DEFAULT_MOBILE_PINS: readonly string[] = ['/', '/tickets', '/inbox'];

/** How many tiles the bottom bar dedicates to user-pinned routes.
 *  The remaining cells are Search + "More" overflow, both
 *  non-customisable. Three tiles keeps each cell above the 44 CSS
 *  pixel touch-target floor on a 360px viewport. */
export const MAX_MOBILE_PINS = 3;

function readFromStorage(): string[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [...DEFAULT_MOBILE_PINS];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [...DEFAULT_MOBILE_PINS];
    return parsed.filter((v): v is string => typeof v === 'string').slice(0, MAX_MOBILE_PINS);
  } catch {
    // Storage is unavailable (Safari private mode) or the blob is
    // corrupt: fall back to the default rather than throwing into
    // a UI render path.
    return [...DEFAULT_MOBILE_PINS];
  }
}

const pinned = ref<string[]>(readFromStorage());

if (typeof window !== 'undefined') {
  window.addEventListener('storage', (event) => {
    if (event.key !== STORAGE_KEY) return;
    pinned.value = readFromStorage();
  });
}

// Persist on every change. JSON-encoded so we can grow the schema
// later (objects with icons / labels) without a localStorage
// migration; right now strings are enough.
watch(
  pinned,
  (next) => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
    } catch {
      // Same fallback as the read path: don't bring down the UI
      // because localStorage is full or disabled.
    }
  },
  { deep: true },
);

/**
 * Shared composable. The reactive `pinned` ref is module-level so
 * every consumer (the Navbar template, the More sheet, the edit
 * mode) sees the same value without prop-drilling or a store.
 */
export function useMobileNavPins() {
  const isPinned = (path: string) => pinned.value.includes(path);

  /** Capacity left in the pinned set, between 0 and `MAX_MOBILE_PINS`. */
  const remainingSlots = computed(() =>
    Math.max(0, MAX_MOBILE_PINS - pinned.value.length),
  );

  /** Add `path` to the pinned set. No-op when already pinned or at
   *  capacity. Capacity is enforced here so callers can blindly
   *  call `togglePin` without checking remainingSlots themselves. */
  function pin(path: string) {
    if (pinned.value.includes(path)) return;
    if (pinned.value.length >= MAX_MOBILE_PINS) return;
    pinned.value = [...pinned.value, path];
  }

  function unpin(path: string) {
    pinned.value = pinned.value.filter((p) => p !== path);
  }

  /** Convenience: pin if not pinned + below cap, unpin if pinned.
   *  Returns `true` when the toggle resulted in a state change. */
  function togglePin(path: string): boolean {
    if (pinned.value.includes(path)) {
      unpin(path);
      return true;
    }
    if (pinned.value.length >= MAX_MOBILE_PINS) return false;
    pin(path);
    return true;
  }

  /** Reset to the shipped defaults; used by the "Reset" button in
   *  edit mode and as the upgrade-path fallback. */
  function resetToDefaults() {
    pinned.value = [...DEFAULT_MOBILE_PINS];
  }

  return {
    pinnedPaths: computed(() => pinned.value),
    isPinned,
    pin,
    unpin,
    togglePin,
    resetToDefaults,
    remainingSlots,
    MAX_MOBILE_PINS,
  };
}

/**
 * Owns the split-view layout state for the tickets list.
 *
 * Two pieces of persisted state, both module-scoped so the user
 * sees the same layout when navigating away and back:
 *
 *  - `enabled` — split-view on/off. Off by default; the user
 *    explicitly opts in via the toolbar toggle. Persisted so
 *    power users who prefer the layout don't have to re-enable
 *    every session.
 *  - `paneWidth` — pixel width of the right preview pane.
 *    Clamped to `[MIN_PANE, MAX_PANE]` so the table stays
 *    readable and the preview stays usable. The composable
 *    doesn't enforce viewport-aware clamping (the consumer
 *    should fall back to non-split layout below a breakpoint
 *    rather than squeeze both panes).
 */
import { ref } from 'vue'

const STORAGE_ENABLED = 'tickets-split-view-enabled'
const STORAGE_WIDTH = 'tickets-split-view-pane-width'

const DEFAULT_PANE = 460
const MIN_PANE = 360
const MAX_PANE = 720

function loadEnabled(): boolean {
  if (typeof localStorage === 'undefined') return false
  return localStorage.getItem(STORAGE_ENABLED) === '1'
}

function loadWidth(): number {
  if (typeof localStorage === 'undefined') return DEFAULT_PANE
  const raw = localStorage.getItem(STORAGE_WIDTH)
  if (!raw) return DEFAULT_PANE
  const n = parseInt(raw)
  if (!Number.isFinite(n)) return DEFAULT_PANE
  return Math.max(MIN_PANE, Math.min(MAX_PANE, n))
}

export function useSplitView() {
  const enabled = ref<boolean>(loadEnabled())
  const paneWidth = ref<number>(loadWidth())

  function setEnabled(value: boolean): void {
    enabled.value = value
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem(STORAGE_ENABLED, value ? '1' : '0')
    }
  }

  function toggle(): void {
    setEnabled(!enabled.value)
  }

  function setPaneWidth(value: number): void {
    const clamped = Math.max(MIN_PANE, Math.min(MAX_PANE, Math.round(value)))
    paneWidth.value = clamped
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem(STORAGE_WIDTH, String(clamped))
    }
  }

  return {
    enabled,
    paneWidth,
    setEnabled,
    toggle,
    setPaneWidth,
    minPaneWidth: MIN_PANE,
    maxPaneWidth: MAX_PANE,
  }
}

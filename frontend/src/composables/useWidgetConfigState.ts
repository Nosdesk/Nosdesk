/**
 * Reactive per-widget configuration state.
 *
 * A widget passes its id and a `defaults` object; it gets back a
 * reactive state object whose fields:
 *   • hydrate from the stored `dashboard_layout.widgets[].config` on
 *     creation, falling back to each `defaults[key]` when absent;
 *   • write back to the store (debounced, per the store's normal
 *     persistence path) on every mutation.
 *
 * When `widgetId` resolves to null (widget rendered outside the
 * dashboard, e.g. on a profile page), persistence is a no-op; state
 * still reacts but nothing is written. That's what lets widgets like
 * `UserAssignedTickets` be reused on user profiles without the two
 * instances fighting for the same config slot.
 *
 * Validation is the caller's responsibility. This composable stores
 * and retrieves JSON-serialisable values verbatim. Callers that care
 * should defensively validate their own fields after hydration.
 */
import { reactive, toRaw, toValue, watch, type MaybeRef } from 'vue'
import { useDashboardLayoutStore } from '@/stores/dashboardLayout'

export function useWidgetConfigState<T extends Record<string, unknown>>(
  widgetId: MaybeRef<string | null>,
  defaults: T,
): T {
  const store = useDashboardLayoutStore()

  // Hydrate synchronously during setup so first-render observers see
  // the persisted value, not the fallback default briefly flashing.
  const initial: Record<string, unknown> = { ...defaults }
  const id = toValue(widgetId)
  if (id) {
    const cfg = store.getConfig(id)
    if (cfg) {
      for (const key of Object.keys(defaults)) {
        if (key in cfg) initial[key] = cfg[key]
      }
    }
  }

  const state = reactive(initial) as T

  // Watching a reactive object is deep by default, so a simple
  // `watch(state, ...)` catches both top-level reassignments
  // (`config.status = 'open'`) and nested mutations (`config.items
  // .push(x)`). We write a `toRaw` snapshot to the store so it
  // doesn't hold onto the live proxy.
  watch(state, () => {
    const currentId = toValue(widgetId)
    if (!currentId) return
    store.setConfig(currentId, { ...toRaw(state) })
  })

  return state
}

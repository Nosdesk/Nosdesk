import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { useAuthStore } from '@/stores/auth'
import userService from '@/services/userService'
import { useSSE } from '@/services/sseService'
import type { DashboardLayout, UserRole } from '@/types/user'
import {
  defaultLayoutFor,
  mergeWithRegistry,
  widgetsForRole,
} from '@/views/dashboard/widgets'

const DEBOUNCE_MS = 300

/**
 * Per-user dashboard-layout state. The backing data is a
 * `DashboardLayout` JSON column on the user row; we mirror it locally
 * and push changes back through `userService.updateUser()` debounced
 * so rapid drag-reorders don't spam the API.
 *
 * `DashboardGrid` reads `layout.widgets` directly for render order;
 * `AddWidgetModal` reads `addable` for the hidden-widget catalog.
 */
export const useDashboardLayoutStore = defineStore('dashboardLayout', () => {
  const auth = useAuthStore()

  function currentRole(): UserRole {
    return (auth.user?.role as UserRole) ?? 'user'
  }

  const layout = ref<DashboardLayout>(
    mergeWithRegistry(auth.user?.dashboard_layout ?? null, currentRole()),
  )
  const editMode = ref(false)
  const saving = ref(false)

  let persistTimer: ReturnType<typeof setTimeout> | null = null

  /** Resolve the store against whoever is currently logged in. Call on
   * mount or when the user object changes (SSO-return, etc.). */
  function loadFromUser() {
    layout.value = mergeWithRegistry(auth.user?.dashboard_layout ?? null, currentRole())
  }

  // Cross-device sync: listen for user-updated SSE events targeting
  // the current user's `dashboard_layout` field. The backend emits
  // these whenever update_user writes a new layout, so a change made
  // in tab A appears in tab B (and on any other device logged in as
  // the same user) without a reload. Echoes from this same client are
  // filtered upstream by the SSE service via `source_client_id`.
  const sse = useSSE()
  sse.addEventListener('user-updated', (raw) => {
    const data = raw as {
      user_uuid?: string
      field?: string
      value?: unknown
    }
    if (!data || data.field !== 'dashboard_layout') return
    if (!auth.user?.uuid || data.user_uuid !== auth.user.uuid) return

    const next = (data.value ?? null) as DashboardLayout | null
    const merged = mergeWithRegistry(next, currentRole())

    // No-op guard: avoid a reactive write (and potential render churn)
    // when the incoming layout is already what we have locally — e.g.
    // an echo path that slipped through.
    if (JSON.stringify(merged) === JSON.stringify(layout.value)) return

    layout.value = merged
    // Keep the auth user's mirror in sync so later `loadFromUser()`
    // calls (role change, auth refresh) see the canonical value.
    auth.user.dashboard_layout = next
  })

  /** Push the current layout to the server, debounced so rapid drags
   * don't fan out to many requests. */
  function schedulePersist() {
    if (persistTimer) clearTimeout(persistTimer)
    persistTimer = setTimeout(persistNow, DEBOUNCE_MS)
  }

  async function persistNow() {
    if (!auth.user?.uuid) return
    saving.value = true
    try {
      const updated = await userService.updateUser(auth.user.uuid, {
        dashboard_layout: layout.value,
      })
      if (updated) {
        // Keep the auth-store copy in sync so a later reload finds
        // the same thing the server now has.
        auth.user.dashboard_layout = layout.value
      }
    } catch (e) {
      console.error('Failed to persist dashboard layout:', e)
    } finally {
      saving.value = false
    }
  }

  // -- Mutators ---------------------------------------------------------

  function hide(id: string) {
    const entry = layout.value.widgets.find((w) => w.id === id)
    if (!entry || !entry.visible) return
    entry.visible = false
    schedulePersist()
  }

  function show(id: string) {
    const entry = layout.value.widgets.find((w) => w.id === id)
    if (entry) {
      entry.visible = true
    } else {
      // Not in the stored list (edge case: forward-compat tail append
      // that got edited then had its tail entry dropped) — add it.
      layout.value.widgets.push({ id, visible: true })
    }
    schedulePersist()
  }

  function setSpan(id: string, span: 1 | 2 | 3) {
    const entry = layout.value.widgets.find((w) => w.id === id)
    if (!entry) return
    if (entry.span === span) return
    entry.span = span
    schedulePersist()
  }

  /** Upsert a widget's per-widget config. Opaque to the layout system —
   * each widget owns the shape of its own config. Pass `null` to clear. */
  function setConfig(id: string, config: Record<string, unknown> | null) {
    const entry = layout.value.widgets.find((w) => w.id === id)
    if (!entry) return
    if (config === null) {
      delete entry.config
    } else {
      entry.config = config
    }
    schedulePersist()
  }

  /** Read the current config for a widget, or `undefined` if none set. */
  function getConfig(id: string): Record<string, unknown> | undefined {
    return layout.value.widgets.find((w) => w.id === id)?.config
  }

  function move(fromIndex: number, toIndex: number) {
    const widgets = layout.value.widgets
    if (
      fromIndex === toIndex ||
      fromIndex < 0 ||
      toIndex < 0 ||
      fromIndex >= widgets.length ||
      toIndex >= widgets.length
    ) {
      return
    }
    const [item] = widgets.splice(fromIndex, 1)
    widgets.splice(toIndex, 0, item)
    schedulePersist()
  }

  function resetToDefaults() {
    layout.value = defaultLayoutFor(currentRole())
    schedulePersist()
  }

  // -- Derived ----------------------------------------------------------

  /** Widgets the role can use that are currently hidden — fed to the
   * "add widget" modal. */
  const addable = computed(() => {
    const hidden = new Set(
      layout.value.widgets.filter((w) => !w.visible).map((w) => w.id),
    )
    const storedIds = new Set(layout.value.widgets.map((w) => w.id))
    return widgetsForRole(currentRole()).filter(
      (w) => hidden.has(w.id) || !storedIds.has(w.id),
    )
  })

  return {
    layout,
    editMode,
    saving,
    loadFromUser,
    addable,
    hide,
    show,
    move,
    setSpan,
    setConfig,
    getConfig,
    resetToDefaults,
    persistNow,
  }
})

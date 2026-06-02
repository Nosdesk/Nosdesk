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

const UNDO_CAP = 50

/**
 * Per-user dashboard-layout state with a transactional edit model
 * (docs/dashboard-and-analytics-plan.md decision 17).
 *
 * The store keeps two parallel layouts:
 *
 * - `canonicalLayout`: the persisted state, mirrored from the user
 *   row. Updated by `loadFromUser`, by the SSE cross-tab handler,
 *   and by `done()` (the only user-driven write path).
 * - `workingCopy`: a deep clone of canonical, initialised at
 *   `beginEdit()` and discarded on `done()` / `discard()`. Every
 *   mutator writes here while edit mode is active; `undo` / `redo`
 *   stacks let the user step backward and forward through the
 *   in-flight changes.
 *
 * The public `layout` is a computed that returns whichever is
 * active — workingCopy in edit mode, canonical otherwise — so the
 * existing widget consumers (DashboardGrid, AddWidgetModal,
 * useDashboardStats) continue to read `store.layout.widgets`
 * without caring which layer they're observing.
 *
 * Persistence is no longer per-mutation. The previous
 * `schedulePersist`-debounce model wrote on every drag, so the
 * "Done" button was a label rather than a save action and a
 * mid-edit tab close left half-applied state on the server. Now
 * the only path that writes is `done()`, which is honest about
 * what the button does and lets the cross-tab SSE event fire once
 * per edit session rather than once per micro-mutation.
 *
 * The SSE handler still mirrors canonical updates from other tabs
 * even while the user is mid-edit; the working copy stays
 * untouched and overwrites whatever canonical drifted to when the
 * user saves. That matches the "user's in-flight edit is sacred"
 * convention every reference tool follows.
 */
export const useDashboardLayoutStore = defineStore('dashboardLayout', () => {
  const auth = useAuthStore()

  function currentRole(): UserRole {
    return (auth.user?.role as UserRole) ?? 'user'
  }

  function clone(l: DashboardLayout): DashboardLayout {
    // Layout shape is plain JSON (no Dates, no functions); structured
    // JSON clone is correct and avoids dragging in a deep-clone dep.
    return JSON.parse(JSON.stringify(l)) as DashboardLayout
  }

  const canonicalLayout = ref<DashboardLayout>(
    mergeWithRegistry(auth.user?.dashboard_layout ?? null, currentRole()),
  )
  const workingCopy = ref<DashboardLayout | null>(null)
  const undoStack = ref<DashboardLayout[]>([])
  const redoStack = ref<DashboardLayout[]>([])
  const saving = ref(false)

  /** True when the user is inside an Edit session. The session
   *  starts at `beginEdit()`, ends at `done()` or `discard()`. */
  const editMode = computed(() => workingCopy.value !== null)

  /** The active layout. Edit mode reads + writes against the
   *  working copy; view mode reads the canonical persisted state.
   *  Existing widget consumers care only about this getter. */
  const layout = computed<DashboardLayout>(
    () => workingCopy.value ?? canonicalLayout.value,
  )

  /** True when the working copy has diverged from canonical;
   *  drives the navigate-away confirm + the Done button's primary
   *  styling. */
  const isDirty = computed<boolean>(() => {
    if (!workingCopy.value) return false
    return JSON.stringify(workingCopy.value) !== JSON.stringify(canonicalLayout.value)
  })

  /** Resolve the canonical layout against whoever is currently
   *  logged in. Called on mount and on user-uuid change. Does NOT
   *  touch the working copy if one exists — an in-flight edit
   *  survives an auth refresh. */
  function loadFromUser() {
    canonicalLayout.value = mergeWithRegistry(
      auth.user?.dashboard_layout ?? null,
      currentRole(),
    )
  }

  // Cross-device sync: listen for user-updated SSE events targeting
  // the current user's `dashboard_layout` field. The backend emits
  // these whenever update_user writes a new layout, so a change made
  // in tab A appears in tab B without a reload. Echoes from this
  // same client are filtered upstream by the SSE service via
  // `source_client_id`. SSE updates land on the canonical layout
  // only; the in-flight working copy stays untouched.
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

    if (JSON.stringify(merged) === JSON.stringify(canonicalLayout.value)) return

    canonicalLayout.value = merged
    auth.user.dashboard_layout = next
  })

  /** Begin an edit session. Snapshots the canonical layout into
   *  the working copy + clears the undo/redo stacks. No-op if a
   *  session is already open. */
  function beginEdit() {
    if (workingCopy.value !== null) return
    workingCopy.value = clone(canonicalLayout.value)
    undoStack.value = []
    redoStack.value = []
  }

  /** Persist the working copy and close the edit session. The
   *  SSE cross-tab handler fires once on the server's write, not
   *  per micro-mutation. Returns the auth user's updated row. */
  async function done() {
    if (!workingCopy.value) return
    if (!auth.user?.uuid) {
      // Auth lost during edit; the safest action is to drop the
      // session rather than persist against a stale user. The
      // working copy is the user's intent; we surface the failure
      // by reverting rather than silently losing the changes.
      workingCopy.value = null
      undoStack.value = []
      redoStack.value = []
      return
    }
    saving.value = true
    try {
      const updated = await userService.updateUser(auth.user.uuid, {
        dashboard_layout: workingCopy.value,
      })
      if (updated) {
        canonicalLayout.value = workingCopy.value
        auth.user.dashboard_layout = workingCopy.value
      }
    } catch (e) {
      console.error('Failed to persist dashboard layout:', e)
      // Leave the working copy intact so the user can retry; closing
      // the session would lose their work without warning.
      throw e
    } finally {
      saving.value = false
    }
    workingCopy.value = null
    undoStack.value = []
    redoStack.value = []
  }

  /** Drop the working copy + clear the stacks. View mode reverts
   *  to canonical instantly. */
  function discard() {
    workingCopy.value = null
    undoStack.value = []
    redoStack.value = []
  }

  /** Record the current working-copy state for the undo stack
   *  BEFORE mutating. Caps the stack at UNDO_CAP entries; any
   *  fresh mutation drops the redo stack (standard undo/redo
   *  semantics — you can't redo through a new branch). */
  function recordUndo() {
    if (!workingCopy.value) return
    undoStack.value.push(clone(workingCopy.value))
    if (undoStack.value.length > UNDO_CAP) undoStack.value.shift()
    redoStack.value = []
  }

  function undo() {
    if (!workingCopy.value || undoStack.value.length === 0) return
    redoStack.value.push(clone(workingCopy.value))
    if (redoStack.value.length > UNDO_CAP) redoStack.value.shift()
    workingCopy.value = undoStack.value.pop()!
  }

  function redo() {
    if (!workingCopy.value || redoStack.value.length === 0) return
    undoStack.value.push(clone(workingCopy.value))
    if (undoStack.value.length > UNDO_CAP) undoStack.value.shift()
    workingCopy.value = redoStack.value.pop()!
  }

  const canUndo = computed(() => editMode.value && undoStack.value.length > 0)
  const canRedo = computed(() => editMode.value && redoStack.value.length > 0)

  // -- Mutators ---------------------------------------------------------
  //
  // Every mutator only runs when a working copy exists; it records
  // the prior state for undo, then writes to the working copy. View-
  // mode callers (rare; only internal paths like SSE / loadFromUser)
  // bypass these and write canonical directly. The user surface is
  // gated by `editMode` so this assumption holds in practice.

  function hide(id: string) {
    if (!workingCopy.value) return
    const entry = workingCopy.value.widgets.find((w) => w.id === id)
    if (!entry || !entry.visible) return
    recordUndo()
    entry.visible = false
  }

  function show(id: string) {
    if (!workingCopy.value) return
    const entry = workingCopy.value.widgets.find((w) => w.id === id)
    if (entry?.visible) return
    recordUndo()
    if (entry) {
      entry.visible = true
    } else {
      workingCopy.value.widgets.push({ id, visible: true })
    }
  }

  function setSpan(id: string, span: 1 | 2 | 3) {
    if (!workingCopy.value) return
    const entry = workingCopy.value.widgets.find((w) => w.id === id)
    if (!entry || entry.span === span) return
    recordUndo()
    entry.span = span
  }

  /** Upsert a widget's per-widget config. Opaque to the layout system —
   *  each widget owns the shape of its own config. Pass `null` to
   *  clear. */
  function setConfig(id: string, config: Record<string, unknown> | null) {
    if (!workingCopy.value) return
    const entry = workingCopy.value.widgets.find((w) => w.id === id)
    if (!entry) return
    recordUndo()
    if (config === null) {
      delete entry.config
    } else {
      entry.config = config
    }
  }

  /** Read the current config for a widget, or `undefined` if none
   *  set. Reads from whatever layout is active (working copy in
   *  edit mode, canonical otherwise) so the inspector sees the
   *  same value the renderer does. */
  function getConfig(id: string): Record<string, unknown> | undefined {
    return layout.value.widgets.find((w) => w.id === id)?.config
  }

  function move(fromIndex: number, toIndex: number) {
    if (!workingCopy.value) return
    const widgets = workingCopy.value.widgets
    if (
      fromIndex === toIndex ||
      fromIndex < 0 ||
      toIndex < 0 ||
      fromIndex >= widgets.length ||
      toIndex >= widgets.length
    ) {
      return
    }
    recordUndo()
    const [item] = widgets.splice(fromIndex, 1)
    widgets.splice(toIndex, 0, item)
  }

  function resetToDefaults() {
    if (!workingCopy.value) return
    recordUndo()
    workingCopy.value = defaultLayoutFor(currentRole())
  }

  // -- Derived ----------------------------------------------------------

  /** Widgets the role can use that are currently hidden — fed to
   *  the "add widget" modal. Reads `layout` (working copy in edit
   *  mode) so additions reflect immediately. */
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
    isDirty,
    saving,
    canUndo,
    canRedo,
    loadFromUser,
    beginEdit,
    done,
    discard,
    undo,
    redo,
    addable,
    hide,
    show,
    move,
    setSpan,
    setConfig,
    getConfig,
    resetToDefaults,
    // persistNow is kept on the surface for callers that still
    // hand-trigger a write (none in v1; preserved for forward
    // compat with any tooling that scripts a layout fix). It
    // writes whatever the layout getter currently returns.
    persistNow: async () => {
      if (!auth.user?.uuid) return
      saving.value = true
      try {
        await userService.updateUser(auth.user.uuid, {
          dashboard_layout: layout.value,
        })
        canonicalLayout.value = clone(layout.value)
        auth.user.dashboard_layout = layout.value
      } finally {
        saving.value = false
      }
    },
  }
})

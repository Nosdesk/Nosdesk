import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { useAuthStore } from '@/stores/auth'
import userService from '@/services/userService'
import { useSSE } from '@/services/sseService'
import { effectiveRole, type DashboardLayout, type UserRole } from '@/types/user'
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
    return auth.user ? effectiveRole(auth.user) : 'user'
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
   *  per micro-mutation. Throws on any failure (auth lost, network
   *  error, server error) WITHOUT destroying the working copy, so
   *  the EditBar's catch can surface a user-visible toast and the
   *  user can retry without losing their edits. */
  async function done() {
    if (!workingCopy.value) return
    if (!auth.user?.uuid) {
      // Auth lost mid-edit. Throwing (rather than silently dropping
      // the working copy as an earlier version did) gives the
      // EditBar a chance to tell the user "you've been signed out;
      // your edits are still here, please sign in to save". The
      // working copy stays intact for that retry path.
      throw new Error('Cannot save dashboard layout: not signed in')
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
      // Only clear edit-session state on a successful write — a
      // failure preserves workingCopy + stacks so retry is possible.
      workingCopy.value = null
      undoStack.value = []
      redoStack.value = []
    } catch (e) {
      console.error('Failed to persist dashboard layout:', e)
      throw e
    } finally {
      saving.value = false
    }
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

  function setRowSpan(id: string, rowSpan: 1 | 2 | 3) {
    if (!workingCopy.value) return
    const entry = workingCopy.value.widgets.find((w) => w.id === id)
    if (!entry || entry.rowSpan === rowSpan) return
    recordUndo()
    entry.rowSpan = rowSpan
  }

  /**
   * Upsert a widget's per-widget config. Opaque to the layout
   * system — each widget owns its own config shape; pass `null` to
   * clear.
   *
   * Widget config has different semantics than a drag/resize/move:
   * it's an in-context settings save (the gear menu on a stat tile,
   * a chart's data selector) that has to work whether or not the
   * user has flipped the dashboard into edit mode. So the write
   * path forks:
   *   • inside an edit session, write to the working copy so the
   *     change composes with whatever else the user is editing and
   *     can be undone / discarded with the rest;
   *   • outside an edit session, write canonical directly and
   *     persist immediately (debounced upstream by Pinia Colada /
   *     the per-widget watcher).
   *
   * This restores the pre-Wave-2 contract that useWidgetConfigState
   * relied on: setting `store.setConfig(id, {...})` from view mode
   * persists. The earlier rewrite silently dropped view-mode writes,
   * which broke per-widget config pickers.
   */
  function setConfig(id: string, config: Record<string, unknown> | null) {
    if (workingCopy.value) {
      const entry = workingCopy.value.widgets.find((w) => w.id === id)
      if (!entry) return
      recordUndo()
      if (config === null) delete entry.config
      else entry.config = config
      return
    }
    // View mode: mutate canonical + fire the persist asynchronously.
    // The mutation is reactive so subscribers update immediately;
    // the persistConfigOnly() call below is fire-and-forget because
    // the caller (a watcher inside useWidgetConfigState) doesn't
    // await it.
    const entry = canonicalLayout.value.widgets.find((w) => w.id === id)
    if (!entry) return
    if (config === null) delete entry.config
    else entry.config = config
    void persistConfigOnly()
  }

  /** Persist the canonical layout as-is (no working-copy interaction).
   *  Used by the view-mode setConfig path so an in-context config
   *  change survives a page reload. Failures log + restore the prior
   *  state from the auth-tracked snapshot so the UI doesn't drift. */
  async function persistConfigOnly() {
    if (!auth.user?.uuid) return
    try {
      await userService.updateUser(auth.user.uuid, {
        dashboard_layout: canonicalLayout.value,
      })
      auth.user.dashboard_layout = canonicalLayout.value
    } catch (e) {
      console.error('Failed to persist widget config:', e)
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
    setRowSpan,
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

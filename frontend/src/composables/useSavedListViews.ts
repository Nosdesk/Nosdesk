/**
 * Saved-views glue for asset and user list views.
 *
 * Wraps `savedViewsService` with the bits a list view actually
 * needs: load on mount, track the active view's uuid, switch via
 * applyShape / applyFilter, save the current state under a new
 * name, rename / delete. Tickets has its own equivalent
 * (`useSavedViewsStore` + `useTicketsViewResolution`); this
 * composable serves the simpler non-ticket surfaces where every
 * view is private to the creator and there's no built-in /
 * workspace / project layer.
 *
 * Active selection is persisted to localStorage per dataset
 * under `nosdesk:saved-view:{dataset}` so a refresh restores the
 * view that was open. The composable doesn't own filter or
 * grouping state itself; the consumer passes capture / apply
 * callbacks so the source of truth stays in `useListControls` /
 * `useListGrouping`.
 */
import { computed, ref, watch, type ComputedRef, type Ref } from 'vue'
import { savedViewsService, type SavedView } from '@/services/savedViewsService'
import type { ViewSwitcherItem } from '@/components/views/ViewSwitcher.vue'

export interface UseSavedListViewsOptions<S, F> {
  dataset: 'assets' | 'users'
  /** Reactive accessor for the current user's uuid. Saved views
   *  are scoped to `private` + this uuid; null disables save /
   *  load (eg. unauthenticated state). */
  userUuid: Ref<string | null>
  /** Snapshot the current display config (group axis, density,
   *  column visibility, ...) — whatever the view wants to round-
   *  trip through saved views. Schema is opaque to this
   *  composable; the consumer owns it. */
  captureShape: () => S
  /** Snapshot the current filter / facet selection state. */
  captureFilter: () => F
  /** Restore a previously captured shape. */
  applyShape: (shape: S) => void
  /** Restore a previously captured filter state. */
  applyFilter: (filter: F) => void
  /** Translate function (Fluent's $t). */
  t: (key: string, args?: Record<string, string | number>) => string
}

export interface UseSavedListViews<S, F> {
  views: Ref<SavedView<S, F>[]>
  activeViewId: Ref<string | null>
  activeView: ComputedRef<SavedView<S, F> | null>
  /** Reactive list shaped for `<ViewSwitcher>`. Empty when there
   *  are no saved views — the consumer can branch on
   *  `switcherItems.value.length === 0` to hide the picker. */
  switcherItems: ComputedRef<ViewSwitcherItem[]>
  isLoading: Ref<boolean>
  loadViews: () => Promise<void>
  switchTo: (uuid: string | null) => void
  saveAs: (name: string) => Promise<SavedView<S, F> | null>
  rename: (uuid: string, name: string) => Promise<boolean>
  deleteView: (uuid: string) => Promise<boolean>
}

function activeIdStorageKey(dataset: string): string {
  return `nosdesk:saved-view:${dataset}`
}

function loadStoredActiveId(dataset: string): string | null {
  if (typeof localStorage === 'undefined') return null
  return localStorage.getItem(activeIdStorageKey(dataset))
}

function persistActiveId(dataset: string, uuid: string | null): void {
  if (typeof localStorage === 'undefined') return
  const key = activeIdStorageKey(dataset)
  if (uuid === null) localStorage.removeItem(key)
  else localStorage.setItem(key, uuid)
}

export function useSavedListViews<S, F>(
  options: UseSavedListViewsOptions<S, F>,
): UseSavedListViews<S, F> {
  const { dataset, userUuid, captureShape, captureFilter, applyShape, applyFilter } =
    options

  const views = ref<SavedView<S, F>[]>([]) as Ref<SavedView<S, F>[]>
  const activeViewId = ref<string | null>(loadStoredActiveId(dataset))
  const isLoading = ref<boolean>(false)

  const activeView = computed<SavedView<S, F> | null>(() => {
    if (!activeViewId.value) return null
    return views.value.find((v) => v.uuid === activeViewId.value) ?? null
  })

  const switcherItems = computed<ViewSwitcherItem[]>(() =>
    views.value.map((v) => ({
      id: v.uuid,
      name: v.name,
      editable: true,
    })),
  )

  async function loadViews(): Promise<void> {
    if (!userUuid.value) return
    isLoading.value = true
    try {
      views.value = await savedViewsService.listForDataset<S, F>(dataset)
      // If the stored active id no longer exists (deleted on
      // another device), clear it so the view reads as flat
      // rather than "missing view".
      if (
        activeViewId.value &&
        !views.value.some((v) => v.uuid === activeViewId.value)
      ) {
        switchTo(null)
      }
    } finally {
      isLoading.value = false
    }
  }

  function switchTo(uuid: string | null): void {
    activeViewId.value = uuid
    persistActiveId(dataset, uuid)
    if (uuid === null) return
    const view = views.value.find((v) => v.uuid === uuid)
    if (!view) return
    applyShape(view.shape)
    applyFilter(view.filter)
  }

  async function saveAs(name: string): Promise<SavedView<S, F> | null> {
    if (!userUuid.value) return null
    const trimmed = name.trim()
    if (trimmed.length === 0) return null
    const created = await savedViewsService.createForDataset<S, F>({
      scope: 'private',
      scope_id: userUuid.value,
      name: trimmed,
      shape: captureShape(),
      filter: captureFilter(),
      dataset,
    })
    views.value = [...views.value, created].sort((a, b) =>
      a.name.localeCompare(b.name),
    )
    switchTo(created.uuid)
    return created
  }

  async function rename(uuid: string, name: string): Promise<boolean> {
    const trimmed = name.trim()
    if (trimmed.length === 0) return false
    try {
      const updated = await savedViewsService.updateForDataset<S, F>(uuid, {
        name: trimmed,
      })
      views.value = views.value
        .map((v) => (v.uuid === uuid ? updated : v))
        .sort((a, b) => a.name.localeCompare(b.name))
      return true
    } catch {
      return false
    }
  }

  async function deleteView(uuid: string): Promise<boolean> {
    try {
      await savedViewsService.delete(uuid)
      views.value = views.value.filter((v) => v.uuid !== uuid)
      if (activeViewId.value === uuid) switchTo(null)
      return true
    } catch {
      return false
    }
  }

  // Refresh views when the signed-in user changes (eg. switching
  // accounts in dev). Re-loading also covers the SSR-to-client
  // hydration case where userUuid arrives after mount.
  watch(
    userUuid,
    async (next, prev) => {
      if (next === prev) return
      if (!next) {
        views.value = []
        return
      }
      await loadViews()
      // Apply the persisted active selection now that views are
      // loaded, so the view renders against the saved
      // shape/filter on first paint after a refresh.
      const stored = loadStoredActiveId(dataset)
      if (stored && views.value.some((v) => v.uuid === stored)) {
        switchTo(stored)
      }
    },
    { immediate: true },
  )

  return {
    views,
    activeViewId,
    activeView,
    switcherItems,
    isLoading,
    loadViews,
    switchTo,
    saveAs,
    rename,
    deleteView,
  }
}

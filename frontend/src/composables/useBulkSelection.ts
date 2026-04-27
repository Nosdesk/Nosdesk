/**
 * Selection state for a list page's bulk-action UI.
 *
 * Mirrors the 2025 Linear/Asana/GitHub pattern: selection persists
 * across pagination boundaries (so a user can select on page 1,
 * page to page 2, select more, then act on the union), but clears
 * automatically when the underlying filter changes (since "selected
 * matching THIS query" stops being a useful concept once the query
 * itself shifts). A `selectAllMatching()` affordance lets users
 * opt into the wider scope explicitly.
 *
 * The composable is data-source-agnostic: it doesn't know about
 * Pinia Colada, infinite vs paginated mode, or the network layer.
 * Callers feed it the visible items (for select-all-on-page) and a
 * `cacheKey` (for clear-on-filter-change). The returned set is just
 * a Set of stringified item ids.
 *
 * Pairs with `BulkActionBar` (chrome) and `optimisticBulkAction`
 * (toast helper) but does not depend on either, callers can wire
 * their own UI on top.
 */
import { computed, ref, watch, type ComputedRef, type Ref } from 'vue'

export interface UseBulkSelectionOptions<T> {
  /** Items currently visible in the page (the slice the user can
   *  click). Used by `selectAllOnPage` and to derive page-level
   *  "select all" / "deselect all" semantics. */
  items: Ref<readonly T[]> | ComputedRef<readonly T[]>
  /** Stable extractor that turns an item into its selection id.
   *  Defaults to `String(item.id)`. */
  itemId?: (item: T) => string
  /** A reactive cache-key string that changes when the filter set
   *  changes. When this flips, the selection clears and the
   *  "all matching" scope is reset. Pass
   *  `controls.cacheKeyPart` from `useListControls`. */
  cacheKey: Ref<string> | ComputedRef<string>
  /** Optional total count, used to render "Selected N of M" copy
   *  and to decide whether the "Select all matching" affordance
   *  has anything more to offer. */
  totalCount?: Ref<number> | ComputedRef<number>
}

export interface BulkSelection<T> {
  /** Selected item ids (Set semantics, but exposed as an array
   *  ref to keep template iteration simple). */
  selectedIds: ComputedRef<string[]>
  /** Number of selected items (page-scoped, not "all matching"). */
  selectedCount: ComputedRef<number>
  /** True when the user opted into the "all matching" scope. The
   *  ids set is *not* expanded (selecting matching might mean
   *  thousands of rows the client never loaded), the consumer is
   *  expected to send the action with the same filter set instead
   *  of a list of ids. */
  isAllMatchingSelected: ComputedRef<boolean>
  /** True when every visible item is currently selected. */
  areAllOnPageSelected: ComputedRef<boolean>
  /** Toggle one id. Pass `event.shiftKey` for range selection
   *  semantics like the legacy bar. */
  toggle: (id: string, event?: { shiftKey?: boolean }) => void
  /** Select every visible item; deselect if all are already
   *  selected (idempotent toggle behaviour). */
  toggleAllOnPage: () => void
  /** Opt into the "all matching" scope. Selection ids stay as
   *  whatever was on screen at the moment of the click; consumers
   *  acting on `isAllMatchingSelected` should ignore the ids and
   *  use the active filter set instead. */
  selectAllMatching: () => void
  /** Clear selection and reset the all-matching scope. */
  clear: () => void
  /** Imperative check used by row checkbox inputs. */
  isSelected: (id: string) => boolean
}

const defaultItemId = <T>(item: T): string => {
  if (item && typeof item === 'object' && 'id' in (item as Record<string, unknown>)) {
    return String((item as Record<string, unknown>).id)
  }
  return String(item)
}

export function useBulkSelection<T>(
  options: UseBulkSelectionOptions<T>,
): BulkSelection<T> {
  const itemId = options.itemId ?? defaultItemId<T>
  const selectedSet = ref<Set<string>>(new Set())
  const allMatching = ref(false)
  const lastSelectedId = ref<string | null>(null)

  // Filter changed → drop selection. Doing this in a watcher (not
  // a computed) keeps the user's selection stable across re-renders
  // that don't actually change the cache key.
  watch(
    () => options.cacheKey.value,
    () => {
      if (selectedSet.value.size > 0 || allMatching.value) {
        selectedSet.value = new Set()
        allMatching.value = false
        lastSelectedId.value = null
      }
    },
  )

  const selectedIds = computed(() => Array.from(selectedSet.value))
  const selectedCount = computed(() => selectedSet.value.size)

  const areAllOnPageSelected = computed(() => {
    const visible = options.items.value
    if (visible.length === 0) return false
    return visible.every((item) => selectedSet.value.has(itemId(item)))
  })

  function toggle(id: string, event?: { shiftKey?: boolean }) {
    if (event?.shiftKey && lastSelectedId.value) {
      // Shift-click range selection over the visible page.
      const visibleIds = options.items.value.map(itemId)
      const lastIdx = visibleIds.indexOf(lastSelectedId.value)
      const curIdx = visibleIds.indexOf(id)
      if (lastIdx !== -1 && curIdx !== -1) {
        const [start, end] = lastIdx < curIdx ? [lastIdx, curIdx] : [curIdx, lastIdx]
        const next = new Set(selectedSet.value)
        for (let i = start; i <= end; i++) next.add(visibleIds[i])
        selectedSet.value = next
        lastSelectedId.value = id
        return
      }
    }
    const next = new Set(selectedSet.value)
    if (next.has(id)) next.delete(id)
    else next.add(id)
    selectedSet.value = next
    lastSelectedId.value = id
    // Manual mutation revokes the "all matching" claim.
    if (allMatching.value) allMatching.value = false
  }

  function toggleAllOnPage() {
    const visibleIds = options.items.value.map(itemId)
    const allSelected =
      visibleIds.length > 0 && visibleIds.every((id) => selectedSet.value.has(id))
    if (allSelected) {
      // Deselect everything visible; persists off-page selections.
      const next = new Set(selectedSet.value)
      for (const id of visibleIds) next.delete(id)
      selectedSet.value = next
    } else {
      const next = new Set(selectedSet.value)
      for (const id of visibleIds) next.add(id)
      selectedSet.value = next
    }
    lastSelectedId.value = null
    if (allMatching.value) allMatching.value = false
  }

  function selectAllMatching() {
    allMatching.value = true
  }

  function clear() {
    selectedSet.value = new Set()
    allMatching.value = false
    lastSelectedId.value = null
  }

  function isSelected(id: string): boolean {
    return selectedSet.value.has(id)
  }

  return {
    selectedIds,
    selectedCount,
    isAllMatchingSelected: computed(() => allMatching.value),
    areAllOnPageSelected,
    toggle,
    toggleAllOnPage,
    selectAllMatching,
    clear,
    isSelected,
  }
}

/**
 * Adapter from `useBulkSelection<T>` to the string-id-based
 * selection API that `DataTable` exposes.
 *
 * `DataTable` is a domain-agnostic primitive (string ids in,
 * `toggle-selection`/`toggle-all` events out). `useBulkSelection`
 * is the domain-aware composable (knows about persist-across-page,
 * select-all-matching, cache-key-driven clears). Without an
 * adapter, every list view repeats the same five lines of
 * stopPropagation + shape-converting glue.
 *
 * Usage in a list view:
 *
 *   const selection = useBulkSelection<Device>({...})
 *   const dt = useBulkSelectionForDataTable(selection)
 *   ...
 *   <DataTable
 *     :selected-items="dt.selectedItems"
 *     @toggle-selection="dt.onToggleSelection"
 *     @toggle-all="dt.onToggleAll"
 *   />
 */
import { reactive } from 'vue'

import type { BulkSelection } from '@/composables/useBulkSelection'

export interface DataTableSelectionBinding {
  /** Pass to `<DataTable :selected-items>`. */
  selectedItems: string[]
  /** Pass to `<DataTable @toggle-selection>`. Stops propagation
   *  to keep row-click navigation from firing on checkbox change. */
  onToggleSelection: (event: Event, itemId: string) => void
  /** Pass to `<DataTable @toggle-all>`. Toggles the visible-page
   *  scope; off-page selections (from earlier page visits) are
   *  preserved. */
  onToggleAll: (event: Event) => void
}

/**
 * `reactive()` wrapper unwraps the underlying `ComputedRef` for
 * `selectedItems` so consumers can pass it as a plain prop:
 * `:selected-items="dt.selectedItems"` (no `.value`, no top-level
 * destructure required) and TypeScript sees the unwrapped
 * `string[]` shape.
 */
export function useBulkSelectionForDataTable<T>(
  selection: BulkSelection<T>,
): DataTableSelectionBinding {
  return reactive({
    selectedItems: selection.selectedIds,
    onToggleSelection: (event: Event, itemId: string) => {
      event.stopPropagation()
      selection.toggle(itemId, event as { shiftKey?: boolean })
    },
    onToggleAll: (event: Event) => {
      event.stopPropagation()
      selection.toggleAllOnPage()
    },
  }) as DataTableSelectionBinding
}

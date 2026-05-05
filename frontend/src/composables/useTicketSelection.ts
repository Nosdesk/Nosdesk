/**
 * Selection state for the tickets list when split-view is on.
 *
 * `selectedId` is the id of the ticket currently shown in the
 * preview pane. The composable provides set / clear / move-up
 * / move-down operations against a reactive list of cards
 * (typically the post-filter, post-sort sortedCards from the
 * shell).
 *
 * Why selection lives here and not in the sync store: this is a
 * UI-state concern, not a data concern. The sync store carries
 * the cards; the view layer decides which one is "focused" in
 * the preview pane.
 *
 * Auto-clear behaviour: when the selected ticket falls out of
 * the visible card set (filter change, sort by something the
 * row no longer matches, etc.), the consumer should call
 * `reconcile()` so the selection follows the user's intent
 * (clear it rather than ghost-select an invisible row).
 */
import { computed, ref, type ComputedRef } from 'vue'
import type { CardData } from '@/sync/views/types'

export function useTicketSelection(visibleCards: ComputedRef<CardData[]>) {
  const selectedId = ref<number | null>(null)

  const selectedIndex = computed<number>(() => {
    if (selectedId.value == null) return -1
    return visibleCards.value.findIndex((c) => c.id === selectedId.value)
  })

  const selectedCard = computed<CardData | null>(() => {
    const idx = selectedIndex.value
    return idx >= 0 ? visibleCards.value[idx] : null
  })

  function setSelected(id: number | null): void {
    selectedId.value = id
  }

  function clearSelected(): void {
    selectedId.value = null
  }

  /** Move selection by `delta` rows in the visible list. Wraps
   * at the edges. No-op if the list is empty. If nothing is
   * currently selected, lands on row 0 (going down) or last
   * row (going up). */
  function move(delta: 1 | -1): void {
    const list = visibleCards.value
    if (list.length === 0) return
    const cur = selectedIndex.value
    let next: number
    if (cur < 0) {
      next = delta === 1 ? 0 : list.length - 1
    } else {
      next = (cur + delta + list.length) % list.length
    }
    selectedId.value = list[next]?.id ?? null
  }

  /** Pick the first row when nothing's selected. The shell calls
   * this when split-view turns on so the preview pane has
   * something to show. */
  function selectFirstIfNone(): void {
    if (selectedId.value != null && selectedIndex.value >= 0) return
    const first = visibleCards.value[0]
    selectedId.value = first ? first.id : null
  }

  /** After a filter / sort change, drop the selection if the
   * selected ticket is no longer in the visible set. Better than
   * silently keeping a selection that doesn't match any visible
   * row. */
  function reconcile(): void {
    if (selectedId.value == null) return
    if (selectedIndex.value < 0) selectedId.value = null
  }

  return {
    selectedId,
    selectedIndex,
    selectedCard,
    setSelected,
    clearSelected,
    move,
    selectFirstIfNone,
    reconcile,
  }
}

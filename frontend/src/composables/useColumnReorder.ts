/**
 * Shared drag-reorder gesture handlers for column headers.
 *
 * Both the tickets table (`useColumnLayout`) and the shared
 * DataTable (`useDataTableColumns`) need the same HTML5 drag-
 * and-drop reorder UX: a draggable header, a drop target with
 * visual indicator, and a clean source/target/end lifecycle.
 *
 * Width handling and the underlying order storage stay in each
 * caller because the rendering mechanism differs (tickets uses
 * <table> with table-layout:fixed; DataTable uses CSS grid).
 * This composable is intentionally *just* the gesture state and
 * the drop reducer — it doesn't own the order array itself,
 * which lets the two callers keep their own persistence shape.
 *
 * Usage:
 *   const reorder = useColumnReorder({
 *     isReorderable: (id) => id !== 'title',
 *     getCurrentOrder: () => currentOrder.value,
 *     onOrderChange: (next) => { currentOrder.value = next; persist() },
 *   })
 *
 * Then bind:
 *   <div
 *     :draggable="reorder.isReorderable(col.id)"
 *     @dragstart="(e) => reorder.onDragStart(col.id, e)"
 *     @dragover="(e) => reorder.onDragOver(col.id, e)"
 *     @dragleave="() => reorder.onDragLeave(col.id)"
 *     @drop="(e) => reorder.onDrop(col.id, e)"
 *     @dragend="reorder.onDragEnd"
 *   >...</div>
 */
import { ref, type Ref } from 'vue'

export interface UseColumnReorderOptions {
  /** Predicate for whether a column can be reordered (typically
   *  false for a pinned title / primary identifier column). */
  isReorderable: (id: string) => boolean
  /** Reactive accessor for the current order. Used at drop time
   *  to compute the new order without owning the order array. */
  getCurrentOrder: () => readonly string[]
  /** Called with the new order on a successful drop. Caller
   *  decides how to persist (localStorage write, saved-view
   *  mutation, etc.). */
  onOrderChange: (next: string[]) => void
}

export interface UseColumnReorder {
  /** Currently-dragged column id, null when no drag in flight. */
  dragSourceId: Ref<string | null>
  /** Column id under the cursor during drag (drop target), null
   *  when not over a valid target. Drives the visual drop
   *  indicator. */
  dragTargetId: Ref<string | null>
  isReorderable: (id: string) => boolean
  onDragStart: (id: string, event: DragEvent) => void
  onDragOver: (id: string, event: DragEvent) => void
  onDragLeave: (id: string) => void
  onDrop: (id: string, event: DragEvent) => void
  onDragEnd: () => void
}

export function useColumnReorder(
  options: UseColumnReorderOptions,
): UseColumnReorder {
  const { isReorderable, getCurrentOrder, onOrderChange } = options

  const dragSourceId = ref<string | null>(null)
  const dragTargetId = ref<string | null>(null)

  function onDragStart(id: string, event: DragEvent): void {
    if (!isReorderable(id)) {
      event.preventDefault()
      return
    }
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = 'move'
      event.dataTransfer.setData('text/plain', id)
    }
    dragSourceId.value = id
  }

  function onDragOver(id: string, event: DragEvent): void {
    if (!dragSourceId.value || !isReorderable(id)) return
    if (id === dragSourceId.value) return
    event.preventDefault()
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
    dragTargetId.value = id
  }

  function onDragLeave(id: string): void {
    if (dragTargetId.value === id) dragTargetId.value = null
  }

  function onDrop(id: string, event: DragEvent): void {
    event.preventDefault()
    const source = dragSourceId.value
    dragSourceId.value = null
    dragTargetId.value = null
    if (!source || source === id || !isReorderable(id)) return

    const next = [...getCurrentOrder()]
    const fromIdx = next.indexOf(source)
    const toIdx = next.indexOf(id)
    if (fromIdx < 0 || toIdx < 0) return
    next.splice(fromIdx, 1)
    next.splice(toIdx, 0, source)
    onOrderChange(next)
  }

  function onDragEnd(): void {
    dragSourceId.value = null
    dragTargetId.value = null
  }

  return {
    dragSourceId,
    dragTargetId,
    isReorderable,
    onDragStart,
    onDragOver,
    onDragLeave,
    onDrop,
    onDragEnd,
  }
}

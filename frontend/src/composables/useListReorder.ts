import { ref, onMounted, onUnmounted, type Ref } from 'vue'

export interface ListReorderState {
  isDragging: boolean
  draggedId: number | null
  draggedIndex: number
  insertIndex: number
  /** Current pointer position (viewport coords) for floating preview */
  pointerX: number
  pointerY: number
  /** Initial grab offset from top-left of the dragged element */
  offsetX: number
  offsetY: number
}

/**
 * Composable for pointer-based drag-and-drop reordering of a vertical list.
 * Uses pointer events (works on both mouse and touch).
 *
 * The dragged item follows the cursor as a floating preview.
 * A drop indicator line shows where the item will land.
 *
 * Usage:
 *   const { dragState, listRef, handleGripDown } = useListReorder(items, {
 *     getId: (item) => item.id,
 *     onReorder: (reorderedItems, previousItems) => { ... persist to backend ... }
 *   })
 *
 * Template:
 *   <div ref="listRef">
 *     <div v-for="(item, i) in items" :data-item-id="item.id">
 *       <button @pointerdown="handleGripDown(item.id, i, $event)">⠿</button>
 *     </div>
 *   </div>
 *   <!-- Floating preview (teleport or position:fixed) rendered by consumer -->
 */
export function useListReorder<T>(
  items: Ref<T[]>,
  options: {
    getId: (item: T) => number
    onReorder: (reorderedItems: T[], previousItems: T[]) => void
  }
) {
  const dragState = ref<ListReorderState>({
    isDragging: false,
    draggedId: null,
    draggedIndex: -1,
    insertIndex: -1,
    pointerX: 0,
    pointerY: 0,
    offsetX: 0,
    offsetY: 0,
  })

  const listRef = ref<HTMLElement | null>(null)

  const handleGripDown = (id: number, index: number, event: PointerEvent) => {
    if (event.button !== 0) return
    event.preventDefault()

    // Calculate grab offset from the item's top-left corner
    const itemEl = listRef.value?.querySelector(`[data-item-id="${id}"]`) as HTMLElement | null
    let offsetX = 0
    let offsetY = 0
    if (itemEl) {
      const rect = itemEl.getBoundingClientRect()
      offsetX = event.clientX - rect.left
      offsetY = event.clientY - rect.top
    }

    dragState.value = {
      isDragging: true,
      draggedId: id,
      draggedIndex: index,
      insertIndex: index,
      pointerX: event.clientX,
      pointerY: event.clientY,
      offsetX,
      offsetY,
    }
  }

  const onPointerMove = (event: PointerEvent) => {
    if (!dragState.value.isDragging || !listRef.value) return

    dragState.value.pointerX = event.clientX
    dragState.value.pointerY = event.clientY

    const itemElements = listRef.value.querySelectorAll('[data-item-id]')
    let newInsertIndex = items.value.length

    for (let i = 0; i < itemElements.length; i++) {
      const rect = (itemElements[i] as HTMLElement).getBoundingClientRect()
      const center = rect.top + rect.height / 2
      if (event.clientY < center) {
        newInsertIndex = i
        break
      }
    }

    dragState.value.insertIndex = newInsertIndex
  }

  const onPointerUp = () => {
    if (!dragState.value.isDragging) return

    const { draggedIndex, insertIndex } = dragState.value

    // Reset drag state immediately for snappy feedback
    dragState.value = {
      isDragging: false,
      draggedId: null,
      draggedIndex: -1,
      insertIndex: -1,
      pointerX: 0,
      pointerY: 0,
      offsetX: 0,
      offsetY: 0,
    }

    // Calculate effective drop index (removal shifts indices)
    let dropIndex = insertIndex
    if (draggedIndex < insertIndex) dropIndex--
    if (dropIndex === draggedIndex || dropIndex < 0) return

    // Optimistic reorder
    const previousItems = [...items.value]
    const reordered = [...items.value]
    const [moved] = reordered.splice(draggedIndex, 1)
    reordered.splice(dropIndex, 0, moved)
    items.value = reordered

    options.onReorder(reordered, previousItems)
  }

  onMounted(() => {
    document.addEventListener('pointermove', onPointerMove)
    document.addEventListener('pointerup', onPointerUp)
  })

  onUnmounted(() => {
    document.removeEventListener('pointermove', onPointerMove)
    document.removeEventListener('pointerup', onPointerUp)
  })

  return {
    dragState,
    listRef,
    handleGripDown,
  }
}

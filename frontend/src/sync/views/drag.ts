/**
 * Drag-and-drop for view shapes that group cards into lanes
 * (kanban swimlanes, list rows, calendar slots).
 *
 * Pointer-event based — no library dependency. The architecture
 * doc names @dnd-kit but the existing hand-rolled drag in
 * useKanbanDragDrop already handles the interaction model we
 * need; this module is a clean rewrite against the view-shape
 * contract that adds bulk-select support and drops the project-
 * specific bits.
 *
 * Contract:
 * - Caller supplies lane ids (strings) at composition time.
 * - Caller calls `onPointerDown(cardId, event)` on each card.
 * - Caller renders `state.draggedCardIds`, `state.hoverLane`, and
 *   the floating drag preview at `state.dragPosition`.
 * - On drop into a lane, `onDrop({cardIds, targetLane})` fires
 *   exactly once. If the user releases outside any lane, no
 *   callback runs.
 */
import { onUnmounted, reactive, type Reactive } from 'vue'
import {
  createDragEdgeScroller,
  getDefaultDragScrollTargets,
  type DragEdgeScrollTarget,
} from '@/composables/useDragEdgeScroll'

export interface DragState {
  /** Cards currently being dragged. Single-card drags carry one id;
   * multi-select drags carry every selected id. */
  draggedCardIds: number[]
  hoverLane: string | null
  dragPosition: { x: number; y: number } | null
  isDragging: boolean
}

export interface DropEvent {
  cardIds: number[]
  targetLane: string
}

export interface UseDragDropOptions {
  /** Returns lane id under the pointer, or null if no lane. */
  resolveLaneAt: (clientX: number, clientY: number) => string | null
  /** Fires once on a successful drop into a lane. */
  onDrop: (event: DropEvent) => void
  /** Fires when the user clicks (down → up without movement past
   * the threshold) instead of dragging. Used by the kanban to open
   * the card detail. Optional — omit if click handling is wired
   * separately. */
  onClick?: (cardId: number) => void
  /** Returns the current selection set when a drag begins on a
   * selected card. Used to drag multi-select groups together; if
   * the dragged card isn't in the selection, only that card moves
   * regardless of what else is selected. */
  selection?: () => Set<number>
  /** Pixels of pointer movement before a press becomes a drag.
   * Below this threshold, pointer-up fires `onClick` instead. */
  clickThreshold?: number
  /** Scroll containers to nudge when the pointer nears viewport edges.
   * Defaults to kanban board (horizontal) and column/list bodies (vertical). */
  getEdgeScrollTargets?: (clientX: number, clientY: number) => DragEdgeScrollTarget[]
}

const CLICK_THRESHOLD_PX = 5

export function useDragDrop(options: UseDragDropOptions): {
  state: Reactive<DragState>
  onPointerDown: (cardId: number, event: PointerEvent) => void
  isDraggedCard: (cardId: number) => boolean
  isHoverLane: (laneId: string) => boolean
} {
  const state = reactive<DragState>({
    draggedCardIds: [],
    hoverLane: null,
    dragPosition: null,
    isDragging: false,
  })

  let pointerId: number | null = null
  let startX = 0
  let startY = 0
  let activeCardId: number | null = null
  const clickThreshold = options.clickThreshold ?? CLICK_THRESHOLD_PX
  const getEdgeScrollTargets = options.getEdgeScrollTargets ?? getDefaultDragScrollTargets
  const edgeScroller = createDragEdgeScroller({
    getTargets: getEdgeScrollTargets,
    onTick: (clientX, clientY) => {
      if (state.isDragging) {
        state.hoverLane = options.resolveLaneAt(clientX, clientY)
      }
    },
  })

  function reset(): void {
    edgeScroller.stop()
    state.draggedCardIds = []
    state.hoverLane = null
    state.dragPosition = null
    state.isDragging = false
    pointerId = null
    activeCardId = null
  }

  function onPointerDown(cardId: number, event: PointerEvent): void {
    // Ignore non-primary buttons; touch and pen come through as
    // primary on most browsers.
    if (event.button !== 0) return
    pointerId = event.pointerId
    startX = event.clientX
    startY = event.clientY
    activeCardId = cardId
    state.dragPosition = { x: event.clientX, y: event.clientY }
    window.addEventListener('pointermove', onPointerMove)
    window.addEventListener('pointerup', onPointerUp)
    window.addEventListener('pointercancel', onPointerCancel)
    window.addEventListener('keydown', onKeyDown)
  }

  function onKeyDown(event: KeyboardEvent): void {
    if (event.key !== 'Escape') return
    if (pointerId === null && activeCardId === null) return
    event.preventDefault()
    cleanupListeners()
    document.body.style.userSelect = ''
    reset()
  }

  function onPointerMove(event: PointerEvent): void {
    if (event.pointerId !== pointerId || activeCardId === null) return
    const dx = event.clientX - startX
    const dy = event.clientY - startY
    const moved = Math.hypot(dx, dy) > clickThreshold
    if (!state.isDragging && moved) {
      // Promote press to drag. Compute the dragged set from the
      // selection if the active card is in it; otherwise just the
      // active card. This is what makes "select 5, drag any one of
      // them, all 5 move" work.
      const sel = options.selection?.() ?? null
      if (sel && sel.has(activeCardId)) {
        state.draggedCardIds = [activeCardId, ...Array.from(sel).filter((id) => id !== activeCardId)]
      } else {
        state.draggedCardIds = [activeCardId]
      }
      state.isDragging = true
      // Suppress text selection during drag.
      document.body.style.userSelect = 'none'
      edgeScroller.start()
    }
    if (state.isDragging) {
      state.dragPosition = { x: event.clientX, y: event.clientY }
      state.hoverLane = options.resolveLaneAt(event.clientX, event.clientY)
      edgeScroller.update(event.clientX, event.clientY)
    }
  }

  function onPointerUp(event: PointerEvent): void {
    if (event.pointerId !== pointerId) return
    cleanupListeners()
    document.body.style.userSelect = ''

    if (state.isDragging && state.draggedCardIds.length > 0 && state.hoverLane) {
      const dropEvent: DropEvent = {
        cardIds: [...state.draggedCardIds],
        targetLane: state.hoverLane,
      }
      reset()
      options.onDrop(dropEvent)
      return
    }

    // Below click-threshold pointer move and a captured cardId — treat as
    // a click. Pointer-up outside a lane on an actual drag is treated
    // as cancel (no onClick fires either, the user clearly intended
    // to drag and aborted).
    const wasClick = !state.isDragging && activeCardId !== null
    const clickedCard = activeCardId
    reset()
    if (wasClick && clickedCard !== null) {
      options.onClick?.(clickedCard)
    }
  }

  function onPointerCancel(): void {
    cleanupListeners()
    document.body.style.userSelect = ''
    reset()
  }

  function cleanupListeners(): void {
    window.removeEventListener('pointermove', onPointerMove)
    window.removeEventListener('pointerup', onPointerUp)
    window.removeEventListener('pointercancel', onPointerCancel)
    window.removeEventListener('keydown', onKeyDown)
  }

  // Belt-and-braces: if the component unmounts mid-drag (route
  // change while holding a card), tear down the listeners so we
  // don't leak event subscriptions.
  onUnmounted(() => {
    cleanupListeners()
    edgeScroller.stop()
    document.body.style.userSelect = ''
  })

  function isDraggedCard(cardId: number): boolean {
    return state.draggedCardIds.includes(cardId)
  }

  function isHoverLane(laneId: string): boolean {
    return state.hoverLane === laneId
  }

  return { state, onPointerDown, isDraggedCard, isHoverLane }
}

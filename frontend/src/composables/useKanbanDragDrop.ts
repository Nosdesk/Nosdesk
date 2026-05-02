import { ref, type Ref, onMounted, onUnmounted } from 'vue'
import ticketService from '@/services/ticketService'
import projectService from '@/services/projectService'

export interface KanbanTicket {
  id: number
  title: string
  assignee_uuid?: string | null
  assignee_name?: string | null
  assignee_avatar?: string | null
  requester_uuid?: string | null
  requester_name?: string | null
  requester_avatar?: string | null
  priority: 'low' | 'medium' | 'high'
  status: string
  workflow_state_id?: number
  modified?: string
}

export interface KanbanColumn {
  /**
   * The workflow state category this column represents (e.g.
   * `"backlog"`, `"active"`). The drag handler resolves the column's
   * category to a concrete `workflow_state_id` via
   * `resolveDropStateId` before PATCHing the backend.
   */
  id: string
  title: string
  tickets: KanbanTicket[]
}

interface DragState {
  draggedTicket: { columnId: string; ticketId: number; ticket: KanbanTicket } | null
  dragOverColumn: string | null
  insertIndex: number
  isDragging: boolean
  dropIndicatorY: number | null
  dragPosition: { x: number; y: number } | null
}

export function useKanbanDragDrop(
  columns: Ref<KanbanColumn[]>,
  onRefresh: () => Promise<void>,
  onExternalDrop?: (ticketId: number, targetColumnId: string) => Promise<void>,
  projectId?: Ref<number | null>,
  onTicketClick?: (ticketId: number) => void,
  /**
   * Resolves a column id (a workflow state category like `"backlog"`)
   * to the concrete `workflow_state_id` to PATCH on drop. Returns
   * `null` when no state exists in the category, in which case the
   * drop is rolled back. Optional so callers that haven't migrated
   * yet keep working — the legacy path falls back to the column id
   * as a status string.
   */
  resolveDropStateId?: (columnId: string) => number | null,
) {
  const dragState = ref<DragState>({
    draggedTicket: null,
    dragOverColumn: null,
    insertIndex: -1,
    isDragging: false,
    dropIndicatorY: null,
    dragPosition: null
  })

  // Track if pointer moved (to distinguish click from drag)
  let pointerMoved = false
  let startPos = { x: 0, y: 0 }
  const CLICK_THRESHOLD = 5

  // Touch long-press handling
  let holdTimeout: ReturnType<typeof setTimeout> | null = null
  const TOUCH_HOLD_DELAY = 400 // ms — long enough to not conflict with scrolling
  const MOVE_CANCEL_THRESHOLD = 10 // px — cancel hold if finger moves
  let pendingDrag: { columnId: string; ticket: KanbanTicket; pointerId: number; target: HTMLElement } | null = null

  // Persist ticket order to backend
  const persistTicketOrder = async () => {
    if (!projectId?.value) return

    const allTicketIds: number[] = []
    for (const column of columns.value) {
      for (const ticket of column.tickets) {
        allTicketIds.push(ticket.id)
      }
    }

    try {
      await projectService.updateTicketOrder(projectId.value, allTicketIds)
    } catch (err) {
      console.error('Failed to persist ticket order:', err)
    }
  }

  // Find column element under pointer
  const findColumnAtPoint = (x: number, y: number): HTMLElement | null => {
    const elements = document.elementsFromPoint(x, y)
    return elements.find(el => el.hasAttribute('data-column-id')) as HTMLElement | null
  }

  // Calculate insert position based on pointer location
  const updateInsertPosition = (clientX: number, clientY: number) => {
    const columnElement = findColumnAtPoint(clientX, clientY)

    if (!columnElement) {
      dragState.value.dragOverColumn = null
      dragState.value.dropIndicatorY = null
      return
    }

    const columnId = columnElement.getAttribute('data-column-id')!
    const columnRect = columnElement.getBoundingClientRect()

    dragState.value.dragOverColumn = columnId

    const column = columns.value.find(col => col.id === columnId)
    if (!column || column.tickets.length === 0) {
      dragState.value.insertIndex = 0
      dragState.value.dropIndicatorY = 0
      return
    }

    const ticketElements = columnElement.querySelectorAll('[data-ticket-id]')
    let insertIndex = column.tickets.length
    let indicatorY = 0

    for (let i = 0; i < ticketElements.length; i++) {
      const ticketElement = ticketElements[i] as HTMLElement
      const ticketRect = ticketElement.getBoundingClientRect()
      const ticketCenter = ticketRect.top + ticketRect.height / 2

      if (clientY < ticketCenter) {
        insertIndex = i
        if (i === 0) {
          indicatorY = ticketRect.top - columnRect.top
        } else {
          const prevTicketElement = ticketElements[i - 1] as HTMLElement
          const prevTicketRect = prevTicketElement.getBoundingClientRect()
          indicatorY = (prevTicketRect.bottom + ticketRect.top) / 2 - columnRect.top
        }
        break
      } else if (i === ticketElements.length - 1) {
        indicatorY = ticketRect.bottom - columnRect.top
      }
    }

    // Adjust for dragging within the same column
    if (dragState.value.draggedTicket?.columnId === columnId) {
      const draggedTicketIndex = column.tickets.findIndex(
        t => t.id === dragState.value.draggedTicket?.ticketId
      )
      if (draggedTicketIndex !== -1 && draggedTicketIndex < insertIndex) {
        insertIndex--
      }
    }

    dragState.value.insertIndex = insertIndex
    dragState.value.dropIndicatorY = indicatorY
  }

  // Document-level pointer move handler
  const onPointerMove = (event: PointerEvent) => {
    // If hold timer is pending, check if finger moved too far → cancel
    if (pendingDrag && holdTimeout) {
      const dx = Math.abs(event.clientX - startPos.x)
      const dy = Math.abs(event.clientY - startPos.y)
      if (dx > MOVE_CANCEL_THRESHOLD || dy > MOVE_CANCEL_THRESHOLD) {
        clearTimeout(holdTimeout)
        holdTimeout = null
        pendingDrag = null
      }
      return // let browser handle scroll
    }

    if (!dragState.value.isDragging) return

    // Check if pointer moved beyond click threshold
    const dx = Math.abs(event.clientX - startPos.x)
    const dy = Math.abs(event.clientY - startPos.y)
    if (dx > CLICK_THRESHOLD || dy > CLICK_THRESHOLD) {
      pointerMoved = true
    }

    dragState.value.dragPosition = { x: event.clientX, y: event.clientY }
    updateInsertPosition(event.clientX, event.clientY)
  }

  // Document-level pointer up handler
  const onPointerUp = () => {
    // Clear hold timer if it hasn't fired yet
    if (holdTimeout) {
      clearTimeout(holdTimeout)
      holdTimeout = null
    }
    if (pendingDrag) {
      pendingDrag = null
      // Timer never fired → was a normal tap, not a drag
      return
    }

    if (!dragState.value.isDragging || !dragState.value.draggedTicket) {
      resetDragState()
      return
    }

    // Capture values before resetting state
    const targetColumnId = dragState.value.dragOverColumn
    const sourceColumnId = dragState.value.draggedTicket.columnId
    const draggedTicketId = dragState.value.draggedTicket.ticketId
    const insertIndex = dragState.value.insertIndex
    const wasDrag = pointerMoved

    // Reset UI immediately for snappy feedback
    resetDragState()

    // If pointer didn't move, treat as click
    if (!wasDrag) {
      onTicketClick?.(draggedTicketId)
      return
    }

    // Perform drop if over a column
    if (targetColumnId) {
      const sourceColumn = columns.value.find(col => col.id === sourceColumnId)
      const targetColumn = columns.value.find(col => col.id === targetColumnId)

      if (sourceColumn && targetColumn) {
        const sourceTicketIndex = sourceColumn.tickets.findIndex(t => t.id === draggedTicketId)
        if (sourceTicketIndex !== -1) {
          const [ticket] = sourceColumn.tickets.splice(sourceTicketIndex, 1)
          const finalInsertIndex = Math.max(0, Math.min(insertIndex, targetColumn.tickets.length))
          targetColumn.tickets.splice(finalInsertIndex, 0, ticket)

          // Update backend in background (don't await)
          if (sourceColumnId !== targetColumnId) {
            const newStateId = resolveDropStateId?.(targetColumnId) ?? null
            const patch: Record<string, unknown> = {
              modified: new Date().toISOString(),
            }
            if (newStateId !== null) {
              patch.workflow_state_id = newStateId
              ticket.workflow_state_id = newStateId
            } else {
              // Fall back to the legacy status string for callers that
              // haven't passed `resolveDropStateId` yet.
              patch.status =
                targetColumnId === 'in-progress'
                  ? 'in-progress'
                  : targetColumnId === 'closed'
                    ? 'closed'
                    : 'open'
            }

            ticketService.updateTicket(ticket.id, patch).catch(err => {
              console.error('Failed to update ticket status:', err)
              onRefresh()
            })
          }

          persistTicketOrder()
        }
      }
    }
  }

  const resetDragState = () => {
    dragState.value.draggedTicket = null
    dragState.value.dragOverColumn = null
    dragState.value.insertIndex = -1
    dragState.value.isDragging = false
    dragState.value.dropIndicatorY = null
    dragState.value.dragPosition = null
  }

  // Pointer down on ticket - start drag
  const handlePointerDown = (columnId: string, ticket: KanbanTicket, event: PointerEvent) => {
    if (event.button !== 0) return

    startPos = { x: event.clientX, y: event.clientY }
    pointerMoved = false

    if (event.pointerType === 'mouse') {
      // Desktop: immediate drag (existing behavior)
      event.preventDefault()
      dragState.value.draggedTicket = { columnId, ticketId: ticket.id, ticket }
      dragState.value.isDragging = true
      dragState.value.dragPosition = { x: event.clientX, y: event.clientY }
    } else {
      // Touch/pen: defer drag until hold timer fires
      const target = event.target as HTMLElement
      const pointerId = event.pointerId
      pendingDrag = { columnId, ticket, pointerId, target }
      holdTimeout = setTimeout(() => {
        if (!pendingDrag) return
        dragState.value.draggedTicket = {
          columnId: pendingDrag.columnId,
          ticketId: pendingDrag.ticket.id,
          ticket: pendingDrag.ticket
        }
        dragState.value.isDragging = true
        dragState.value.dragPosition = { x: startPos.x, y: startPos.y }
        // Capture pointer so we don't lose touch events
        try { pendingDrag.target.setPointerCapture(pendingDrag.pointerId) } catch {}
        pendingDrag = null
        holdTimeout = null
        // Haptic feedback
        navigator.vibrate?.(50)
      }, TOUCH_HOLD_DELAY)
    }
  }

  const isDraggedTicket = (ticketId: number): boolean => {
    return dragState.value.draggedTicket?.ticketId === ticketId
  }

  const isColumnDragOver = (columnId: string): boolean => {
    return dragState.value.dragOverColumn === columnId && dragState.value.isDragging
  }

  // Add document listeners on mount
  onMounted(() => {
    document.addEventListener('pointermove', onPointerMove)
    document.addEventListener('pointerup', onPointerUp)
  })

  // Cleanup on unmount
  onUnmounted(() => {
    document.removeEventListener('pointermove', onPointerMove)
    document.removeEventListener('pointerup', onPointerUp)
    if (holdTimeout) {
      clearTimeout(holdTimeout)
      holdTimeout = null
    }
    pendingDrag = null
  })

  return {
    dragState,
    handlePointerDown,
    isDraggedTicket,
    isColumnDragOver
  }
}

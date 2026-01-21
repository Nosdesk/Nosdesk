import { ref, type Ref, onMounted, onUnmounted } from 'vue'
import ticketService from '@/services/ticketService'
import projectService from '@/services/projectService'
import type { TicketStatus } from '@/constants/ticketOptions'

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
  modified?: string
}

export interface KanbanColumn {
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
  onTicketClick?: (ticketId: number) => void
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
            let newStatus: TicketStatus
            switch (targetColumnId) {
              case 'in-progress':
                newStatus = 'in-progress'
                break
              case 'closed':
                newStatus = 'closed'
                break
              case 'open':
              default:
                newStatus = 'open'
                break
            }

            ticketService.updateTicket(ticket.id, {
              status: newStatus,
              modified: new Date().toISOString()
            }).catch(err => {
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

    event.preventDefault()

    startPos = { x: event.clientX, y: event.clientY }
    pointerMoved = false

    dragState.value.draggedTicket = { columnId, ticketId: ticket.id, ticket }
    dragState.value.isDragging = true
    dragState.value.dragPosition = { x: event.clientX, y: event.clientY }
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
  })

  return {
    dragState,
    handlePointerDown,
    isDraggedTicket,
    isColumnDragOver
  }
}

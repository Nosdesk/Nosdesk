import { ref, type Ref, onUnmounted } from 'vue'
import ticketService from '@/services/ticketService'
import type { TicketStatus } from '@/constants/ticketOptions'
import { useTicketDrag } from '@/composables/useTicketDrag'

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
  // Touch drag position for floating preview
  touchDragPosition: { x: number; y: number } | null
  // External drag (from recent tickets)
  isExternalDrag: boolean
}

// Touch drag delay in ms (shorter = more responsive)
const TOUCH_HOLD_DELAY = 150

// Auto-scroll configuration
const SCROLL_EDGE_SIZE = 60 // pixels from edge to trigger scroll
const SCROLL_SPEED = 8 // pixels per frame

export function useKanbanDragDrop(
  columns: Ref<KanbanColumn[]>,
  onRefresh: () => Promise<void>,
  onExternalDrop?: (ticketId: number, targetColumnId: string) => Promise<void>
) {
  const dragState = ref<DragState>({
    draggedTicket: null,
    dragOverColumn: null,
    insertIndex: -1,
    isDragging: false,
    dropIndicatorY: null,
    touchDragPosition: null,
    isExternalDrag: false
  })

  // Get shared drag state for external tickets
  const { dragState: externalDragState, endDrag: endExternalDrag } = useTicketDrag()

  // Touch handling state
  let touchTimeout: ReturnType<typeof setTimeout> | null = null
  let touchStartPos = { x: 0, y: 0 }
  let activeTouchId: number | null = null
  let draggedElement: HTMLElement | null = null

  // Auto-scroll state (shared between mouse and touch)
  let scrollAnimationId: number | null = null
  let currentDragPosition: { x: number; y: number } | null = null

  // Cached scroll container reference
  let scrollContainer: HTMLElement | null = null

  // Find the scrollable parent container (the kanban board's overflow-auto parent)
  const findScrollableParent = (): HTMLElement | null => {
    if (scrollContainer) return scrollContainer

    // Look for the kanban board's scrollable parent
    const kanbanColumns = document.querySelector('[data-column-id]')
    if (kanbanColumns) {
      let parent = kanbanColumns.parentElement
      while (parent) {
        const style = window.getComputedStyle(parent)
        const overflowX = style.overflowX
        const overflowY = style.overflowY
        if (overflowX === 'auto' || overflowX === 'scroll' ||
            overflowY === 'auto' || overflowY === 'scroll') {
          scrollContainer = parent
          return parent
        }
        parent = parent.parentElement
      }
    }

    // Fallback to document element
    return document.documentElement
  }

  // Auto-scroll when near edges (works for both mouse and touch)
  const performAutoScroll = () => {
    if (!currentDragPosition || !dragState.value.isDragging) {
      if (scrollAnimationId) {
        cancelAnimationFrame(scrollAnimationId)
        scrollAnimationId = null
      }
      return
    }

    const container = findScrollableParent()
    if (!container) {
      scrollAnimationId = requestAnimationFrame(performAutoScroll)
      return
    }

    const { x, y } = currentDragPosition
    const containerRect = container.getBoundingClientRect()

    let scrollX = 0
    let scrollY = 0

    // Horizontal scroll - check distance from container edges
    const distFromLeft = x - containerRect.left
    const distFromRight = containerRect.right - x
    const distFromTop = y - containerRect.top
    const distFromBottom = containerRect.bottom - y

    // Horizontal scroll
    if (distFromLeft < SCROLL_EDGE_SIZE && distFromLeft > 0) {
      scrollX = -SCROLL_SPEED * ((SCROLL_EDGE_SIZE - distFromLeft) / SCROLL_EDGE_SIZE)
    } else if (distFromRight < SCROLL_EDGE_SIZE && distFromRight > 0) {
      scrollX = SCROLL_SPEED * ((SCROLL_EDGE_SIZE - distFromRight) / SCROLL_EDGE_SIZE)
    }

    // Vertical scroll
    if (distFromTop < SCROLL_EDGE_SIZE && distFromTop > 0) {
      scrollY = -SCROLL_SPEED * ((SCROLL_EDGE_SIZE - distFromTop) / SCROLL_EDGE_SIZE)
    } else if (distFromBottom < SCROLL_EDGE_SIZE && distFromBottom > 0) {
      scrollY = SCROLL_SPEED * ((SCROLL_EDGE_SIZE - distFromBottom) / SCROLL_EDGE_SIZE)
    }

    // Apply scroll to the container
    if (scrollX !== 0 || scrollY !== 0) {
      container.scrollLeft += scrollX
      container.scrollTop += scrollY
    }

    // Continue animation
    scrollAnimationId = requestAnimationFrame(performAutoScroll)
  }

  const startAutoScroll = () => {
    if (!scrollAnimationId) {
      scrollAnimationId = requestAnimationFrame(performAutoScroll)
    }
  }

  const stopAutoScroll = () => {
    if (scrollAnimationId) {
      cancelAnimationFrame(scrollAnimationId)
      scrollAnimationId = null
    }
    currentDragPosition = null
    scrollContainer = null // Reset cache so it's re-found next drag
  }

  // Global drag tracking for edge scrolling even when not over a column
  const handleGlobalDragOver = (event: DragEvent) => {
    if (dragState.value.isDragging) {
      currentDragPosition = { x: event.clientX, y: event.clientY }
    }
  }

  const handleDragStart = (columnId: string, ticket: KanbanTicket, event: DragEvent) => {
    dragState.value.draggedTicket = { columnId, ticketId: ticket.id, ticket }
    dragState.value.isDragging = true

    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = 'move'
      event.dataTransfer.setData('text/plain', ticket.id.toString())
    }

    // Initialize drag position and start auto-scroll
    currentDragPosition = { x: event.clientX, y: event.clientY }
    startAutoScroll()

    // Add global listener to track cursor even when not over a column
    document.addEventListener('dragover', handleGlobalDragOver)
  }

  const handleDragEnd = () => {
    stopAutoScroll()
    document.removeEventListener('dragover', handleGlobalDragOver)
    dragState.value.draggedTicket = null
    dragState.value.dragOverColumn = null
    dragState.value.insertIndex = -1
    dragState.value.isDragging = false
    dragState.value.dropIndicatorY = null
    dragState.value.touchDragPosition = null
  }

  const handleColumnDragOver = (columnId: string, event: DragEvent) => {
    event.preventDefault()

    // Update drag position for auto-scroll
    currentDragPosition = { x: event.clientX, y: event.clientY }

    // Check for external drag from recent tickets
    const isExternal = externalDragState.value.isDragging && externalDragState.value.source === 'recent-tickets'

    if (!dragState.value.draggedTicket && !isExternal) return

    dragState.value.dragOverColumn = columnId
    dragState.value.isExternalDrag = isExternal
    if (isExternal) {
      dragState.value.isDragging = true
      // Start auto-scroll for external drags too
      if (!scrollAnimationId) {
        startAutoScroll()
      }
    }

    const columnElement = event.currentTarget as HTMLElement
    const columnRect = columnElement.getBoundingClientRect()
    const cursorY = event.clientY

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

      if (cursorY < ticketCenter) {
        insertIndex = i
        if (i === 0) {
          // Before first ticket - position at top
          indicatorY = ticketRect.top - columnRect.top
        } else {
          // Between tickets - position in the middle of the gap
          const prevTicketElement = ticketElements[i - 1] as HTMLElement
          const prevTicketRect = prevTicketElement.getBoundingClientRect()
          indicatorY = (prevTicketRect.bottom + ticketRect.top) / 2 - columnRect.top
        }
        break
      } else if (i === ticketElements.length - 1) {
        // After last ticket - position at bottom
        indicatorY = ticketRect.bottom - columnRect.top
      }
    }

    // Adjust for dragging within the same column (only for internal drags)
    if (!isExternal && dragState.value.draggedTicket?.columnId === columnId) {
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

  const handleColumnDrop = async (targetColumnId: string, event: DragEvent) => {
    event.preventDefault()

    // Check for external drop from recent tickets
    if (dragState.value.isExternalDrag && externalDragState.value.ticket) {
      const ticketId = externalDragState.value.ticket.id
      endExternalDrag()
      handleDragEnd()

      // Call the external drop handler
      if (onExternalDrop) {
        await onExternalDrop(ticketId, targetColumnId)
      }
      return
    }

    if (!dragState.value.draggedTicket) return

    const sourceColumnId = dragState.value.draggedTicket.columnId
    const draggedTicketId = dragState.value.draggedTicket.ticketId
    const insertIndex = dragState.value.insertIndex

    const sourceColumn = columns.value.find(col => col.id === sourceColumnId)
    const targetColumn = columns.value.find(col => col.id === targetColumnId)

    if (!sourceColumn || !targetColumn) {
      handleDragEnd()
      return
    }

    const sourceTicketIndex = sourceColumn.tickets.findIndex(t => t.id === draggedTicketId)
    if (sourceTicketIndex === -1) {
      handleDragEnd()
      return
    }

    const [ticket] = sourceColumn.tickets.splice(sourceTicketIndex, 1)
    const finalInsertIndex = Math.max(0, Math.min(insertIndex, targetColumn.tickets.length))
    targetColumn.tickets.splice(finalInsertIndex, 0, ticket)

    // Update ticket status if moving to a different column
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

      try {
        await ticketService.updateTicket(ticket.id, {
          status: newStatus,
          modified: new Date().toISOString()
        })
      } catch (err) {
        console.error('Failed to update ticket status:', err)
        await onRefresh()
      }
    }

    handleDragEnd()
  }

  const isDraggedTicket = (ticketId: number): boolean => {
    return dragState.value.draggedTicket?.ticketId === ticketId
  }

  const isColumnDragOver = (columnId: string): boolean => {
    const isInternal = dragState.value.dragOverColumn === columnId && dragState.value.isDragging
    const isExternal = dragState.value.dragOverColumn === columnId && dragState.value.isExternalDrag
    return isInternal || isExternal
  }

  const handleColumnDragLeave = (event: DragEvent) => {
    // Only reset when leaving to outside the column (not to a child element)
    const relatedTarget = event.relatedTarget as HTMLElement | null
    const currentTarget = event.currentTarget as HTMLElement

    if (!relatedTarget || !currentTarget.contains(relatedTarget)) {
      if (dragState.value.isExternalDrag) {
        dragState.value.dragOverColumn = null
        dragState.value.dropIndicatorY = null
      }
    }
  }

  // Touch event handlers for mobile
  const handleTouchStart = (columnId: string, ticket: KanbanTicket, event: TouchEvent) => {
    const touch = event.touches[0]
    if (!touch) return

    touchStartPos = { x: touch.clientX, y: touch.clientY }
    activeTouchId = touch.identifier
    draggedElement = event.currentTarget as HTMLElement

    // Start drag after short hold
    touchTimeout = setTimeout(() => {
      dragState.value.draggedTicket = { columnId, ticketId: ticket.id, ticket }
      dragState.value.isDragging = true
      dragState.value.touchDragPosition = { x: touch.clientX, y: touch.clientY }

      // Initialize auto-scroll
      currentDragPosition = { x: touch.clientX, y: touch.clientY }
      startAutoScroll()

      // Haptic feedback if available
      if (navigator.vibrate) {
        navigator.vibrate(50)
      }
    }, TOUCH_HOLD_DELAY)
  }

  const handleTouchMove = (event: TouchEvent) => {
    const touch = Array.from(event.touches).find(t => t.identifier === activeTouchId)
    if (!touch) return

    // Cancel drag initiation if moved too much before hold completed
    if (!dragState.value.isDragging && touchTimeout) {
      const dx = Math.abs(touch.clientX - touchStartPos.x)
      const dy = Math.abs(touch.clientY - touchStartPos.y)
      if (dx > 10 || dy > 10) {
        clearTimeout(touchTimeout)
        touchTimeout = null
        return
      }
    }

    if (!dragState.value.isDragging || !dragState.value.draggedTicket) return

    // Prevent default scrolling while dragging
    event.preventDefault()

    // Update floating preview position and auto-scroll position
    dragState.value.touchDragPosition = { x: touch.clientX, y: touch.clientY }
    currentDragPosition = { x: touch.clientX, y: touch.clientY }

    // Find which column the touch is over
    const elementsAtPoint = document.elementsFromPoint(touch.clientX, touch.clientY)
    const columnContent = elementsAtPoint.find(el =>
      el.hasAttribute('data-column-id')
    ) as HTMLElement | undefined

    if (columnContent) {
      const columnId = columnContent.getAttribute('data-column-id')!
      const columnRect = columnContent.getBoundingClientRect()

      dragState.value.dragOverColumn = columnId

      const column = columns.value.find(col => col.id === columnId)
      if (!column || column.tickets.length === 0) {
        dragState.value.insertIndex = 0
        dragState.value.dropIndicatorY = 0
        return
      }

      const ticketElements = columnContent.querySelectorAll('[data-ticket-id]')
      let insertIndex = column.tickets.length
      let indicatorY = 0

      for (let i = 0; i < ticketElements.length; i++) {
        const ticketElement = ticketElements[i] as HTMLElement
        const ticketRect = ticketElement.getBoundingClientRect()
        const ticketCenter = ticketRect.top + ticketRect.height / 2

        if (touch.clientY < ticketCenter) {
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
      if (dragState.value.draggedTicket.columnId === columnId) {
        const draggedTicketIndex = column.tickets.findIndex(
          t => t.id === dragState.value.draggedTicket?.ticketId
        )
        if (draggedTicketIndex !== -1 && draggedTicketIndex < insertIndex) {
          insertIndex--
        }
      }

      dragState.value.insertIndex = insertIndex
      dragState.value.dropIndicatorY = indicatorY
    } else {
      dragState.value.dragOverColumn = null
      dragState.value.dropIndicatorY = null
    }
  }

  const handleTouchEnd = async (event: TouchEvent) => {
    // Stop auto-scroll
    stopAutoScroll()

    if (touchTimeout) {
      clearTimeout(touchTimeout)
      touchTimeout = null
    }

    if (!dragState.value.isDragging || !dragState.value.draggedTicket) {
      activeTouchId = null
      draggedElement = null
      return
    }

    // Perform drop if over a column
    if (dragState.value.dragOverColumn) {
      const targetColumnId = dragState.value.dragOverColumn
      const sourceColumnId = dragState.value.draggedTicket.columnId
      const draggedTicketId = dragState.value.draggedTicket.ticketId
      const insertIndex = dragState.value.insertIndex

      const sourceColumn = columns.value.find(col => col.id === sourceColumnId)
      const targetColumn = columns.value.find(col => col.id === targetColumnId)

      if (sourceColumn && targetColumn) {
        const sourceTicketIndex = sourceColumn.tickets.findIndex(t => t.id === draggedTicketId)
        if (sourceTicketIndex !== -1) {
          const [ticket] = sourceColumn.tickets.splice(sourceTicketIndex, 1)
          const finalInsertIndex = Math.max(0, Math.min(insertIndex, targetColumn.tickets.length))
          targetColumn.tickets.splice(finalInsertIndex, 0, ticket)

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

            try {
              await ticketService.updateTicket(ticket.id, {
                status: newStatus,
                modified: new Date().toISOString()
              })
            } catch (err) {
              console.error('Failed to update ticket status:', err)
              await onRefresh()
            }
          }
        }
      }
    }

    // Reset state
    dragState.value.draggedTicket = null
    dragState.value.dragOverColumn = null
    dragState.value.insertIndex = -1
    dragState.value.isDragging = false
    dragState.value.dropIndicatorY = null
    dragState.value.touchDragPosition = null
    activeTouchId = null
    draggedElement = null
  }

  const handleTouchCancel = () => {
    // Stop auto-scroll
    stopAutoScroll()

    if (touchTimeout) {
      clearTimeout(touchTimeout)
      touchTimeout = null
    }
    dragState.value.draggedTicket = null
    dragState.value.dragOverColumn = null
    dragState.value.insertIndex = -1
    dragState.value.isDragging = false
    dragState.value.dropIndicatorY = null
    dragState.value.touchDragPosition = null
    activeTouchId = null
    draggedElement = null
  }

  // Cleanup
  onUnmounted(() => {
    stopAutoScroll()
    document.removeEventListener('dragover', handleGlobalDragOver)
    if (touchTimeout) {
      clearTimeout(touchTimeout)
    }
  })

  return {
    dragState,
    handleDragStart,
    handleDragEnd,
    handleColumnDragOver,
    handleColumnDragLeave,
    handleColumnDrop,
    handleTouchStart,
    handleTouchMove,
    handleTouchEnd,
    handleTouchCancel,
    isDraggedTicket,
    isColumnDragOver
  }
}

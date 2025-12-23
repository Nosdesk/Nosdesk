import { ref, readonly } from 'vue'

export interface DraggableTicket {
  id: number
  title: string
  status?: string
  priority?: 'low' | 'medium' | 'high'
  assignee?: string | null
}

interface TicketDragState {
  isDragging: boolean
  ticket: DraggableTicket | null
  source: 'recent-tickets' | 'kanban' | null
  position: { x: number; y: number } | null
}

// Shared singleton state for cross-component drag operations
const dragState = ref<TicketDragState>({
  isDragging: false,
  ticket: null,
  source: null,
  position: null
})

// Touch handling state
let touchTimeout: ReturnType<typeof setTimeout> | null = null
let activeTouchId: number | null = null
let touchStartPos = { x: 0, y: 0 }

const TOUCH_HOLD_DELAY = 150

export function useTicketDrag() {
  const startDrag = (ticket: DraggableTicket, source: 'recent-tickets' | 'kanban', position?: { x: number; y: number }) => {
    dragState.value = {
      isDragging: true,
      ticket,
      source,
      position: position || null
    }
  }

  const updatePosition = (x: number, y: number) => {
    if (dragState.value.isDragging) {
      dragState.value.position = { x, y }
    }
  }

  const endDrag = () => {
    dragState.value = {
      isDragging: false,
      ticket: null,
      source: null,
      position: null
    }
    if (touchTimeout) {
      clearTimeout(touchTimeout)
      touchTimeout = null
    }
    activeTouchId = null
  }

  // HTML5 Drag handlers for desktop
  const handleDragStart = (ticket: DraggableTicket, source: 'recent-tickets' | 'kanban', event: DragEvent) => {
    startDrag(ticket, source)
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = 'move'
      event.dataTransfer.setData('application/json', JSON.stringify({
        ticketId: ticket.id,
        source
      }))
      // Set a custom drag image (transparent 1x1 pixel)
      const img = new Image()
      img.src = 'data:image/gif;base64,R0lGODlhAQABAIAAAAUEBAAAACwAAAAAAQABAAACAkQBADs='
      event.dataTransfer.setDragImage(img, 0, 0)
    }
  }

  const handleDrag = (event: DragEvent) => {
    if (event.clientX && event.clientY) {
      updatePosition(event.clientX, event.clientY)
    }
  }

  const handleDragEnd = () => {
    endDrag()
  }

  // Touch handlers for mobile
  const handleTouchStart = (ticket: DraggableTicket, source: 'recent-tickets' | 'kanban', event: TouchEvent) => {
    const touch = event.touches[0]
    if (!touch) return

    touchStartPos = { x: touch.clientX, y: touch.clientY }
    activeTouchId = touch.identifier

    touchTimeout = setTimeout(() => {
      startDrag(ticket, source, { x: touch.clientX, y: touch.clientY })
      // Haptic feedback
      if (navigator.vibrate) {
        navigator.vibrate(50)
      }
    }, TOUCH_HOLD_DELAY)
  }

  const handleTouchMove = (event: TouchEvent) => {
    const touch = Array.from(event.touches).find(t => t.identifier === activeTouchId)
    if (!touch) return

    // Cancel if moved before hold completed
    if (!dragState.value.isDragging && touchTimeout) {
      const dx = Math.abs(touch.clientX - touchStartPos.x)
      const dy = Math.abs(touch.clientY - touchStartPos.y)
      if (dx > 10 || dy > 10) {
        clearTimeout(touchTimeout)
        touchTimeout = null
        return
      }
    }

    if (dragState.value.isDragging) {
      event.preventDefault()
      updatePosition(touch.clientX, touch.clientY)
    }
  }

  const handleTouchEnd = () => {
    if (touchTimeout) {
      clearTimeout(touchTimeout)
      touchTimeout = null
    }
    // Don't end drag here - let the drop handler do it
  }

  const handleTouchCancel = () => {
    endDrag()
  }

  return {
    dragState: readonly(dragState),
    startDrag,
    updatePosition,
    endDrag,
    handleDragStart,
    handleDrag,
    handleDragEnd,
    handleTouchStart,
    handleTouchMove,
    handleTouchEnd,
    handleTouchCancel
  }
}

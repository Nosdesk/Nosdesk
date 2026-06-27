import { ref, readonly } from 'vue'
import type { WorkflowStateCategory } from '@nosdesk/core/types/workflow'
import { shareableRouteUrl } from '@/utils/shareUrl'
import { createDragEdgeScroller } from '@/composables/useDragEdgeScroll'

export interface DraggableTicket {
  id: number
  title: string
  category?: WorkflowStateCategory
  assigneeUuid?: string | null
  priority?: 'low' | 'medium' | 'high' | 'none'
}

/** Parse a ticket id from an HTML5 drag payload (sidebar, kanban). */
export function parseTicketDragTransfer(transfer: DataTransfer | null): number | null {
  if (!transfer) return null

  const jsonData = transfer.getData('application/json')
  if (jsonData) {
    try {
      const data = JSON.parse(jsonData) as { ticketId?: number }
      if (typeof data.ticketId === 'number') return data.ticketId
    } catch {
      // ignore malformed payload
    }
  }

  const text = transfer.getData('text/plain')
  if (text) {
    const match = text.trim().match(/\/tickets\/(\d+)/)
    if (match) return Number.parseInt(match[1], 10)
  }

  return null
}

/** True when the drag carries in-app ticket data we can drop. */
export function isTicketDragEvent(event: DragEvent): boolean {
  const types = event.dataTransfer?.types ?? []
  return types.includes('application/json')
    || types.includes('text/uri-list')
    || types.includes('text/plain')
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
let activeDragSource: HTMLElement | null = null
/** Set when Escape aborts a drag; drop handlers must ignore the next drop. */
let dropSuppressed = false

let edgeScrollTickHandler: ((clientX: number, clientY: number) => void) | null = null
const edgeScroller = createDragEdgeScroller({
  onTick: (clientX, clientY) => edgeScrollTickHandler?.(clientX, clientY),
})

/** Called each auto-scroll frame so drop targets can re-resolve under a fixed pointer. */
export function onDragEdgeScrollTick(
  handler: ((clientX: number, clientY: number) => void) | null,
): void {
  edgeScrollTickHandler = handler
}

const TOUCH_HOLD_DELAY = 150

function onDocumentDragKeyDown(event: KeyboardEvent): void {
  if (event.key !== 'Escape' || !dragState.value.isDragging) return
  event.preventDefault()
  event.stopPropagation()
  cancelActiveDrag()
}

function attachDragSessionListeners(): void {
  document.addEventListener('keydown', onDocumentDragKeyDown, true)
}

function detachDragSessionListeners(): void {
  document.removeEventListener('keydown', onDocumentDragKeyDown, true)
}

function resetDragState(): void {
  edgeScroller.stop()
  dragState.value = {
    isDragging: false,
    ticket: null,
    source: null,
    position: null
  }
  document.body.classList.remove('cursor-grabbing')
  detachDragSessionListeners()
  activeDragSource = null
  if (touchTimeout) {
    clearTimeout(touchTimeout)
    touchTimeout = null
  }
  activeTouchId = null
}

/** True once after Escape (or programmatic cancel); clears the flag. */
export function shouldSuppressTicketDrop(): boolean {
  if (!dropSuppressed) return false
  dropSuppressed = false
  return true
}

function cancelActiveDrag(): void {
  if (!dragState.value.isDragging) return
  dropSuppressed = true
  const source = activeDragSource
  resetDragState()
  if (source) {
    source.dispatchEvent(new DragEvent('dragend', { bubbles: true, cancelable: true }))
  }
}

/** Hide the browser's default drag ghost — callers render TicketDragPreview. */
function setTransparentDragImage(event: DragEvent): void {
  if (!event.dataTransfer) return
  const ghost = document.createElement('div')
  ghost.style.cssText = 'position:fixed;top:0;left:0;width:1px;height:1px;opacity:0;pointer-events:none;'
  document.body.appendChild(ghost)
  event.dataTransfer.setDragImage(ghost, 0, 0)
  requestAnimationFrame(() => ghost.remove())
}

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
      edgeScroller.update(x, y)
    }
  }

  const endDrag = () => {
    resetDragState()
  }

  // HTML5 Drag handlers for desktop
  const handleDragStart = (ticket: DraggableTicket, source: 'recent-tickets' | 'kanban', event: DragEvent) => {
    dropSuppressed = false
    activeDragSource = event.currentTarget instanceof HTMLElement ? event.currentTarget : null
    attachDragSessionListeners()
    startDrag(ticket, source)
    edgeScroller.start()
    if (event.dataTransfer) {
      // Allow all effects for maximum compatibility with external apps
      event.dataTransfer.effectAllowed = 'all'

      // Build ticket URL (workspace-scoped in path mode)
      const ticketUrl = shareableRouteUrl('ticket-view', { id: String(ticket.id) })
      const ticketLabel = `#${ticket.id} ${ticket.title}`

      // Set multiple data formats for maximum compatibility
      // text/plain - most apps use this (Slack, Discord, etc.)
      event.dataTransfer.setData('text/plain', ticketUrl)

      // text/uri-list - URL-aware apps
      event.dataTransfer.setData('text/uri-list', ticketUrl)

      // text/html - apps that support rich text (creates clickable link)
      event.dataTransfer.setData('text/html', `<a href="${ticketUrl}">${ticketLabel}</a>`)

      // Internal app data for in-app drops
      event.dataTransfer.setData('application/json', JSON.stringify({
        ticketId: ticket.id,
        source
      }))

      setTransparentDragImage(event)
    }
  }

  const handleDrag = (event: DragEvent) => {
    if (event.clientX !== 0 || event.clientY !== 0) {
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
      dropSuppressed = false
      attachDragSessionListeners()
      startDrag(ticket, source, { x: touch.clientX, y: touch.clientY })
      edgeScroller.start()
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
    cancelDrag: cancelActiveDrag,
    handleDragStart,
    handleDrag,
    handleDragEnd,
    handleTouchStart,
    handleTouchMove,
    handleTouchEnd,
    handleTouchCancel
  }
}

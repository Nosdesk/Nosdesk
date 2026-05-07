import { onMounted, onUnmounted, onActivated, onDeactivated, type Ref } from 'vue'
import { useSSE, type SSEEventType } from '@/services/sseService'
import { useAuthStore } from '@/stores/auth'
import {
  unwrapEventData,
  type TicketUpdatedEventData,
  type TicketDeletedEventData,
  type ProjectEventData,
} from '@/types/sse'

const DEBUG_SSE = import.meta.env.DEV && import.meta.env.VITE_DEBUG_SSE === 'true'

export interface ProjectSSECallbacks {
  onTicketAssigned?: (ticketId: number, projectId: number) => void
  onTicketUnassigned?: (ticketId: number, projectId: number) => void
  onTicketUpdated?: (data: TicketUpdatedEventData) => void
  onTicketDeleted?: (ticketId: number) => void
}

/**
 * Composable for handling SSE events scoped to a project view.
 *
 * Listens for project-assigned, project-unassigned, ticket-updated,
 * and ticket-deleted events. Filters by projectId and projectTicketIds
 * so only relevant events reach the callbacks.
 */
export function useProjectSSE(
  projectId: Ref<number>,
  projectTicketIds: Ref<Set<number>>,
  callbacks: ProjectSSECallbacks,
) {
  const { addEventListener, removeEventListener, isConnected, connect } = useSSE()
  const authStore = useAuthStore()

  function handleProjectAssigned(rawData: unknown): void {
    const data = unwrapEventData(rawData as ProjectEventData)
    if (data.project_id !== projectId.value) return

    if (DEBUG_SSE) console.log('[SSE:Project] project-assigned ticket:', data.ticket_id)
    projectTicketIds.value.add(data.ticket_id)
    callbacks.onTicketAssigned?.(data.ticket_id, data.project_id)
  }

  function handleProjectUnassigned(rawData: unknown): void {
    const data = unwrapEventData(rawData as ProjectEventData)
    if (data.project_id !== projectId.value) return

    if (DEBUG_SSE) console.log('[SSE:Project] project-unassigned ticket:', data.ticket_id)
    projectTicketIds.value.delete(data.ticket_id)
    callbacks.onTicketUnassigned?.(data.ticket_id, data.project_id)
  }

  function handleTicketUpdated(rawData: unknown): void {
    const data = unwrapEventData(rawData as TicketUpdatedEventData)
    if (!projectTicketIds.value.has(data.ticket_id)) return

    if (DEBUG_SSE) console.log('[SSE:Project] ticket-updated:', data.ticket_id, data.field)
    callbacks.onTicketUpdated?.(data)
  }

  function handleTicketDeleted(rawData: unknown): void {
    const data = unwrapEventData(rawData as TicketDeletedEventData)
    if (!projectTicketIds.value.has(data.ticket_id)) return

    if (DEBUG_SSE) console.log('[SSE:Project] ticket-deleted:', data.ticket_id)
    projectTicketIds.value.delete(data.ticket_id)
    callbacks.onTicketDeleted?.(data.ticket_id)
  }

  type SSEHandler = (data: unknown) => void
  const eventHandlers: [SSEEventType, SSEHandler][] = [
    ['project-assigned', handleProjectAssigned],
    ['project-unassigned', handleProjectUnassigned],
    ['ticket-updated', handleTicketUpdated],
    ['ticket-deleted', handleTicketDeleted],
  ]

  function setupListeners() {
    for (const [event, handler] of eventHandlers) {
      addEventListener(event, handler)
    }
  }

  function cleanupListeners() {
    for (const [event, handler] of eventHandlers) {
      removeEventListener(event, handler)
    }
  }

  onMounted(async () => {
    setupListeners()
    if (authStore.isAuthenticated && !isConnected.value) {
      await connect()
    }
  })

  onUnmounted(() => {
    cleanupListeners()
  })

  // KeepAlive support
  onActivated(() => {
    setupListeners()
  })

  onDeactivated(() => {
    cleanupListeners()
  })

  return { isConnected }
}

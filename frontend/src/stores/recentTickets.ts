import { defineStore } from 'pinia'
import { logger } from '@/utils/logger';
import { ref } from 'vue'
import ticketService from '@/services/ticketService'
import type { RecentTicket } from '@/types/ticket'

export const useRecentTicketsStore = defineStore('recentTickets', () => {
  const recentTickets = ref<RecentTicket[]>([])
  const isLoading = ref(false)
  const error = ref<string | null>(null)
  // Track recently-removed ticket IDs so recordTicketView won't immediately re-add them
  const removedTicketIds = ref<Set<number>>(new Set())

  // Fetch recent tickets from the server
  const fetchRecentTickets = async () => {
    isLoading.value = true
    error.value = null

    try {
      const tickets = await ticketService.getRecentTickets()
      // Filter out any tickets the user has explicitly removed during this session
      recentTickets.value = removedTicketIds.value.size > 0
        ? tickets.filter(t => !removedTicketIds.value.has(t.id))
        : tickets

      if (import.meta.env.DEV) {
        logger.debug(`Fetched ${tickets.length} recent tickets from server`)
      }
    } catch (err) {
      error.value = 'Failed to fetch recent tickets'
      logger.error('Error fetching recent tickets:', err)
    } finally {
      isLoading.value = false
    }
  }

  // Record that a ticket was viewed (automatically updates server)
  const recordTicketView = async (ticketId: number) => {
    // Skip if this ticket was recently removed from the list by the user
    if (removedTicketIds.value.has(ticketId)) return

    try {
      await ticketService.recordTicketView(ticketId)

      // Refresh the recent tickets list to reflect the new view
      await fetchRecentTickets()

      if (import.meta.env.DEV) {
        logger.debug(`Recorded view for ticket #${ticketId}`)
      }
    } catch (err) {
      logger.error(`Error recording view for ticket #${ticketId}:`, err)
    }
  }

  // Update ticket data in the local cache (after changes)
  const updateTicketData = (ticketId: number, updatedData: Partial<RecentTicket>) => {
    const ticketIndex = recentTickets.value.findIndex(t => t.id === ticketId)

    if (ticketIndex !== -1) {
      // Object.assign mutates in-place, preserving object reference and Vue reactivity
      Object.assign(recentTickets.value[ticketIndex], updatedData)

      if (import.meta.env.DEV) {
        logger.debug(`Updated ticket #${ticketId} in recent tickets cache`)
      }
    }
  }

  // Remove a ticket from the recent list
  const removeTicket = async (ticketId: number) => {
    // Mark as removed so concurrent/future recordTicketView calls won't re-add it
    removedTicketIds.value.add(ticketId)
    // Optimistic removal from local list
    recentTickets.value = recentTickets.value.filter(t => t.id !== ticketId)
    try {
      await ticketService.removeRecentTicket(ticketId)
      // Server confirmed deletion — clear suppression so future views work normally
      removedTicketIds.value.delete(ticketId)
    } catch (err) {
      logger.error(`Error removing ticket #${ticketId} from recent:`, err)
      // Server delete failed — undo the suppression and re-fetch to restore correct state
      removedTicketIds.value.delete(ticketId)
      await fetchRecentTickets()
    }
  }

  // Reorder tickets in the list (local only - persists until next fetch)
  const reorderTickets = (fromIndex: number, toIndex: number) => {
    if (fromIndex === toIndex) return
    if (fromIndex < 0 || fromIndex >= recentTickets.value.length) return
    if (toIndex < 0 || toIndex >= recentTickets.value.length) return

    const tickets = [...recentTickets.value]
    const [moved] = tickets.splice(fromIndex, 1)
    tickets.splice(toIndex, 0, moved)
    recentTickets.value = tickets

    if (import.meta.env.DEV) {
      logger.debug(`Reordered ticket from index ${fromIndex} to ${toIndex}`)
    }
  }

  return {
    recentTickets,
    isLoading,
    error,
    fetchRecentTickets,
    recordTicketView,
    updateTicketData,
    removeTicket,
    reorderTickets
  }
})
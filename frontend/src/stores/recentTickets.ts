import { defineStore } from 'pinia'
import { logger } from '@/utils/logger';
import { ref } from 'vue'
import ticketService from '@/services/ticketService'

interface Ticket {
  id: number;
  title: string;
  status: 'open' | 'in-progress' | 'closed';
  requester?: string;
  assignee?: string;
  created_at?: string;
  updated_at?: string;
  last_viewed_at?: string;
  view_count?: number;
}

export const useRecentTicketsStore = defineStore('recentTickets', () => {
  const recentTickets = ref<Ticket[]>([])
  const isLoading = ref(false)
  const error = ref<string | null>(null)

  // Fetch recent tickets from the server
  const fetchRecentTickets = async () => {
    isLoading.value = true
    error.value = null

    try {
      const tickets = await ticketService.getRecentTickets()
      recentTickets.value = tickets

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
  const updateTicketData = (ticketId: number, updatedData: Partial<Ticket>) => {
    const ticketIndex = recentTickets.value.findIndex(t => t.id === ticketId)

    if (ticketIndex !== -1) {
      // Use direct mutation to preserve object reference
      const ticket = recentTickets.value[ticketIndex]
      Object.keys(updatedData).forEach(key => {
        (ticket as any)[key] = (updatedData as any)[key]
      })

      if (import.meta.env.DEV) {
        logger.debug(`Updated ticket #${ticketId} in recent tickets cache`)
      }
    }
  }

  return {
    recentTickets,
    isLoading,
    error,
    fetchRecentTickets,
    recordTicketView,
    updateTicketData
  }
})
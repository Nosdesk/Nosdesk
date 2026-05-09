/**
 * Wires the site-header's "Create Ticket" button to the standard
 * empty-ticket-then-detail-page flow for the calling view.
 *
 * Three list-style views (Dashboard, Tickets list, an open
 * Ticket detail) all need the same handler: ask the backend to
 * mint an empty ticket row and route to its detail page where
 * the user fills it in inline. Centralising here keeps the
 * shape consistent and makes future changes (e.g. swapping to a
 * modal create flow, adding a toast on failure) a one-file edit.
 */
import { useRouter } from 'vue-router'
import ticketService from '@/services/ticketService'
import { usePageCreateAction } from '@/composables/usePageCreateAction'

export function useCreateTicketAction(): void {
  const router = useRouter()

  usePageCreateAction(async () => {
    try {
      const ticket = await ticketService.createEmptyTicket()
      await router.push(`/tickets/${ticket.id}`)
    } catch (err) {
      console.error('Failed to create empty ticket:', err)
    }
  })
}

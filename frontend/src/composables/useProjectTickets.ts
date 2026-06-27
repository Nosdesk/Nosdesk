import { computed, toValue, type ComputedRef, type MaybeRefOrGetter } from 'vue'
import { useSyncTicketsStore, type SyncTicket } from '@/sync/stores/tickets'
import { useAggregate } from '@nosdesk/core/sync/composables'
import { toCardData } from '@/sync/views/cardData'
import type { CardData } from '@nosdesk/core/sync/views/types'

/** A project<->ticket association row as it lands in the sync pool. */
export interface ProjectTicketAssoc {
  project_id: number
  ticket_id: number
  display_order: number
}

/**
 * The tickets linked to a project, sourced from the sync pool's
 * `project_ticket` aggregate joined against the ticket pool and ordered
 * by `display_order`. Returns both the raw `SyncTicket`s and their
 * `CardData` projection so the board, gantt, and cycles views share one
 * materialisation instead of each re-deriving it (and drifting on the
 * ordering, which is exactly what happened before this was extracted).
 *
 * `projectId` may be a ref/getter so the result tracks route changes.
 */
export function useProjectTickets(projectId: MaybeRefOrGetter<number>): {
  tickets: ComputedRef<SyncTicket[]>
  cards: ComputedRef<CardData[]>
} {
  const ticketsStore = useSyncTicketsStore()
  const associations = useAggregate<ProjectTicketAssoc>('project_ticket')

  const tickets = computed<SyncTicket[]>(() => {
    const pid = toValue(projectId)
    return associations.value
      .filter((a) => a.project_id === pid)
      .sort((a, b) => a.display_order - b.display_order)
      .map((a) => ticketsStore.byId(a.ticket_id).value)
      .filter((t): t is SyncTicket => t != null)
  })

  const cards = computed<CardData[]>(() =>
    tickets.value.map((t) => toCardData(t)).filter((c): c is CardData => c != null),
  )

  return { tickets, cards }
}

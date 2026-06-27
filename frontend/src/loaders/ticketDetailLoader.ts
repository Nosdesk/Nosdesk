/**
 * Vue Router Data Loader for the /tickets/:id route.
 *
 * Mirrors `ticketsListLoader` and `inboxLoader`: the loader runs
 * during navigation, before the route component finishes
 * mounting, and primes a Pinia Colada cache entry the view's
 * data composable consumes synchronously on mount.
 *
 * Without this loader the lifecycle is:
 *   1. navigation lands TicketView with `ticket = null`
 *   2. component mounts, fires fetchTicket → skeleton renders
 *   3. fetch resolves → real content swaps in → layout shifts
 *
 * With it, in-app navigations land the route with the ticket
 * already in cache, so the view mounts directly into the real
 * content. Cold loads (deep-link, refresh) still hit the
 * skeleton — the loader's promise gates the navigation, but the
 * subsequent mount still sees populated data.
 *
 * The cache key is the same one `useTicketData.fetchTicket`
 * checks before going to network. Drift between the loader's
 * key and the consumer's key would silently orphan the prime
 * (cf. ticketsListLoader's caveat); centralising the key in
 * `TICKET_DETAIL_KEY` here is the simplest way to keep them
 * in lockstep.
 */
import { defineColadaLoader } from 'vue-router/experimental/pinia-colada'
import { useQueryCache } from '@pinia/colada'
import ticketService from '@nosdesk/core/services/ticketService'
import type { Ticket } from '@nosdesk/core/types/ticket'

/** Build the cache key for a single ticket's detail payload.
 * Both this loader and `useTicketData.fetchTicket` import this
 * helper so they can never drift. */
export function ticketDetailKey(id: number | string): readonly (string | number)[] {
  return ['tickets', 'detail', Number(id)] as const
}

export const useTicketDetailLoader = defineColadaLoader<Ticket | null>({
  key: (to) => ['tickets', 'detail-loader', String(to.params.id)],

  async query(to) {
    const id = Number(to.params.id)
    if (!Number.isFinite(id)) return null

    const fetched = await ticketService.getTicketById(id)
    if (!fetched) return null

    const queryCache = useQueryCache()
    queryCache.setQueryData(ticketDetailKey(id), fetched)

    return fetched
  },
})

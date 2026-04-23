/**
 * Ticket-backed stat widget plumbing.
 *
 * Thin wrapper over `useAsyncResource`: fetches the full ticket set,
 * passes it through a caller-supplied `compute` to derive stats, and
 * re-derives on any ticket-* SSE event (coalesced so a burst fires a
 * single refetch).
 *
 * Callers only own the `compute` function and the initial/error
 * strings; loading, error handling, and live-update wiring are
 * standardised so the stat widgets that use this composable are just
 * a pure data transform plus a render template.
 */
import { getTickets, type Ticket } from '@/services/ticketService'
import { useSSEListeners } from '@/composables/useSSEListeners'
import { useAsyncResource, type UseAsyncResourceResult } from '@/composables/useAsyncResource'

const REFETCH_COALESCE_MS = 400

export function useTicketStats<T>(
  compute: (tickets: Ticket[]) => T,
  initial: T,
  errorLabel = 'Failed to load',
): UseAsyncResourceResult<T> {
  const resource = useAsyncResource(
    async () => compute(await getTickets()),
    initial,
    errorLabel,
  )

  // Coalesced SSE refetch. Any ticket-* event triggers one `reload`
  // REFETCH_COALESCE_MS after the last event in a burst lands.
  let timer: ReturnType<typeof setTimeout> | null = null
  const scheduleRefetch = () => {
    if (timer) clearTimeout(timer)
    timer = setTimeout(resource.reload, REFETCH_COALESCE_MS)
  }

  const { on } = useSSEListeners()
  on('ticket-updated', scheduleRefetch)
  on('ticket-created', scheduleRefetch)
  on('ticket-deleted', scheduleRefetch)

  return resource
}

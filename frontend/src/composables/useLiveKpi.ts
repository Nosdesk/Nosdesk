/**
 * Live refresh for KPI / chart queries that derive from ticket
 * state.
 *
 * Listens to the dashboard SSE channel for events that mutate the
 * tickets table (`ticket-created`, `ticket-updated`,
 * `ticket-deleted`), and on receipt schedules a refetch on the
 * caller's query handle. The refetch is debounced (250 ms) so a
 * burst of state transitions doesn't kick off a refetch storm.
 *
 * The composable owns its listener lifecycle: it attaches on mount
 * and detaches on unmount, so callers just call it once at setup.
 *
 * Why this lives here (and not inside `useQuery`'s `staleTime`):
 * Pinia Colada's stale time is a passive trigger — it refetches on
 * mount or window focus, not on backend state changes. For a
 * dashboard tab the user leaves open while triaging tickets, we
 * want the headline number to be live without them clicking
 * refresh. This is the active push, complementing the passive
 * useQuery cache.
 */
import { onBeforeUnmount, onMounted } from 'vue'
import { useSSE } from '@/services/sseService'

const TICKET_EVENTS = ['ticket-created', 'ticket-updated', 'ticket-deleted'] as const

const DEBOUNCE_MS = 250

export interface LiveKpiOptions {
  /** Called when a relevant SSE event has fired (debounced). The
   *  caller usually wires this to `query.refetch()` on its Pinia
   *  Colada handle. */
  onRefresh: () => void
}

export function useLiveKpi({ onRefresh }: LiveKpiOptions) {
  const sse = useSSE()

  let timer: ReturnType<typeof setTimeout> | null = null
  function schedule() {
    if (timer) clearTimeout(timer)
    timer = setTimeout(() => {
      timer = null
      onRefresh()
    }, DEBOUNCE_MS)
  }

  // The SSE service expects per-event-name listeners. Attach the
  // same coalescing scheduler to each event we care about so any
  // ticket mutation triggers a single trailing refetch.
  function onEvent() {
    schedule()
  }

  onMounted(() => {
    for (const name of TICKET_EVENTS) {
      sse.addEventListener(name, onEvent)
    }
  })

  onBeforeUnmount(() => {
    for (const name of TICKET_EVENTS) {
      sse.removeEventListener(name, onEvent)
    }
    if (timer) clearTimeout(timer)
  })
}

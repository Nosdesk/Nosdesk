/**
 * Numbers for tickets known only by id (merge history, references). The pool
 * answers for a ticket it holds; any other is fetched once and remembered.
 * An id's number never changes, so the memo outlives a workspace switch.
 */
import { reactive } from 'vue'
import { knownTicketNumber, ticketNumberForId } from '@/utils/ticketNumbers'

// null: the ticket couldn't be read (deleted, or out of reach).
const fetched = reactive(new Map<number, number | null>())
const inFlight = new Set<number>()

/**
 * The number of the ticket with this id, or `undefined` while it's being
 * looked up (or when it can't be). Reactive: a template or `computed` reading
 * it updates when the lookup lands.
 */
export function numberForTicketId(id: number): number | undefined {
  const known = knownTicketNumber({ id })
  if (known !== undefined) return known
  const remembered = fetched.get(id)
  if (remembered !== undefined) return remembered ?? undefined
  if (!inFlight.has(id)) {
    inFlight.add(id)
    void ticketNumberForId(id).then((number) => {
      fetched.set(id, number ?? null)
      inFlight.delete(id)
    })
  }
  return undefined
}

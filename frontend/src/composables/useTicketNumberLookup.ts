/**
 * Numbers and titles for tickets known only by id (merge history,
 * references). The pool answers for a ticket it holds; any other is fetched
 * once and remembered. An id's number never changes, so the memo outlives a
 * workspace switch; a title read this way can be a little behind.
 */
import { reactive } from 'vue'
import * as pool from '@nosdesk/core/sync/pool'
import { knownTicketNumber, ticketReferenceForId } from '@/utils/ticketNumbers'

// null: the ticket couldn't be read (deleted, or out of reach).
const fetched = reactive(new Map<number, { number: number; title: string | null } | null>())
const inFlight = new Set<number>()

function lookUp(id: number) {
  if (inFlight.has(id)) return
  inFlight.add(id)
  void ticketReferenceForId(id).then((reference) => {
    fetched.set(id, reference ?? null)
    inFlight.delete(id)
  })
}

/**
 * The number of the ticket with this id, or `undefined` while it's being
 * looked up (or when it can't be). Reactive: a template or `computed` reading
 * it updates when the lookup lands.
 */
export function numberForTicketId(id: number): number | undefined {
  const known = knownTicketNumber({ id })
  if (known !== undefined) return known
  const remembered = fetched.get(id)
  if (remembered !== undefined) return remembered?.number
  lookUp(id)
  return undefined
}

/**
 * The title of the ticket with this id, or `undefined` while it's being
 * looked up (or when it can't be). Reactive, as {@link numberForTicketId}.
 */
export function titleForTicketId(id: number): string | undefined {
  const pooled = pool.get<{ title?: string | null }>('ticket', id)
  if (pooled) return pooled.title || undefined
  const remembered = fetched.get(id)
  if (remembered !== undefined) return remembered?.title || undefined
  lookUp(id)
  return undefined
}

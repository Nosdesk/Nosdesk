/**
 * Paths to a ticket from what the client holds: its number when known, else
 * the pooled ticket's, else by id. Built from {@link ticketRoute} and
 * {@link ticketRouteById}.
 *
 * Every row from a current server carries `number`. The mobile app can reach
 * an older server, which has none: there a ticket is quoted, and routed, by its
 * id, which is why the helpers fall back to it.
 */
import * as pool from '../sync/pool'
import { serverHasTicketNumbers } from '../services/instanceConfig'
import { ticketRoute, ticketRouteById } from './ticketRoutes'

export interface NumberedTicket {
  id: number
  number?: number | null
}

/** The ticket as people quote it, without the `#`. */
export function ticketNumber(ticket: NumberedTicket): number {
  return ticket.number ?? ticket.id
}

/** The number of the pooled ticket with this id, if the pool holds it. */
export function pooledTicketNumber(id: number): number | undefined {
  const row = pool.get<NumberedTicket>('ticket', id)
  return row ? ticketNumber(row) : undefined
}

/**
 * The ticket's route, `/tickets/<number>`. A row without its number (one
 * cached before tickets had numbers) goes by {@link ticketPathForId}.
 */
export function ticketPath(ticket: NumberedTicket): string {
  return ticket.number != null ? ticketRoute(ticket.number) : ticketPathForId(ticket.id)
}

/**
 * The route to a ticket known only by id. Straight to `/tickets/<number>` when
 * the pool holds the ticket; otherwise `/tickets/id/<id>`, which looks the
 * number up and redirects. On an older server the id is the number.
 */
export function ticketPathForId(id: number): string {
  if (!serverHasTicketNumbers()) return ticketRoute(id)
  const number = pooledTicketNumber(id)
  return number !== undefined ? ticketRoute(number) : ticketRouteById(id)
}

/**
 * A ticket's number is what people read and what URLs carry; its id is what
 * the API and stored references use. These turn one into the other.
 *
 * Every row from a current server carries `number`. The mobile app can reach
 * an older server, which has none: there a ticket is quoted, and routed, by
 * its id, which is why each helper falls back to it.
 */
import * as pool from '@nosdesk/core/sync/pool'
import { getWorkspaceRouting, serverHasTicketNumbers } from '@nosdesk/core/services/instanceConfig'
import ticketService from '@nosdesk/core/services/ticketService'
import { activeWorkspaceSlug } from '@/services/activeWorkspace'

interface NumberedTicket {
  id: number
  number?: number | null
}

/** The ticket as people quote it, without the `#`. */
export function ticketNumber(ticket: NumberedTicket): number {
  return ticket.number ?? ticket.id
}

/**
 * The ticket's route, `/tickets/<number>`. A row without its number (one
 * cached before tickets had numbers) goes by {@link ticketPathForId}.
 */
export function ticketPath(ticket: NumberedTicket): string {
  return ticket.number != null ? `/tickets/${ticket.number}` : ticketPathForId(ticket.id)
}

/**
 * The number to show for a row that may lack one (a list cached before
 * tickets had numbers): its own, else the pooled ticket's, else on an older
 * server its id. `undefined` until one is known, rather than a wrong number.
 */
export function knownTicketNumber(ticket: NumberedTicket): number | undefined {
  if (ticket.number != null) return ticket.number
  if (!serverHasTicketNumbers()) return ticket.id
  return pooledTicketNumber(ticket.id)
}

/** The number of the pooled ticket with this id, if the pool holds it. */
export function pooledTicketNumber(id: number): number | undefined {
  const row = pool.get<NumberedTicket>('ticket', id)
  return row ? ticketNumber(row) : undefined
}

/**
 * The route to a ticket known only by id. Straight to `/tickets/<number>` when
 * the pool holds the ticket; otherwise `/tickets/id/<id>`, which looks the
 * number up and redirects.
 */
export function ticketPathForId(id: number): string {
  if (!serverHasTicketNumbers()) return `/tickets/${id}`
  const number = pooledTicketNumber(id)
  return number !== undefined ? `/tickets/${number}` : `/tickets/id/${id}`
}

/**
 * The number of the ticket with this id: from the pool when it holds the
 * ticket, otherwise from the server. `undefined` when it can't be read.
 */
export async function ticketNumberForId(id: number): Promise<number | undefined> {
  if (!serverHasTicketNumbers()) return id
  const pooled = pooledTicketNumber(id)
  if (pooled !== undefined) return pooled
  try {
    return ticketNumber(await ticketService.getTicketById(id))
  } catch {
    return undefined
  }
}

/**
 * The id of the pooled ticket with this number (a typed `#N`, a URL's
 * number), or `undefined` when the pool doesn't hold it. On an older server
 * the number is the id.
 */
export function pooledTicketIdForNumber(number: number): number | undefined {
  if (!serverHasTicketNumbers()) return pool.has('ticket', number) ? number : undefined
  return pool.ticketIdForNumber(number)
}

/**
 * The id of the ticket a pasted or dropped URL names, or null. Only a ticket
 * URL for this workspace counts: on another origin, or under another
 * workspace's slug, the same number is a different ticket.
 */
export function ticketIdFromUrl(text: string): number | null {
  let url: URL
  try {
    url = new URL(text.trim(), window.location.origin)
  } catch {
    return null
  }
  if (url.origin !== window.location.origin) return null
  const segments = url.pathname.split('/').filter(Boolean)
  if (getWorkspaceRouting() === 'path') {
    if (segments[0] !== activeWorkspaceSlug()) return null
    segments.shift()
  }
  if (segments[0] !== 'tickets') return null
  if (segments.length === 3 && segments[1] === 'id' && /^\d+$/.test(segments[2])) {
    return Number(segments[2])
  }
  if (segments.length === 2 && /^\d+$/.test(segments[1])) {
    return pooledTicketIdForNumber(Number(segments[1])) ?? null
  }
  return null
}

/**
 * The id of the ticket with this number: from the pool, else from the server.
 * `undefined` when this workspace has no such ticket the caller can read.
 */
export async function ticketIdForNumber(number: number): Promise<number | undefined> {
  const pooled = pooledTicketIdForNumber(number)
  if (pooled !== undefined) return pooled
  if (!serverHasTicketNumbers()) return number
  try {
    return (await ticketService.getTicketByNumber(number)).id
  } catch {
    return undefined
  }
}

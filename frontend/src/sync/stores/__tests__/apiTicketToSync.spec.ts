import { afterEach, describe, expect, it } from 'vitest'
import * as pool from '@nosdesk/core/sync/pool'
import type { Ticket } from '@nosdesk/core/types/ticket'
import { apiTicketToSync, type SyncTicket } from '@/sync/stores/tickets'
import { toCardData } from '@/sync/views/cardData'

const backlog = { id: 3, name: 'Backlog', category: 'backlog', color: '#888888' } as const

function restTicket(extra: Partial<Ticket> = {}): Ticket {
  return {
    id: 253,
    number: 248,
    title: 'New Ticket',
    priority: 'medium',
    workflow_state_id: backlog.id,
    created: '2026-11-02T06:16:02Z',
    modified: '2026-11-02T06:16:02Z',
    ...extra,
  } as Ticket
}

/** Puts a REST ticket in the pool the way "New ticket" and the ticket page do. */
function putInPool(ticket: Ticket): SyncTicket {
  pool.upsert('ticket', ticket.id, apiTicketToSync(ticket))
  return pool.get<SyncTicket>('ticket', ticket.id)! as SyncTicket
}

afterEach(() => pool.reset())

describe('apiTicketToSync', () => {
  it('lists a ticket whose response nests its state', () => {
    const row = putInPool(restTicket({ workflow_state: { ...backlog } as Ticket['workflow_state'] }))
    expect(toCardData(row)?.workflow_state).toEqual(backlog)
  })

  // Pressing "New ticket" again returns the ticket already open; from a
  // server that doesn't nest the state, that response used to blank the
  // row's state and drop the ticket from every list.
  it("keeps the pool row's state when the response has none", () => {
    pool.upsert('ticket', 253, { ...apiTicketToSync(restTicket()), workflow_state: { ...backlog } })
    const row = putInPool(restTicket())
    expect(toCardData(row)).not.toBeNull()
    expect(row.workflow_state).toEqual(backlog)
  })

  it("doesn't keep a state the ticket has left", () => {
    pool.upsert('ticket', 253, { ...apiTicketToSync(restTicket()), workflow_state: { ...backlog } })
    const row = putInPool(restTicket({ workflow_state_id: 4 }))
    expect(row.workflow_state).toBeNull()
  })
})

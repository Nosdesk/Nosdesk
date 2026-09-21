/**
 * A workspace-wide snapshot is authoritative for the aggregates it
 * streams in full: rows it did not carry are stale and go. A partial
 * bootstrap (a project or ticket group) prunes nothing, and per-ticket
 * aggregates are never pruned.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import * as pool from '@nosdesk/core/sync/pool'

vi.mock('@nosdesk/core/services/ticketService', () => ({ default: {} }))
vi.mock('@/sync/stores/tickets', () => ({ apiTicketToSync: (t: unknown) => t }))

const { reconcileWithSnapshot } = await import('./lifecycle')

function seeded() {
  pool.reset()
  pool.upsert('ticket', 1, { id: 1, title: 'kept' })
  pool.upsert('ticket', 2, { id: 2, title: 'kept too' })
  pool.upsert('ticket', 3, { id: 3, title: 'deleted while the stream was down' })
  pool.upsert('comment', 9, { id: 9, ticket_id: 1 })
  pool.upsert('documentation_page', 5, { id: 5, title: 'leaked from another workspace' })
}

const seen = () =>
  new Map<'ticket' | 'documentation_page', Set<string>>([
    ['ticket', new Set(['1', '2'])],
    ['documentation_page', new Set()],
  ])

describe('reconcileWithSnapshot', () => {
  beforeEach(seeded)

  it('prunes rows the workspace snapshot did not carry', async () => {
    await reconcileWithSnapshot(['workspace:1', 'ticket:1'], seen())
    expect(pool.has('ticket', 1)).toBe(true)
    expect(pool.has('ticket', 2)).toBe(true)
    expect(pool.has('ticket', 3)).toBe(false)
    expect(pool.has('documentation_page', 5)).toBe(false)
    // Comments stream per ticket group, never workspace-wide: untouched.
    expect(pool.has('comment', 9)).toBe(true)
  })

  it('prunes nothing after a partial bootstrap', async () => {
    await reconcileWithSnapshot(['project:7'], seen())
    expect(pool.has('ticket', 3)).toBe(true)
    expect(pool.has('documentation_page', 5)).toBe(true)
  })
})

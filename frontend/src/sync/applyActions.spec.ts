/**
 * A live update for a row the pool never received must not mint a
 * partial row: the create frame was missed, so the whole row is fetched.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import * as pool from '@nosdesk/core/sync/pool'

const getTicketById = vi.fn()
vi.mock('@nosdesk/core/services/ticketService', () => ({
  default: { getTicketById: (id: number) => getTicketById(id) },
  getTicketById: (id: number) => getTicketById(id),
}))
vi.mock('@/sync/stores/tickets', () => ({
  apiTicketToSync: (t: { id: number; created: string; modified: string; title: string }) => ({
    id: t.id,
    title: t.title,
    created_at: t.created,
    updated_at: t.modified,
    last_activity_at: t.modified,
  }),
}))

const { applySseFrame } = await import('./lifecycle')

const update = (id: number, data: Record<string, unknown>) => ({
  sync_id: 1,
  aggregate: 'ticket' as const,
  aggregate_id: String(id),
  op: 'U' as const,
  event_type: 'ticket.assignee_changed',
  schema_version: 1,
  data,
  actor_uuid: null,
  actor_kind: 'user',
  actor_ref: null,
  correlation_id: null,
  causation_id: null,
  occurred_at: new Date().toISOString(),
  groups: ['workspace:1'],
})

describe('applyActions on a missed create', () => {
  beforeEach(() => {
    pool.reset()
    getTicketById.mockReset()
  })

  it('fetches the whole row instead of merging into nothing', async () => {
    getTicketById.mockResolvedValue({
      id: 36,
      title: 'Full',
      created: '2026-09-14T09:19:54Z',
      modified: '2026-09-14T09:20:39Z',
    })
    applySseFrame([update(36, { id: 36, assignee_uuid: 'a' })], 1, 1)
    expect(pool.has('ticket', 36)).toBe(false)
    await vi.waitFor(() => expect(pool.has('ticket', 36)).toBe(true))
    expect(getTicketById).toHaveBeenCalledWith(36)
    const row = pool.get<{ created_at: string; updated_at: string }>('ticket', 36)!
    expect(row.created_at).toBe('2026-09-14T09:19:54Z')
    expect(row.updated_at).toBe('2026-09-14T09:20:39Z')
  })

  it('merges as before when the row is present', () => {
    pool.upsert('ticket', 36, { id: 36, title: 'T', created_at: 'c', updated_at: 'u' })
    applySseFrame([update(36, { id: 36, assignee_uuid: 'a' })], 1, 1)
    expect(getTicketById).not.toHaveBeenCalled()
    const row = pool.get<{ assignee_uuid: string; created_at: string }>('ticket', 36)!
    expect(row.assignee_uuid).toBe('a')
    expect(row.created_at).toBe('c')
  })
})

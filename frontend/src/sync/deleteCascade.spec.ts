/**
 * A delete takes the rows the server's foreign keys cascade to, which arrive
 * with no event of their own: a project's links and cycles, a cycle's ticket
 * links, a ticket's comments (and their attachments), asset links, ticket
 * links in both directions, and project and cycle links. Deleting the row
 * behind a group also takes that group off every row it owns and drops its
 * snapshot watermark. The cache loses every row the pool does.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import * as pool from '@nosdesk/core/sync/pool'
import type { SyncAction, SyncAggregate } from '@nosdesk/core/sync/types'

const fake = vi.hoisted(() => ({
  deleted: [] as Array<readonly [string, string]>,
  watermarks: {} as Record<string, number>,
}))

vi.mock('./idb', () => ({
  putModels: async () => {},
  deleteModel: async (_h: unknown, aggregate: string, id: string) => {
    fake.deleted.push([aggregate, id])
  },
  deleteModels: async (_h: unknown, keys: Array<readonly [string, string]>) => {
    fake.deleted.push(...keys)
  },
  getGroupWatermarks: async () => ({ ...fake.watermarks }),
  replaceGroupWatermarks: async (_h: unknown, marks: Record<string, number>) => {
    fake.watermarks = { ...marks }
  },
  setLastSyncId: async () => {},
  setLastXid8: async () => {},
}))
vi.mock('@nosdesk/core/services/ticketService', () => ({
  default: { getTicketById: vi.fn() },
  getTicketById: vi.fn(),
}))

const { applyActions } = await import('./apply')
const { state } = await import('./runtimeState')

const action = (
  aggregate: SyncAggregate,
  aggregateId: string | number,
  data: Record<string, unknown>,
  op: SyncAction['op'] = 'D',
): SyncAction => ({
  sync_id: 1,
  aggregate,
  aggregate_id: String(aggregateId),
  op,
  event_type: `${aggregate}.deleted`,
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

const deleteTicket = (id: number) => applyActions([action('ticket', id, { id, number: id })])
const deleteProject = (id: number) => applyActions([action('project', id, { id })])

const owned = ['workspace:1']

/** Ticket 5 and ticket 6, each with its children. */
function seedTickets(): void {
  for (const id of [5, 6, 7, 8]) pool.upsert('ticket', id, { id, number: id }, owned)
  pool.upsert('comment', 50, { id: 50, ticket_id: 5 }, owned)
  pool.upsert('comment', 51, { id: 51, ticket_id: 5 }, owned)
  pool.upsert('comment', 60, { id: 60, ticket_id: 6 }, owned)
  pool.upsert('attachment', 500, { id: 500, comment_id: 50 }, owned)
  pool.upsert('attachment', 510, { id: 510, comment_id: 51 }, owned)
  pool.upsert('attachment', 600, { id: 600, comment_id: 60 }, owned)
  pool.upsert('attachment', 900, { id: 900, comment_id: null }, owned)
  pool.upsert('ticket_asset', '5:3', { ticket_id: 5, asset_id: 3 }, owned)
  pool.upsert('ticket_asset', '6:3', { ticket_id: 6, asset_id: 3 }, owned)
  pool.upsert('linked_ticket', '5:7', { ticket_id: 5, linked_ticket_id: 7 }, owned)
  pool.upsert('linked_ticket', '7:5', { ticket_id: 7, linked_ticket_id: 5 }, owned)
  pool.upsert('linked_ticket', '7:8', { ticket_id: 7, linked_ticket_id: 8 }, owned)
  pool.upsert('project_ticket', '1:5', { project_id: 1, ticket_id: 5 }, owned)
  pool.upsert('project_ticket', '1:6', { project_id: 1, ticket_id: 6 }, owned)
  pool.upsert('cycle_ticket', '10:5', { cycle_id: 10, ticket_id: 5 }, owned)
  pool.upsert('cycle_ticket', '10:6', { cycle_id: 10, ticket_id: 6 }, owned)
}

/** Project 1 with cycle 10, project 2 with cycle 20, and their links. */
function seedProjects(): void {
  pool.upsert('project', 1, { id: 1 }, owned)
  pool.upsert('project', 2, { id: 2 }, owned)
  pool.upsert('cycle', 10, { id: 10, project_id: 1 }, owned)
  pool.upsert('cycle', 20, { id: 20, project_id: 2 }, owned)
  pool.upsert('project_ticket', '2:6', { project_id: 2, ticket_id: 6 }, owned)
  pool.upsert('cycle_ticket', '20:7', { cycle_id: 20, ticket_id: 7 }, owned)
}

const deletedKeys = () => fake.deleted.map(([aggregate, id]) => `${aggregate}:${id}`).sort()

describe('applyActions on a delete', () => {
  beforeEach(() => {
    pool.reset()
    fake.deleted = []
    fake.watermarks = {}
    state.handle = { db: {} as IDBDatabase, name: 'test' }
  })

  describe('of a ticket', () => {
    beforeEach(seedTickets)

    it('removes its comments and their attachments', () => {
      deleteTicket(5)
      expect(pool.has('ticket', 5)).toBe(false)
      expect(pool.has('comment', 50)).toBe(false)
      expect(pool.has('comment', 51)).toBe(false)
      expect(pool.has('attachment', 500)).toBe(false)
      expect(pool.has('attachment', 510)).toBe(false)
      // Another ticket's, and a draft upload on no comment.
      expect(pool.has('comment', 60)).toBe(true)
      expect(pool.has('attachment', 600)).toBe(true)
      expect(pool.has('attachment', 900)).toBe(true)
    })

    it('removes its asset, project and cycle links', () => {
      deleteTicket(5)
      expect(pool.has('ticket_asset', '5:3')).toBe(false)
      expect(pool.has('project_ticket', '1:5')).toBe(false)
      expect(pool.has('cycle_ticket', '10:5')).toBe(false)
      expect(pool.has('ticket_asset', '6:3')).toBe(true)
      expect(pool.has('project_ticket', '1:6')).toBe(true)
      expect(pool.has('cycle_ticket', '10:6')).toBe(true)
    })

    it('removes its ticket links in both directions', () => {
      deleteTicket(5)
      expect(pool.has('linked_ticket', '5:7')).toBe(false)
      expect(pool.has('linked_ticket', '7:5')).toBe(false)
      expect(pool.has('linked_ticket', '7:8')).toBe(true)
      expect(pool.has('ticket', 7)).toBe(true)
    })

    it('deletes the same rows from the cache', () => {
      deleteTicket(5)
      expect(deletedKeys()).toEqual(
        [
          'ticket:5',
          'comment:50',
          'comment:51',
          'attachment:500',
          'attachment:510',
          'ticket_asset:5:3',
          'linked_ticket:5:7',
          'linked_ticket:7:5',
          'project_ticket:1:5',
          'cycle_ticket:10:5',
        ].sort(),
      )
    })

    it('keeps the row for the observers that run after it', () => {
      const del = action('ticket', 5, { id: 5, number: 5 })
      applyActions([del])
      expect(del.prior).toEqual({ id: 5, number: 5 })
    })

    it('takes its group off the rows it owns and removes those it alone owned', () => {
      pool.upsert('project', 1, { id: 1 }, ['ticket:5'])
      pool.upsert('project', 2, { id: 2 }, ['workspace:1', 'ticket:5'])
      pool.upsert('user', 'u', { uuid: 'u' }, ['workspace:1', 'ticket:5', 'ticket:6'])
      deleteTicket(5)
      expect(pool.has('project', 1)).toBe(false)
      expect([...pool.ownersOf('project', 2)]).toEqual(['workspace:1'])
      expect([...pool.ownersOf('user', 'u')].sort()).toEqual(['ticket:6', 'workspace:1'])
      expect(deletedKeys()).toContain('project:1')
      expect(deletedKeys()).not.toContain('project:2')
    })

    it('leaves a local row it does not own', () => {
      pool.upsert('project', 3, { id: 3 })
      deleteTicket(5)
      expect([...pool.ownersOf('project', 3)]).toEqual([pool.LOCAL_OWNER])
    })

    it("drops its group's watermark", async () => {
      fake.watermarks = { 'workspace:1': 9, 'ticket:5': 8, 'ticket:6': 7 }
      deleteTicket(5)
      await vi.waitFor(() => expect(fake.watermarks).toEqual({ 'workspace:1': 9, 'ticket:6': 7 }))
    })

    it('drops the watermarks of several deletes in one frame', async () => {
      fake.watermarks = { 'workspace:1': 9, 'ticket:5': 8, 'ticket:6': 7 }
      applyActions([action('ticket', 5, { id: 5 }), action('ticket', 6, { id: 6 })])
      await vi.waitFor(() => expect(fake.watermarks).toEqual({ 'workspace:1': 9 }))
    })

    it('strips its group even when the pool never held the row', async () => {
      pool.upsert('comment', 70, { id: 70, ticket_id: 99 }, ['ticket:99'])
      fake.watermarks = { 'ticket:99': 4 }
      deleteTicket(99)
      expect(pool.has('comment', 70)).toBe(false)
      await vi.waitFor(() => expect(fake.watermarks).toEqual({}))
    })
  })

  describe('of a project', () => {
    beforeEach(() => {
      seedTickets()
      seedProjects()
    })

    it('removes its ticket links and cycles, and the cycles take their ticket links', () => {
      deleteProject(1)
      expect(pool.has('project', 1)).toBe(false)
      expect(pool.has('project_ticket', '1:5')).toBe(false)
      expect(pool.has('project_ticket', '1:6')).toBe(false)
      expect(pool.has('cycle', 10)).toBe(false)
      expect(pool.has('cycle_ticket', '10:5')).toBe(false)
      expect(pool.has('cycle_ticket', '10:6')).toBe(false)
      // The tickets themselves stay, as does everything of project 2.
      expect(pool.has('ticket', 5)).toBe(true)
      expect(pool.has('ticket', 6)).toBe(true)
      expect(pool.has('project', 2)).toBe(true)
      expect(pool.has('cycle', 20)).toBe(true)
      expect(pool.has('project_ticket', '2:6')).toBe(true)
      expect(pool.has('cycle_ticket', '20:7')).toBe(true)
      expect(deletedKeys()).toEqual(
        [
          'project:1',
          'project_ticket:1:5',
          'project_ticket:1:6',
          'cycle:10',
          'cycle_ticket:10:5',
          'cycle_ticket:10:6',
        ].sort(),
      )
    })

    it('strips its group and the groups of its cycles, and drops their watermarks', async () => {
      for (const id of [5, 6, 7]) pool.remove('ticket', id)
      pool.upsert('ticket', 5, { id: 5 }, ['project:1'])
      pool.upsert('ticket', 6, { id: 6 }, ['project:1', 'workspace:1'])
      pool.upsert('ticket', 7, { id: 7 }, ['cycle:10'])
      fake.watermarks = { 'workspace:1': 9, 'project:1': 8, 'cycle:10': 7, 'project:2': 6 }
      deleteProject(1)
      expect(pool.has('ticket', 5)).toBe(false)
      expect([...pool.ownersOf('ticket', 6)]).toEqual(['workspace:1'])
      expect(pool.has('ticket', 7)).toBe(false)
      await vi.waitFor(() => expect(fake.watermarks).toEqual({ 'workspace:1': 9, 'project:2': 6 }))
    })
  })

  describe('of a cycle', () => {
    beforeEach(() => {
      seedTickets()
      seedProjects()
    })

    it('removes its ticket links only', async () => {
      fake.watermarks = { 'cycle:10': 7 }
      applyActions([action('cycle', 10, { id: 10 })])
      expect(pool.has('cycle', 10)).toBe(false)
      expect(pool.has('cycle_ticket', '10:5')).toBe(false)
      expect(pool.has('cycle_ticket', '10:6')).toBe(false)
      expect(pool.has('cycle_ticket', '20:7')).toBe(true)
      expect(pool.has('project', 1)).toBe(true)
      expect(pool.has('ticket', 5)).toBe(true)
      await vi.waitFor(() => expect(fake.watermarks).toEqual({}))
    })

    it('cascades nothing when archived', () => {
      applyActions([action('cycle', 10, { id: 10, project_id: 1, archived_at: 'now' }, 'A')])
      expect(pool.has('cycle', 10)).toBe(true)
      expect(pool.has('cycle_ticket', '10:5')).toBe(true)
    })
  })

  it('removes a deleted comment with its attachments', () => {
    seedTickets()
    applyActions([action('comment', 50, { id: 50 })])
    expect(pool.has('comment', 50)).toBe(false)
    expect(pool.has('attachment', 500)).toBe(false)
    expect(pool.has('attachment', 510)).toBe(true)
    expect(pool.has('ticket', 5)).toBe(true)
    expect(deletedKeys()).toEqual(['attachment:500', 'comment:50'])
  })

  it('removes only the row for an aggregate with no cascade', () => {
    seedTickets()
    applyActions([action('project_ticket', '1:5', { project_id: 1, ticket_id: 5 })])
    expect(pool.has('project_ticket', '1:5')).toBe(false)
    expect(pool.has('ticket', 5)).toBe(true)
    expect(pool.has('project_ticket', '1:6')).toBe(true)
    expect(deletedKeys()).toEqual(['project_ticket:1:5'])
  })
})

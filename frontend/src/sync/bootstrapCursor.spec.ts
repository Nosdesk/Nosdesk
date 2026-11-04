/**
 * Subscribing a new group (opening a ticket) bootstraps that group alone.
 * Its snapshot ends at the server's horizon, which can be past events for
 * the groups already subscribed that no delta or SSE frame has delivered
 * yet. The next delta must still deliver them.
 */
import { ref } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import * as pool from '@nosdesk/core/sync/pool'

const fake = vi.hoisted(() => {
  const dbs = new Map<string, Map<string, unknown>>()
  const meta = (handle: { name: string }) => {
    let m = dbs.get(handle.name)
    if (!m) dbs.set(handle.name, (m = new Map()))
    return m
  }
  const get = (key: string) => async (h: { name: string }) => meta(h).get(key)
  const set = (key: string) => async (h: { name: string }, v: unknown) => {
    meta(h).set(key, v)
  }
  return {
    dbs,
    idb: {
      CACHE_GENERATION: 2,
      open: async (user: string, schema: string, slug: string | null) => ({
        db: { close() {} },
        name: `test:${user}:${schema}:${slug}`,
      }),
      wipe: async (name: string) => {
        dbs.delete(name)
      },
      putModels: async () => {},
      loadModels: async () => [],
      deleteModel: async () => {},
      putTransaction: async () => {},
      deleteTransaction: async () => {},
      loadTransactions: async () => [],
      setLastSyncId: set('last_sync_id'),
      getLastSyncId: get('last_sync_id'),
      setLastXid8: set('last_xid8'),
      getLastXid8: get('last_xid8'),
      setSchemaHash: set('schema_hash'),
      getSchemaHash: get('schema_hash'),
      setInstanceId: set('instance_id'),
      getInstanceId: get('instance_id'),
      setWorkspaceEpoch: set('workspace_epoch'),
      getWorkspaceEpoch: get('workspace_epoch'),
      setSubscribedGroups: set('subscribed_groups'),
      getSubscribedGroups: get('subscribed_groups'),
      getGroupWatermarks: get('group_watermarks'),
      setGroupWatermarks: async () => {},
      replaceGroupWatermarks: async () => {},
      setModelVersionsTag: set('model_versions_tag'),
      getModelVersionsTag: get('model_versions_tag'),
    },
  }
})

/** The server: a feed of committed events, and the horizon a snapshot ends at. */
const server = vi.hoisted(() => ({
  horizon: 10,
  events: [] as Array<{ xid8: number; sync_id: number; group: string; id: number; title: string }>,
  deltaFrom: [] as number[],
}))

vi.mock('./idb', () => fake.idb)
vi.mock('./queue', () => ({ setIdbHandle: () => {}, flush: async () => {} }))
vi.mock('@/utils/collabLocalCache', () => ({ purgeAllCollabDocs: async () => {} }))
vi.mock('@nosdesk/core/services/session', () => ({ sessionGone: () => false }))
vi.mock('@/services/activeWorkspace', () => ({
  workspaceReady: () => true,
  workspaceReadyRef: ref(true),
}))
vi.mock('@nosdesk/core/services/ticketService', () => ({ default: {} }))
vi.mock('@/sync/stores/tickets', () => ({ apiTicketToSync: (t: unknown) => t }))
vi.mock('@/services/authFetch', () => ({
  authFetch: vi.fn(async (url: string) => {
    const params = new URL(url, 'http://x').searchParams
    if (url.startsWith('/sync/bootstrap')) {
      const groups = (params.get('groups') ?? '').split(',')
      const end = { last_xid8: server.horizon, last_sync_id: server.horizon * 10 }
      const lines = [
        { __meta__: { server_schema: 'h', ...end, groups_granted: groups } },
        { __end__: end },
      ]
      return new Response(lines.map((l) => JSON.stringify(l)).join('\n') + '\n')
    }
    if (url.startsWith('/sync/delta')) {
      const from = Number(params.get('from_xid8') ?? 0)
      const groups = new Set((params.get('groups') ?? '').split(','))
      server.deltaFrom.push(from)
      const actions = server.events
        .filter((e) => e.xid8 > from && groups.has(e.group))
        .map((e) => ({
          sync_id: e.sync_id,
          aggregate: 'ticket',
          aggregate_id: String(e.id),
          op: 'I',
          event_type: 'ticket.created',
          schema_version: 1,
          data: { id: e.id, title: e.title },
          groups: [e.group],
          occurred_at: '2026-11-02T06:16:02Z',
        }))
      return new Response(
        JSON.stringify({
          actions,
          last_xid8: server.horizon,
          last_sync_id: server.horizon * 10,
          has_more: false,
        }),
      )
    }
    return new Response(null, { status: 404 })
  }),
}))

const { hydrate, subscribe, pullDelta, tearDown } = await import('./lifecycle')

const WORKSPACE = 'workspace:6'

beforeEach(() => {
  fake.dbs.clear()
  fake.dbs.set('test:u1:h:acme', new Map<string, unknown>([['schema_hash', 'h']]))
  server.horizon = 10
  server.events = []
  server.deltaFrom = []
})

afterEach(async () => {
  await tearDown()
})

describe('bootstrapping a newly subscribed group', () => {
  it("doesn't skip events for groups already subscribed", async () => {
    await hydrate('u1', 'h', '', 'acme', '')
    await subscribe(WORKSPACE)
    expect(pool.getLastXid8()).toBe(10)

    // Someone creates ticket 253 (xid8 15). No delta or SSE frame has
    // delivered it when the app opens ticket 99, whose group bootstraps
    // with a snapshot ending at the horizon, 20.
    server.events.push({ xid8: 15, sync_id: 150, group: WORKSPACE, id: 253, title: 'New Ticket' })
    server.horizon = 20
    const before = server.deltaFrom.length
    await subscribe('ticket:99')
    await pullDelta()

    const since = server.deltaFrom.slice(before)
    expect(Math.min(...since), 'a delta asks from before the event').toBeLessThan(15)
    expect(pool.has('ticket', 253), 'the event reached the pool').toBe(true)
    expect(pool.getLastXid8()).toBe(20)
  })

  it('delivers the event by delta when no other group is bootstrapped', async () => {
    await hydrate('u1', 'h', '', 'acme', '')
    await subscribe(WORKSPACE)
    server.events.push({ xid8: 15, sync_id: 150, group: WORKSPACE, id: 253, title: 'New Ticket' })
    server.horizon = 20

    await pullDelta()
    expect(pool.has('ticket', 253)).toBe(true)
  })
})

/**
 * A retention resync: the server says the cursor is older than the feed it
 * keeps. The cache stays, so the lists paint from it while every subscribed
 * group snapshots again; the watermarks go, the snapshot's strip removes what
 * was deleted meanwhile, and the snapshot sets the new cursor. Nothing is
 * asked for from the expired cursor: live events during the resync apply
 * their rows and are replayed over the snapshot from the new cursor. A new
 * workspace epoch still wipes.
 */
import { ref } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import * as pool from '@nosdesk/core/sync/pool'
import { MODEL_VERSIONS_TAG } from './schemaVersions'

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
  type Row = { aggregate: string; id: string; schema_version?: number; data?: unknown; owners?: string[] }
  /** The cache as the next hydrate reads it, by `aggregate:id`. */
  const rows = new Map<string, Row>()
  const wipes: string[] = []
  const drop = (keys: Array<[string, string]>) => {
    for (const [a, id] of keys) rows.delete(`${a}:${id}`)
  }
  return {
    dbs,
    rows,
    wipes,
    idb: {
      CACHE_GENERATION: 2,
      open: async (user: string, schema: string, slug: string | null) => ({
        db: { close() {} },
        name: `test:${user}:${schema}:${slug}`,
      }),
      wipe: async (name: string) => {
        wipes.push(name)
        dbs.delete(name)
        rows.clear()
      },
      putModels: async (_h: unknown, put: Row[]) => {
        for (const r of put) rows.set(`${r.aggregate}:${r.id}`, r)
      },
      loadModels: async () => [...rows.values()],
      deleteModel: async (_h: unknown, aggregate: string, id: string) => drop([[aggregate, id]]),
      deleteModels: async (_h: unknown, keys: Array<[string, string]>) => drop(keys),
      deleteModelsOwnedOnlyBy: async () => {},
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
      setGroupWatermarks: async (h: { name: string }, groups: string[], mark: unknown) => {
        const current = { ...((meta(h).get('group_watermarks') as Record<string, unknown>) ?? {}) }
        for (const g of groups) current[g] = mark
        meta(h).set('group_watermarks', current)
      },
      replaceGroupWatermarks: set('group_watermarks'),
      setModelVersionsTag: set('model_versions_tag'),
      getModelVersionsTag: get('model_versions_tag'),
      setSnapshotMaxAge: set('snapshot_max_age_secs'),
      getSnapshotMaxAge: get('snapshot_max_age_secs'),
    },
  }
})

/** A point in a snapshot stream where it waits for `server.held()`. */
const HOLD = { __hold__: true } as const

/**
 * The server: each group's snapshot rows (ending at xid8 30), the live events
 * its feed holds, and every request it served. A delta from a cursor below
 * `floor` is past retention.
 */
const server = vi.hoisted(() => ({
  rows: {} as Record<string, Array<Record<string, unknown>>>,
  actions: [] as Array<Record<string, unknown> & { xid8: number }>,
  floor: 0,
  epoch: '' as string,
  failBootstraps: 0,
  requests: [] as string[],
  held: null as null | (() => void),
}))

vi.mock('./idb', () => fake.idb)
vi.mock('./queue', () => ({ setIdbHandle: () => {}, flush: async () => {} }))
vi.mock('@/utils/collabLocalCache', () => ({ purgeAllCollabDocs: vi.fn(async () => {}) }))
vi.mock('@nosdesk/core/services/session', () => ({ sessionGone: () => false }))
vi.mock('@/services/activeWorkspace', () => ({
  workspaceReady: () => true,
  workspaceReadyRef: ref(true),
}))
vi.mock('@nosdesk/core/services/ticketService', () => ({ default: {} }))
vi.mock('@/sync/stores/tickets', () => ({ apiTicketToSync: (t: unknown) => t }))
vi.mock('@/services/authFetch', () => ({
  authFetch: vi.fn(async (url: string) => {
    server.requests.push(url)
    const params = new URL(url, 'http://x').searchParams
    const groups = (params.get('groups') ?? '').split(',')
    const epoch = server.epoch === '' ? {} : { workspace_epoch: server.epoch }
    if (url.startsWith('/sync/bootstrap')) {
      if (server.failBootstraps > 0) {
        server.failBootstraps--
        return new Response(null, { status: 503 })
      }
      const end = { last_xid8: 30, last_sync_id: 300 }
      const lines = [
        { __meta__: { server_schema: 'h', ...end, groups_granted: groups } },
        ...groups.flatMap((g) => server.rows[g] ?? []),
        { __end__: end },
      ]
      const encoder = new TextEncoder()
      const body = new ReadableStream<Uint8Array>({
        async start(controller) {
          for (const line of lines) {
            if ('__hold__' in line) {
              await new Promise<void>((resolve) => (server.held = resolve))
              continue
            }
            controller.enqueue(encoder.encode(JSON.stringify(line) + '\n'))
          }
          controller.close()
        },
      })
      return new Response(body)
    }
    if (url.startsWith('/sync/delta')) {
      const from = Number(params.get('from_xid8') ?? 0)
      if (from < server.floor) {
        return new Response(
          JSON.stringify({
            actions: [],
            last_xid8: from,
            last_sync_id: from * 10,
            has_more: false,
            resync_required: true,
            ...epoch,
          }),
        )
      }
      const actions = server.actions.filter((a) => a.xid8 > from)
      const last = Math.max(from, ...actions.map((a) => a.xid8))
      return new Response(
        JSON.stringify({ actions, last_xid8: last, last_sync_id: last * 10, has_more: false, ...epoch }),
      )
    }
    return new Response(null, { status: 404 })
  }),
}))

const { hydrate, subscribe, tearDown, pullDelta, applySseFrame } = await import('./lifecycle')
const { state } = await import('./runtimeState')

const WORKSPACE = 'workspace:6'
const DB = 'test:u1:h:acme'
const NOW = new Date('2026-11-24T09:00:00Z').getTime()

const meta = () => fake.dbs.get(DB)!
const bootstraps = () => server.requests.filter((u) => u.startsWith('/sync/bootstrap'))
const deltaCursors = () =>
  server.requests
    .filter((u) => u.startsWith('/sync/delta'))
    .map((u) => Number(new URL(u, 'http://x').searchParams.get('from_xid8')))

function cache(aggregate: string, id: number, data: Record<string, unknown>, owners?: string[]): void {
  fake.rows.set(`${aggregate}:${id}`, {
    aggregate,
    id: String(id),
    schema_version: 1,
    data: { id, ...data },
    owners,
  })
}

/** Last session: tickets 1 and 5 still on the server, ticket 2 deleted while
 * away, a cycle; the workspace watermarked fresh, the cursor at 20. */
async function warmLaunch(): Promise<void> {
  const m = meta()
  m.set('last_xid8', 20)
  m.set('last_sync_id', 200)
  m.set('model_versions_tag', MODEL_VERSIONS_TAG)
  m.set('group_watermarks', { [WORKSPACE]: { xid8: 20, at: NOW - 60_000 } })
  cache('ticket', 1, { title: 'Ticket 1', status: 'open' }, [WORKSPACE])
  cache('ticket', 2, { title: 'Deleted while away' }, [WORKSPACE])
  cache('ticket', 5, { title: 'Ticket 5' }, [WORKSPACE])
  cache('cycle', 3, { name: 'Sprint', project_id: 7 }, [WORKSPACE])
  await hydrate('u1', 'h', '', 'acme', '')
  await subscribe(WORKSPACE)
  expect(bootstraps()).toEqual([])
  server.requests = []
}

/** Resolves once a stream reaches a HOLD; the result lets it go on. */
async function reachHold(): Promise<() => void> {
  await vi.waitFor(() => {
    if (!server.held) throw new Error('stream not held yet')
  })
  const release = server.held!
  server.held = null
  return release
}

function ticketEvent(xid8: number, op: 'U' | 'D', data: Record<string, unknown>) {
  return {
    xid8,
    sync_id: xid8 * 10,
    aggregate: 'ticket',
    aggregate_id: String(data.id),
    op,
    event_type: op === 'D' ? 'ticket.deleted' : 'ticket.updated',
    schema_version: 1,
    data,
    groups: [WORKSPACE],
  } as Record<string, unknown> & { xid8: number; sync_id: number }
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime(NOW)
  fake.dbs.clear()
  fake.dbs.set(DB, new Map<string, unknown>([['schema_hash', 'h']]))
  fake.rows.clear()
  fake.wipes.length = 0
  server.rows = {
    [WORKSPACE]: [
      { __model__: 'ticket', id: 1, title: 'Ticket 1', status: 'open' },
      { __model__: 'ticket', id: 5, title: 'Ticket 5' },
    ],
  }
  server.actions = []
  server.floor = 0
  server.epoch = ''
  server.failBootstraps = 0
  server.requests = []
  server.held = null
})

afterEach(async () => {
  await tearDown()
  vi.useRealTimers()
})

describe('a retention resync', () => {
  it('paints from the cache while the snapshot streams, then strips what was deleted', async () => {
    await warmLaunch()
    server.rows[WORKSPACE] = [HOLD, ...server.rows[WORKSPACE]!]
    server.floor = 25

    const resyncing = pullDelta()
    const release = await reachHold()
    // No wipe: the cached rows are still there to paint, the watermarks gone.
    expect(fake.wipes).toEqual([])
    expect(pool.has('ticket', 1)).toBe(true)
    expect(pool.has('ticket', 2)).toBe(true)
    expect(fake.rows.has('ticket:2')).toBe(true)
    expect(meta().get('group_watermarks')).toEqual({})
    // Rows no snapshot carries can't be reconciled; they go as a wipe took them.
    expect(pool.has('cycle', 3)).toBe(false)
    expect(fake.rows.has('cycle:3')).toBe(false)

    release()
    expect(await resyncing).toBe(false)
    expect(pool.has('ticket', 1)).toBe(true)
    expect(pool.has('ticket', 2)).toBe(false)
    expect(fake.rows.has('ticket:2')).toBe(false)
    expect(meta().get('group_watermarks')).toEqual({
      [WORKSPACE]: { xid8: 30, at: expect.any(Number) },
    })
  })

  it('takes its cursor from the snapshot and asks nothing of the expired one', async () => {
    await warmLaunch()
    server.floor = 25

    await pullDelta()
    expect(pool.getLastXid8()).toBe(30)
    expect(meta().get('last_xid8')).toBe(30)
    // The one delta from 20 said it was too old; the replay ran from 30.
    expect(deltaCursors()).toEqual([20, 30])
    expect(bootstraps()).toHaveLength(1)
  })

  it('replays the live events that land while it streams over its rows, from the new cursor', async () => {
    await warmLaunch()
    server.rows[WORKSPACE] = [
      { __model__: 'ticket', id: 1, title: 'Ticket 1', status: 'open' },
      HOLD,
      { __model__: 'ticket', id: 5, title: 'Ticket 5' },
    ]
    server.floor = 25
    const resyncing = pullDelta()
    const release = await reachHold()

    const close = ticketEvent(35, 'U', { id: 1, title: 'Ticket 1', status: 'closed' })
    const remove = ticketEvent(36, 'D', { id: 5 })
    server.actions = [close, remove]
    applySseFrame([close as never, remove as never], 36, 360)
    // The rows apply; the cursor waits for the snapshot.
    expect(pool.get('ticket', 1)).toMatchObject({ status: 'closed' })
    expect(pool.getLastXid8()).toBe(20)
    expect(await pullDelta()).toBe(false)

    release()
    await resyncing
    expect(pool.get('ticket', 1)).toMatchObject({ status: 'closed' })
    expect(pool.has('ticket', 5)).toBe(false)
    expect((fake.rows.get('ticket:1')?.data as Record<string, unknown>).status).toBe('closed')
    expect(fake.rows.has('ticket:5')).toBe(false)
    expect(pool.getLastXid8()).toBe(36)
    expect(deltaCursors()).toEqual([20, 30])
  })

  it('retries the snapshot, not a delta, when it fails', async () => {
    await warmLaunch()
    server.floor = 25
    server.failBootstraps = 1

    await pullDelta()
    expect(bootstraps()).toHaveLength(1)
    expect(deltaCursors()).toEqual([20])
    // The cache still paints.
    expect(pool.has('ticket', 1)).toBe(true)

    await pullDelta()
    expect(bootstraps()).toHaveLength(2)
    expect(deltaCursors()).toEqual([20, 30])
    expect(pool.getLastXid8()).toBe(30)
    expect(pool.has('ticket', 2)).toBe(false)
  })

  it('is a wipe all the same when the workspace epoch is new', async () => {
    await warmLaunch()
    server.floor = 25
    server.epoch = 'e2'

    await pullDelta()
    expect(fake.wipes).toEqual([DB])
    expect(pool.has('ticket', 2)).toBe(false)
    expect(pool.getLastXid8()).toBe(30)
    expect(meta().get('workspace_epoch')).toBe('e2')
  })
})

describe('sync ids told to observers ahead of a delta', () => {
  it('are kept to a bounded number, and forgotten when no group is subscribed', async () => {
    await hydrate('u1', 'h', '', 'acme', '')
    // Not caught up yet, so frames leave the cursor where it is.
    const frames = Array.from({ length: 6_000 }, (_, i) =>
      ticketEvent(i + 1, 'U', { id: 1000 + (i % 3), title: 'x' }),
    )
    applySseFrame(frames as never[], 6_000, 60_000)
    expect(state.notifiedAhead.size).toBe(5_000)
    expect(state.notifiedAhead.has(60_000)).toBe(true)
    expect(state.notifiedAhead.has(10)).toBe(false)

    expect(await pullDelta()).toBe(false)
    expect(state.notifiedAhead.size).toBe(0)
  })
})

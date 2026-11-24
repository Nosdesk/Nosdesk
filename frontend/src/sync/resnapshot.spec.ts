/**
 * The server flags a delta `resnapshot` when what the viewer may see changed
 * in the span it covers (a demotion, a membership). The client snapshots its
 * subscribed groups again with the strip, holding the cursor, one run at a
 * time; not while a resync, which snapshots every group, is under way.
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
      deleteModelsOwnedOnlyBy: async (
        _h: unknown,
        keys: Array<[string, string]>,
        groups: ReadonlySet<string>,
      ) => {
        drop(
          keys.filter(([a, id]) => {
            const owners = rows.get(`${a}:${id}`)?.owners
            return !!owners && owners.length > 0 && owners.every((o) => groups.has(o))
          }),
        )
      },
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
  /** The next delta carries `resnapshot: true` (an access change). */
  resnapshot: false,
  /** Groups bootstraps no longer grant. */
  denied: [] as string[],
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
        {
          __meta__: {
            server_schema: 'h',
            ...end,
            groups_granted: groups.filter((g) => !server.denied.includes(g)),
          },
        },
        ...groups.filter((g) => !server.denied.includes(g)).flatMap((g) => server.rows[g] ?? []),
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
      const resnapshot = server.resnapshot ? { resnapshot: true } : {}
      server.resnapshot = false
      return new Response(
        JSON.stringify({
          actions,
          last_xid8: last,
          last_sync_id: last * 10,
          has_more: false,
          ...epoch,
          ...resnapshot,
        }),
      )
    }
    return new Response(null, { status: 404 })
  }),
}))

const { hydrate, subscribe, tearDown, pullDelta, resnapshotSubscribedGroups } =
  await import('./lifecycle')

const WORKSPACE = 'workspace:6'
const TICKET = 'ticket:9'
const DB = 'test:u1:h:acme'
const NOW = new Date('2026-11-24T09:00:00Z').getTime()

const meta = () => fake.dbs.get(DB)!
const bootstraps = () =>
  server.requests
    .filter((u) => u.startsWith('/sync/bootstrap'))
    .map((u) => new URL(u, 'http://x').searchParams.get('groups')!.split(',').sort())

function cache(aggregate: string, id: number, data: Record<string, unknown>, owners: string[]): void {
  fake.rows.set(`${aggregate}:${id}`, {
    aggregate,
    id: String(id),
    schema_version: 1,
    data: { id, ...data },
    owners,
  })
}

/** Last session had the workspace and ticket 9 open, both fresh, the cursor
 * at 20. Ticket 2 and ticket 9's comment are what the viewer may lose. */
async function warmLaunch(): Promise<void> {
  const m = meta()
  m.set('last_xid8', 20)
  m.set('last_sync_id', 200)
  m.set('model_versions_tag', MODEL_VERSIONS_TAG)
  const fresh = { xid8: 20, at: NOW - 60_000 }
  m.set('group_watermarks', { [WORKSPACE]: fresh, [TICKET]: fresh })
  cache('ticket', 1, { title: 'Ticket 1' }, [WORKSPACE])
  cache('ticket', 2, { title: 'Ticket 2' }, [WORKSPACE])
  cache('ticket', 9, { title: 'Ticket 9' }, [WORKSPACE, TICKET])
  cache('comment', 40, { ticket_id: 9 }, [TICKET])
  await hydrate('u1', 'h', '', 'acme', '')
  await Promise.all([subscribe(WORKSPACE), subscribe(TICKET)])
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

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime(NOW)
  fake.dbs.clear()
  fake.dbs.set(DB, new Map<string, unknown>([['schema_hash', 'h']]))
  fake.rows.clear()
  fake.wipes.length = 0
  // The viewer was demoted: ticket 2 and ticket 9 are no longer theirs.
  server.rows = {
    [WORKSPACE]: [
      { __model__: 'ticket', id: 1, title: 'Ticket 1' },
      { __model__: 'ticket', id: 9, title: 'Ticket 9' },
    ],
    [TICKET]: [],
  }
  server.denied = [TICKET]
  server.actions = []
  server.floor = 0
  server.epoch = ''
  server.failBootstraps = 0
  server.requests = []
  server.held = null
  server.resnapshot = false
})

afterEach(async () => {
  await tearDown()
  vi.useRealTimers()
})

describe('a delta flagged resnapshot', () => {
  it('snapshots every subscribed group again with the strip, holding the cursor', async () => {
    await warmLaunch()
    server.resnapshot = true
    await pullDelta()
    await vi.waitFor(() => expect(pool.has('ticket', 2)).toBe(false))
    await vi.waitFor(() => expect(server.requests.at(-1)).toMatch(/^\/sync\/delta/))

    expect(bootstraps()).toEqual([[TICKET, WORKSPACE]])
    expect(pool.has('ticket', 1)).toBe(true)
    // No longer granted: the ticket group's rows and watermark go.
    expect(pool.has('comment', 40)).toBe(false)
    expect(fake.rows.has('comment:40')).toBe(false)
    expect(Object.keys(meta().get('group_watermarks') as object)).toEqual([WORKSPACE])
    // The cursor held for the snapshot (30) and the closing delta read from 20.
    expect(pool.getLastXid8()).toBe(20)
    expect(meta().get('last_xid8')).toBe(20)
  })

  it('runs one at a time, and signals during one ask for one more, not one each', async () => {
    await warmLaunch()
    server.denied = []
    server.rows[WORKSPACE] = [HOLD, ...server.rows[WORKSPACE]!]
    const first = resnapshotSubscribedGroups()
    const release = await reachHold()
    void resnapshotSubscribedGroups()
    void resnapshotSubscribedGroups()
    server.rows[WORKSPACE] = server.rows[WORKSPACE]!.filter((l) => !('__hold__' in l))

    release()
    await first
    expect(bootstraps()).toHaveLength(2)
  })

  it('leaves out a group a bootstrap is already streaming', async () => {
    await warmLaunch()
    server.denied = []
    server.rows['project:7'] = [HOLD]
    const opening = subscribe('project:7')
    const release = await reachHold()

    await resnapshotSubscribedGroups()
    expect(bootstraps()).toEqual([['project:7'], [TICKET, WORKSPACE]])
    release()
    await opening
  })

  it('waits for nothing while a resync runs: the resync snapshots every group', async () => {
    await warmLaunch()
    server.denied = []
    server.floor = 25
    server.rows[WORKSPACE] = [HOLD, ...server.rows[WORKSPACE]!]
    const resyncing = pullDelta()
    const release = await reachHold()

    await resnapshotSubscribedGroups()
    expect(bootstraps()).toHaveLength(1)
    release()
    await resyncing
    expect(bootstraps()).toHaveLength(1)
  })
})

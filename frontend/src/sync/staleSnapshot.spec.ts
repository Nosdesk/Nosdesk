/**
 * A group's snapshot is taken again once it grows past the max age the server
 * sends (a day when it sends none, never when it sends 0), timed on this
 * device's clock from when the snapshot completed. A watermark from before
 * ages were kept is stale. A stale group paints from the cache and catches
 * up by delta first, so its new snapshot holds the cursor and strips what
 * lingered; while the app runs, the tab becoming visible does the same. Live
 * events that land while such a snapshot streams are replayed over it.
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
  const drop = (keys: Array<[string, string]>) => {
    for (const [a, id] of keys) rows.delete(`${a}:${id}`)
  }
  return {
    dbs,
    rows,
    idb: {
      CACHE_GENERATION: 2,
      open: async (user: string, schema: string, slug: string | null) => ({
        db: { close() {} },
        name: `test:${user}:${schema}:${slug}`,
      }),
      wipe: async (name: string) => {
        dbs.delete(name)
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

/** The server: each group's snapshot rows, the max age it sends (none, like
 * a 1.1 server, when undefined), and every request it served. Snapshots end
 * at xid8 30; deltas at 20. */
const server = vi.hoisted(() => ({
  rows: {} as Record<string, Array<Record<string, unknown>>>,
  maxAge: undefined as number | undefined,
  requests: [] as string[],
  /** Groups bootstraps no longer grant (access was taken away). */
  denied: [] as string[],
  /** How many of the next deltas, and of the next bootstraps, fail. */
  failDeltas: 0,
  failBootstraps: 0,
  /** Live events the feed holds; a delta serves those past its cursor and
   * ends at the newest (20 when none is). */
  actions: [] as Array<Record<string, unknown> & { xid8: number }>,
  /** A stream waiting at a HOLD line; call it to go on. */
  held: null as null | (() => void),
}))

/** A point in a snapshot stream where it waits for `server.held()`. */
const HOLD = { __hold__: true } as const

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
    server.requests.push(url)
    const params = new URL(url, 'http://x').searchParams
    const groups = (params.get('groups') ?? '').split(',')
    const maxAge = server.maxAge === undefined ? {} : { snapshot_max_age_secs: server.maxAge }
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
            ...maxAge,
          },
        },
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
      if (server.failDeltas > 0) {
        server.failDeltas--
        return new Response(null, { status: 503 })
      }
      const from = Number(params.get('from_xid8') ?? 0)
      const actions = server.actions.filter((a) => a.xid8 > from)
      const last = Math.max(20, ...actions.map((a) => a.xid8))
      return new Response(
        JSON.stringify({
          actions,
          last_xid8: last,
          last_sync_id: last * 10,
          has_more: false,
          ...maxAge,
        }),
      )
    }
    return new Response(null, { status: 404 })
  }),
}))

const { hydrate, subscribe, tearDown, refreshStaleSnapshots, pullDelta, applySseFrame } =
  await import('./lifecycle')

const WORKSPACE = 'workspace:6'
const DB = 'test:u1:h:acme'
const NOW = new Date('2026-11-24T09:00:00Z').getTime()
const HOUR = 3_600_000

const meta = () => fake.dbs.get(DB)!
const bootstraps = () => server.requests.filter((u) => u.startsWith('/sync/bootstrap'))
const deltas = () => server.requests.filter((u) => u.startsWith('/sync/delta'))

/** A cache from last session: ticket 1 still on the server, ticket 2 deleted
 * while away, the workspace watermarked with `mark`. */
function lastSession(mark: unknown, maxAge?: number): void {
  const m = meta()
  m.set('last_xid8', 10)
  m.set('last_sync_id', 100)
  m.set('model_versions_tag', MODEL_VERSIONS_TAG)
  m.set('group_watermarks', { [WORKSPACE]: mark })
  if (maxAge !== undefined) m.set('snapshot_max_age_secs', maxAge)
  for (const id of [1, 2]) {
    fake.rows.set(`ticket:${id}`, {
      aggregate: 'ticket',
      id: String(id),
      schema_version: 1,
      data: { id, title: `Ticket ${id}` },
      owners: [WORKSPACE],
    })
  }
}

async function launch(): Promise<void> {
  await hydrate('u1', 'h', '', 'acme', '')
  expect(pool.has('ticket', 2)).toBe(true)
  await subscribe(WORKSPACE)
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

/** A ticket event as the feed carries it. */
function ticketEvent(
  xid8: number,
  op: 'U' | 'D',
  data: Record<string, unknown>,
): Record<string, unknown> & { xid8: number } {
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
  }
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date', 'setTimeout', 'clearTimeout', 'performance'] })
  vi.setSystemTime(NOW)
  fake.dbs.clear()
  fake.dbs.set(DB, new Map<string, unknown>([['schema_hash', 'h']]))
  fake.rows.clear()
  server.rows = { [WORKSPACE]: [{ __model__: 'ticket', id: 1, title: 'Ticket 1' }] }
  server.maxAge = undefined
  server.requests = []
  server.denied = []
  server.failDeltas = 0
  server.failBootstraps = 0
  server.actions = []
  server.held = null
})

afterEach(async () => {
  await tearDown()
  vi.useRealTimers()
})

describe('a launch', () => {
  it('catches a fresh snapshot up by delta alone', async () => {
    lastSession({ xid8: 10, at: NOW - HOUR })
    await launch()

    expect(bootstraps()).toEqual([])
    expect(deltas().length).toBeGreaterThan(0)
    // Nothing stripped the row deleted while away: only a snapshot would.
    expect(pool.has('ticket', 2)).toBe(true)
  })

  it('takes a numeric watermark from an older build as stale, and keeps the cursor', async () => {
    lastSession(10)
    await launch()

    expect(bootstraps()).toHaveLength(1)
    // Caught up by delta first, so the snapshot held the cursor.
    expect(server.requests[0]).toMatch(/^\/sync\/delta/)
    expect(pool.getLastXid8()).toBe(20)
    expect(meta().get('last_xid8')).toBe(20)
    expect(pool.has('ticket', 2)).toBe(false)
    expect(meta().get('group_watermarks')).toEqual({ [WORKSPACE]: { xid8: 30, at: NOW } })
  })

  it('takes a snapshot past a day as stale when the server sends no max age', async () => {
    lastSession({ xid8: 10, at: NOW - 25 * HOUR })
    await launch()

    expect(bootstraps()).toHaveLength(1)
    expect(pool.has('ticket', 2)).toBe(false)
  })

  it("follows the server's max age over the default", async () => {
    lastSession({ xid8: 10, at: NOW - 2 * HOUR }, 3600)
    await launch()

    expect(bootstraps()).toHaveLength(1)
  })

  it('never takes a snapshot as stale when the max age is 0', async () => {
    lastSession({ xid8: 10, at: NOW - 30 * 24 * HOUR }, 0)
    await launch()

    expect(bootstraps()).toEqual([])
  })

  it('still takes a numeric watermark as stale when the max age is 0', async () => {
    lastSession(10, 0)
    await launch()

    expect(bootstraps()).toHaveLength(1)
  })

  it('takes a snapshot stamped in the future (the clock was set back) as stale', async () => {
    lastSession({ xid8: 10, at: NOW + HOUR })
    await launch()

    expect(bootstraps()).toHaveLength(1)
  })
})

describe('a launch whose catch-up delta fails', () => {
  it('still holds the cursor through the stale snapshot', async () => {
    lastSession(10)
    server.failDeltas = 1
    await launch()

    expect(bootstraps()).toHaveLength(1)
    // Never the snapshot's 30: that would skip the events in between.
    expect(pool.getLastXid8()).toBe(20)
    expect(meta().get('last_xid8')).toBe(20)
    expect(pool.has('ticket', 2)).toBe(false)
  })
})

describe('an upgrade from 1.1', () => {
  it('paints the cache, catches up, snapshots once, and is warm the launch after', async () => {
    // What 1.1 left: a real cursor, rows without owners, a bare xid8.
    const m = meta()
    m.set('last_xid8', 10)
    m.set('last_sync_id', 100)
    m.set('model_versions_tag', MODEL_VERSIONS_TAG)
    m.set('group_watermarks', { [WORKSPACE]: 10 })
    fake.rows.set('ticket:1', {
      aggregate: 'ticket',
      id: '1',
      schema_version: 1,
      data: { id: 1, title: 'Ticket 1' },
    })
    fake.rows.set('ticket:2', {
      aggregate: 'ticket',
      id: '2',
      schema_version: 1,
      data: { id: 2, title: 'Deleted under 1.1' },
    })

    await hydrate('u1', 'h', '', 'acme', '')
    expect([...pool.ownersOf('ticket', 2)]).toEqual([pool.LEGACY_OWNER])
    await subscribe(WORKSPACE)

    expect(server.requests.map((u) => u.split('?')[0])).toEqual([
      '/sync/delta',
      '/sync/bootstrap',
      '/sync/delta',
    ])
    expect([...pool.ownersOf('ticket', 1)]).toEqual([WORKSPACE])
    expect(pool.has('ticket', 2)).toBe(false)
    expect(fake.rows.get('ticket:1')?.owners).toEqual([WORKSPACE])
    expect(fake.rows.has('ticket:2')).toBe(false)
    expect(meta().get('group_watermarks')).toEqual({ [WORKSPACE]: { xid8: 30, at: NOW } })
    expect(meta().get('last_xid8')).toBe(20)

    await tearDown()
    server.requests = []
    vi.setSystemTime(NOW + HOUR)
    await hydrate('u1', 'h', '', 'acme', '')
    await subscribe(WORKSPACE)
    expect(bootstraps()).toEqual([])
    expect(pool.get('ticket', 1)).toMatchObject({ title: 'Ticket 1' })
  })
})

describe('while running', () => {
  function becomeVisible(): void {
    Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'visible' })
    document.dispatchEvent(new Event('visibilitychange'))
  }

  it('snapshots again when the tab becomes visible past the max age, holding the cursor', async () => {
    lastSession({ xid8: 10, at: NOW - HOUR })
    await launch()
    expect(bootstraps()).toEqual([])

    vi.setSystemTime(NOW + 24 * HOUR)
    becomeVisible()
    // Each client waits a random while first, up to half a minute.
    await vi.advanceTimersByTimeAsync(30_000)
    await vi.waitFor(() => expect(bootstraps()).toHaveLength(1))
    await vi.waitFor(() =>
      expect(meta().get('group_watermarks')).toEqual({
        [WORKSPACE]: { xid8: 30, at: expect.toSatisfy((at: number) => at >= NOW + 24 * HOUR) },
      }),
    )

    expect(pool.has('ticket', 2)).toBe(false)
    expect(pool.has('ticket', 1)).toBe(true)
    expect(pool.getLastXid8()).toBe(20)
    expect(meta().get('last_xid8')).toBe(20)
  })

  it('replays the live events that land while the snapshot streams over its rows', async () => {
    lastSession({ xid8: 10, at: NOW - HOUR })
    for (const id of [5, 6]) {
      fake.rows.set(`ticket:${id}`, {
        aggregate: 'ticket',
        id: String(id),
        schema_version: 1,
        data: { id, title: `Ticket ${id}`, status: 'open' },
        owners: [WORKSPACE],
      })
    }
    await launch()

    // The snapshot was read before ticket 5 was closed and ticket 6 deleted.
    server.rows[WORKSPACE] = [
      { __model__: 'ticket', id: 1, title: 'Ticket 1' },
      HOLD,
      { __model__: 'ticket', id: 5, title: 'Ticket 5', status: 'open' },
      { __model__: 'ticket', id: 6, title: 'Ticket 6' },
    ]
    vi.setSystemTime(NOW + 25 * HOUR)
    const refreshing = refreshStaleSnapshots(0)
    const release = await reachHold()

    const close = ticketEvent(25, 'U', { id: 5, title: 'Ticket 5', status: 'closed' })
    const remove = ticketEvent(26, 'D', { id: 6 })
    server.actions = [close, remove]
    // The close arrives by SSE: its row applies, the cursor holds.
    applySseFrame([close as never], 25, 250)
    expect(pool.get('ticket', 5)).toMatchObject({ status: 'closed' })
    expect(pool.getLastXid8()).toBe(20)
    // The poll waits for the snapshot, so the delete comes with the replay.
    server.requests = []
    expect(await pullDelta()).toBe(false)
    expect(server.requests).toEqual([])

    release()
    await refreshing

    expect(pool.get('ticket', 5)).toMatchObject({ status: 'closed' })
    expect(pool.has('ticket', 6)).toBe(false)
    expect((fake.rows.get('ticket:5')?.data as Record<string, unknown>).status).toBe('closed')
    expect(fake.rows.has('ticket:6')).toBe(false)
    expect(pool.getLastXid8()).toBe(26)
    expect(meta().get('last_xid8')).toBe(26)
  })

  it('does not start a second snapshot of a group whose launch snapshot is still streaming', async () => {
    lastSession(10)
    server.rows[WORKSPACE] = [HOLD, { __model__: 'ticket', id: 1, title: 'Ticket 1' }]
    await hydrate('u1', 'h', '', 'acme', '')
    const launching = subscribe(WORKSPACE)
    const release = await reachHold()

    await refreshStaleSnapshots(0)
    expect(bootstraps()).toHaveLength(1)
    release()
    await launching
    expect(bootstraps()).toHaveLength(1)
  })

  it('drops the snapshot of a group the server no longer grants', async () => {
    lastSession({ xid8: 10, at: NOW - HOUR })
    meta().set('group_watermarks', {
      [WORKSPACE]: { xid8: 10, at: NOW - HOUR },
      'ticket:9': { xid8: 10, at: NOW - HOUR },
    })
    fake.rows.set('comment:40', {
      aggregate: 'comment',
      id: '40',
      schema_version: 1,
      data: { id: 40, ticket_id: 9 },
      owners: ['ticket:9'],
    })
    await hydrate('u1', 'h', '', 'acme', '')
    await Promise.all([subscribe(WORKSPACE), subscribe('ticket:9')])
    expect(pool.has('comment', 40)).toBe(true)

    server.denied = ['ticket:9']
    vi.setSystemTime(NOW + 25 * HOUR)
    await refreshStaleSnapshots(0)

    expect(pool.has('comment', 40)).toBe(false)
    expect(fake.rows.has('comment:40')).toBe(false)
    expect(Object.keys(meta().get('group_watermarks') as object)).toEqual([WORKSPACE])
    // Nothing is left to take again.
    vi.advanceTimersByTime(60 * 60_000)
    server.requests = []
    await refreshStaleSnapshots(0)
    expect(bootstraps()).toEqual([])
  })

  it('waits before trying again a snapshot that failed', async () => {
    lastSession({ xid8: 10, at: NOW - HOUR })
    await launch()
    server.failBootstraps = 2

    vi.setSystemTime(NOW + 24 * HOUR)
    await refreshStaleSnapshots(0)
    await refreshStaleSnapshots(0)
    expect(bootstraps()).toHaveLength(1)

    vi.advanceTimersByTime(10 * 60_000)
    await refreshStaleSnapshots(0)
    expect(bootstraps()).toHaveLength(2)
  })

  it('leaves a snapshot within the max age alone', async () => {
    lastSession({ xid8: 10, at: NOW - HOUR })
    await launch()

    vi.setSystemTime(NOW + 10 * HOUR)
    await refreshStaleSnapshots(0)
    expect(bootstraps()).toEqual([])
  })

  it("judges by this session's snapshot, not one another tab took since", async () => {
    lastSession({ xid8: 10, at: NOW - HOUR })
    await launch()
    vi.setSystemTime(NOW + 24 * HOUR)
    // Another tab of the workspace just snapshotted it.
    meta().set('group_watermarks', { [WORKSPACE]: { xid8: 40, at: NOW + 24 * HOUR } })

    await refreshStaleSnapshots(0)
    expect(bootstraps()).toHaveLength(1)
    expect(pool.has('ticket', 2)).toBe(false)
  })

  it('judges by the max age the server sends with a delta, and keeps it for the next launch', async () => {
    lastSession({ xid8: 10, at: NOW - HOUR })
    server.maxAge = 60
    await launch()
    expect(meta().get('snapshot_max_age_secs')).toBe(60)

    await refreshStaleSnapshots(0)
    expect(bootstraps()).toHaveLength(1)
    expect(pool.has('ticket', 2)).toBe(false)
  })
})

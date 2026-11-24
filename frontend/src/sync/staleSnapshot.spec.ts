/**
 * A group's snapshot is taken again once it grows past the max age the server
 * sends (a day when it sends none, never when it sends 0), timed on this
 * device's clock from when the snapshot completed. A watermark from before
 * ages were kept is stale. A stale group paints from the cache and catches
 * up by delta first, so its new snapshot holds the cursor and strips what
 * lingered; while the app runs, the tab becoming visible does the same.
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

/** The server: each group's snapshot rows, the max age it sends (none, like
 * a 1.1 server, when undefined), and every request it served. Snapshots end
 * at xid8 30; deltas at 20. */
const server = vi.hoisted(() => ({
  rows: {} as Record<string, Array<Record<string, unknown>>>,
  maxAge: undefined as number | undefined,
  requests: [] as string[],
  /** Bootstraps grant no group (access was taken away). */
  grantsNone: false,
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
    server.requests.push(url)
    const params = new URL(url, 'http://x').searchParams
    const groups = (params.get('groups') ?? '').split(',')
    const maxAge = server.maxAge === undefined ? {} : { snapshot_max_age_secs: server.maxAge }
    if (url.startsWith('/sync/bootstrap')) {
      const end = { last_xid8: 30, last_sync_id: 300 }
      const lines = [
        {
          __meta__: {
            server_schema: 'h',
            ...end,
            groups_granted: server.grantsNone ? [] : groups,
            ...maxAge,
          },
        },
        ...groups.flatMap((g) => server.rows[g] ?? []),
        { __end__: end },
      ]
      return new Response(lines.map((l) => JSON.stringify(l)).join('\n') + '\n')
    }
    if (url.startsWith('/sync/delta')) {
      return new Response(
        JSON.stringify({ actions: [], last_xid8: 20, last_sync_id: 200, has_more: false, ...maxAge }),
      )
    }
    return new Response(null, { status: 404 })
  }),
}))

const { hydrate, subscribe, tearDown, refreshStaleSnapshots } = await import('./lifecycle')

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

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime(NOW)
  fake.dbs.clear()
  fake.dbs.set(DB, new Map<string, unknown>([['schema_hash', 'h']]))
  fake.rows.clear()
  server.rows = { [WORKSPACE]: [{ __model__: 'ticket', id: 1, title: 'Ticket 1' }] }
  server.maxAge = undefined
  server.requests = []
  server.grantsNone = false
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
    // vi.waitFor moves the faked clock along as it polls.
    await vi.waitFor(() =>
      expect(meta().get('group_watermarks')).toEqual({
        [WORKSPACE]: { xid8: 30, at: expect.toSatisfy((at: number) => at >= NOW + 24 * HOUR) },
      }),
    )

    expect(bootstraps()).toHaveLength(1)
    expect(pool.has('ticket', 2)).toBe(false)
    expect(pool.has('ticket', 1)).toBe(true)
    expect(pool.getLastXid8()).toBe(20)
    expect(meta().get('last_xid8')).toBe(20)
  })

  it('waits before trying again a group the server no longer grants', async () => {
    lastSession({ xid8: 10, at: NOW - HOUR })
    await launch()
    server.grantsNone = true

    vi.setSystemTime(NOW + 24 * HOUR)
    await refreshStaleSnapshots()
    await refreshStaleSnapshots()
    expect(bootstraps()).toHaveLength(1)

    vi.setSystemTime(NOW + 24 * HOUR + 10 * 60_000)
    await refreshStaleSnapshots()
    expect(bootstraps()).toHaveLength(2)
  })

  it('leaves a snapshot within the max age alone', async () => {
    lastSession({ xid8: 10, at: NOW - HOUR })
    await launch()

    vi.setSystemTime(NOW + 10 * HOUR)
    await refreshStaleSnapshots()
    expect(bootstraps()).toEqual([])
  })

  it('judges by the max age the server sends with a delta, and keeps it for the next launch', async () => {
    lastSession({ xid8: 10, at: NOW - HOUR })
    server.maxAge = 60
    await launch()
    expect(meta().get('snapshot_max_age_secs')).toBe(60)

    await refreshStaleSnapshots()
    expect(bootstraps()).toHaveLength(1)
    expect(pool.has('ticket', 2)).toBe(false)
  })
})

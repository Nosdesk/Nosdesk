/**
 * The workspace data fence. When a workspace's data is replaced (an archive
 * import gives every row a new id), the server moves the workspace's data
 * epoch. A client that sees a new one, on launch or on any delta, drops its
 * sync pool, cursor and collaborative-document caches and bootstraps again.
 * The same epoch, or none, leaves everything alone.
 */
import { ref } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import * as pool from '@nosdesk/core/sync/pool'

/** An in-memory stand-in for the sync IndexedDB: one meta map per database. */
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
    wipes: [] as string[],
    idb: {
      CACHE_GENERATION: 2,
      open: async (user: string, schema: string, slug: string | null) => ({
        db: { close() {} },
        name: `test:${user}:${schema}:${slug}`,
      }),
      wipe: async (name: string) => {
        fake.wipes.push(name)
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

const purgeAllCollabDocs = vi.hoisted(() => vi.fn(async (_opts: { includeOpen: boolean }) => {}))
/** Whether the session is gone, as a refused refresh leaves it. */
const session = vi.hoisted(() => ({ gone: false }))
vi.mock('@nosdesk/core/services/session', () => ({ sessionGone: () => session.gone }))
const server = vi.hoisted(() => ({
  /** Rows each bootstrap returns, in order; the last one repeats. */
  snapshots: [] as Array<Array<Record<string, unknown>>>,
  bootstraps: 0,
  delta: {} as Record<string, unknown>,
}))

vi.mock('./idb', () => fake.idb)
vi.mock('./queue', () => ({ setIdbHandle: () => {}, flush: async () => {} }))
vi.mock('@/utils/collabLocalCache', () => ({ purgeAllCollabDocs }))
vi.mock('@/services/activeWorkspace', () => ({
  workspaceReady: () => true,
  workspaceReadyRef: ref(true),
}))
vi.mock('@nosdesk/core/services/ticketService', () => ({ default: {} }))
vi.mock('@/sync/stores/tickets', () => ({ apiTicketToSync: (t: unknown) => t }))
vi.mock('@/services/authFetch', () => ({
  authFetch: vi.fn(async (url: string) => {
    if (url.startsWith('/sync/bootstrap')) {
      const rows = server.snapshots[Math.min(server.bootstraps, server.snapshots.length - 1)]
      server.bootstraps += 1
      const lines = [
        {
          __meta__: {
            server_schema: 'h',
            last_xid8: 10,
            last_sync_id: 100,
            groups_granted: ['project:5'],
          },
        },
        ...rows.map((r) => ({ __model__: 'ticket', ...r })),
        { __end__: { last_xid8: 10, last_sync_id: 100 } },
      ]
      return new Response(lines.map((l) => JSON.stringify(l)).join('\n') + '\n')
    }
    if (url.startsWith('/sync/delta')) return new Response(JSON.stringify(server.delta))
    return new Response(null, { status: 404 })
  }),
}))

const { hydrate, subscribe, pullDelta, tearDown } = await import('./lifecycle')
const { authFetch } = await import('@/services/authFetch')

const DB = 'test:u1:h:acme'
const GROUP = 'project:5'

/** A cache written under `epoch` by an earlier launch. */
function cachedUnder(epoch: string | undefined) {
  const meta = new Map<string, unknown>([['schema_hash', 'h']])
  if (epoch !== undefined) meta.set('workspace_epoch', epoch)
  fake.dbs.set(DB, meta)
}

function emptyDelta(extra: Record<string, unknown> = {}) {
  return { actions: [], last_xid8: 10, last_sync_id: 100, has_more: false, ...extra }
}

beforeEach(() => {
  fake.dbs.clear()
  fake.wipes.length = 0
  purgeAllCollabDocs.mockClear()
  server.snapshots = [[{ id: 1, title: 'Before the import' }]]
  server.bootstraps = 0
  server.delta = emptyDelta()
  session.gone = false
})

afterEach(async () => {
  await tearDown()
})

describe('on launch', () => {
  it('wipes every cache when the server reports a new workspace epoch', async () => {
    cachedUnder('e1')
    await hydrate('u1', 'h', '', 'acme', 'e2')
    expect(fake.wipes).toEqual([DB])
    expect(purgeAllCollabDocs).toHaveBeenCalledWith({ includeOpen: true })
    expect(fake.dbs.get(DB)?.get('workspace_epoch')).toBe('e2')
  })

  it('wipes a cache from before the first replacement', async () => {
    cachedUnder(undefined)
    await hydrate('u1', 'h', '', 'acme', 'e1')
    expect(fake.wipes).toEqual([DB])
    expect(purgeAllCollabDocs).toHaveBeenCalledWith({ includeOpen: true })
  })

  it('keeps everything under the same epoch', async () => {
    cachedUnder('e1')
    await hydrate('u1', 'h', '', 'acme', 'e1')
    expect(fake.wipes).toEqual([])
    expect(purgeAllCollabDocs).not.toHaveBeenCalled()
  })

  it('keeps everything when the server reports no epoch', async () => {
    cachedUnder('e1')
    await hydrate('u1', 'h', '', 'acme', '')
    expect(fake.wipes).toEqual([])
    expect(purgeAllCollabDocs).not.toHaveBeenCalled()
    expect(fake.dbs.get(DB)?.get('workspace_epoch')).toBe('e1')
  })
})

describe('while running', () => {
  async function running(epoch: string) {
    cachedUnder(epoch || undefined)
    await hydrate('u1', 'h', '', 'acme', epoch)
    await subscribe(GROUP)
    expect(pool.has('ticket', 1)).toBe(true)
    fake.wipes.length = 0
    purgeAllCollabDocs.mockClear()
  }

  it('wipes every cache and bootstraps again when a delta carries a new epoch', async () => {
    await running('e1')
    server.snapshots = [[{ id: 2, title: 'After the import' }]]
    server.delta = emptyDelta({
      workspace_epoch: 'e2',
      actions: [
        {
          sync_id: 101,
          aggregate: 'ticket',
          aggregate_id: '3',
          op: 'insert',
          event_type: 'ticket.created',
          schema_version: 1,
          data: { id: 3, title: 'Not applied' },
          groups: [GROUP],
          occurred_at: '2026-10-19T00:00:00Z',
        },
      ],
    })

    await pullDelta()

    expect(fake.wipes).toEqual([DB])
    expect(purgeAllCollabDocs).toHaveBeenCalledWith({ includeOpen: true })
    expect(server.bootstraps).toBe(2)
    // The old row is gone, the new snapshot is in, and the delta page from
    // the old generation was not applied.
    expect(pool.has('ticket', 1)).toBe(false)
    expect(pool.has('ticket', 2)).toBe(true)
    expect(pool.has('ticket', 3)).toBe(false)
    expect(pool.getSubscribedGroups().has(GROUP)).toBe(true)
    const meta = fake.dbs.get(DB)
    expect(meta?.get('workspace_epoch')).toBe('e2')
    expect(meta?.get('schema_hash')).toBe('h')
    expect(meta?.get('last_sync_id')).toBe(100)

    // The next poll under the new epoch leaves it alone.
    fake.wipes.length = 0
    purgeAllCollabDocs.mockClear()
    server.delta = emptyDelta({ workspace_epoch: 'e2' })
    await pullDelta()
    expect(fake.wipes).toEqual([])
    expect(purgeAllCollabDocs).not.toHaveBeenCalled()
  })

  it('learns of the first replacement from a delta', async () => {
    await running('')
    server.delta = emptyDelta({ workspace_epoch: 'e1', resync_required: true })
    await pullDelta()
    expect(fake.wipes).toEqual([DB])
    expect(purgeAllCollabDocs).toHaveBeenCalledWith({ includeOpen: true })
  })

  it('keeps everything under the same epoch, or none', async () => {
    await running('e1')
    server.delta = emptyDelta({ workspace_epoch: 'e1' })
    await pullDelta()
    server.delta = emptyDelta({ workspace_epoch: null })
    await pullDelta()
    server.delta = emptyDelta()
    await pullDelta()
    expect(fake.wipes).toEqual([])
    expect(purgeAllCollabDocs).not.toHaveBeenCalled()
    expect(server.bootstraps).toBe(1)
    expect(pool.has('ticket', 1)).toBe(true)
  })

  it('resyncs an aged-out cursor without touching document caches', async () => {
    await running('e1')
    server.delta = emptyDelta({ workspace_epoch: 'e1', resync_required: true })
    await pullDelta()
    expect(fake.wipes).toEqual([DB])
    expect(purgeAllCollabDocs).not.toHaveBeenCalled()
    expect(server.bootstraps).toBe(2)
  })
})

describe('after the session is gone', () => {
  it('sends no delta, so no refresh is asked for every poll', async () => {
    cachedUnder('e1')
    await hydrate('u1', 'h', '', 'acme', 'e1')
    await subscribe(GROUP)
    vi.mocked(authFetch).mockClear()

    session.gone = true
    expect(await pullDelta()).toBe(false)
    expect(authFetch).not.toHaveBeenCalled()

    // Signed in again: polling resumes.
    session.gone = false
    await pullDelta()
    expect(authFetch).toHaveBeenCalledTimes(1)
  })
})

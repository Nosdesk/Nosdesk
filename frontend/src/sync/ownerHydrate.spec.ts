/**
 * What a launch keeps from the cache. A row of a snapshot-backed aggregate
 * keeps only the owners whose group holds a watermark, `legacy` only while a
 * workspace group holds one, and is dropped from the pool and the cache when
 * none is left. `cycle` rows are outside ownership and stay as cached. The
 * live-only aggregates are never cached.
 */
import { ref } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import * as pool from '@nosdesk/core/sync/pool'
import type { SyncAction } from '@nosdesk/core/sync/types'
import type { IdbHandle, ModelRow } from './idb'

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
  const store = {
    cached: [] as unknown[],
    put: [] as Array<{ aggregate: string; id: string; owners?: string[] }>,
    deleted: [] as string[],
  }
  return {
    dbs,
    store,
    idb: {
      CACHE_GENERATION: 2,
      open: async (user: string, schema: string, slug: string | null) => ({
        db: { close() {} },
        name: `test:${user}:${schema}:${slug}`,
      }),
      wipe: async (name: string) => {
        dbs.delete(name)
      },
      putModels: async (_h: unknown, rows: Array<{ aggregate: string; id: string; owners?: string[] }>) => {
        store.put.push(...rows)
      },
      loadModels: async () => store.cached,
      deleteModel: async () => {},
      deleteModels: async (_h: unknown, keys: Array<[string, string]>) => {
        store.deleted.push(...keys.map(([a, id]) => `${a}:${id}`))
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
      setGroupWatermarks: async () => {},
      replaceGroupWatermarks: set('group_watermarks'),
      setModelVersionsTag: set('model_versions_tag'),
      getModelVersionsTag: get('model_versions_tag'),
    },
  }
})

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
  authFetch: vi.fn(async () => new Response(null, { status: 404 })),
}))

const { hydrate, applySseFrame, tearDown } = await import('./lifecycle')

const WORKSPACE = 'workspace:6'
const DB = 'test:u1:h:acme'

function cached(aggregate: string, id: number, rowOwners?: string[]): ModelRow {
  return {
    aggregate: aggregate as ModelRow['aggregate'],
    id: String(id),
    schema_version: 1,
    data: { id },
    owners: rowOwners,
  }
}

async function launch(marks: Record<string, number>, rows: ModelRow[]): Promise<void> {
  fake.dbs.get(DB)!.set('group_watermarks', marks)
  fake.store.cached = rows
  await hydrate('u1', 'h', '', 'acme', '')
}

const owners = (aggregate: 'ticket' | 'comment' | 'cycle', id: number) =>
  [...pool.ownersOf(aggregate, id)].sort()

beforeEach(() => {
  fake.dbs.clear()
  fake.dbs.set(DB, new Map<string, unknown>([['schema_hash', 'h']]))
  fake.store.cached = []
  fake.store.put = []
  fake.store.deleted = []
})

afterEach(async () => {
  await tearDown()
})

describe('hydrate', () => {
  it('keeps a row whose owner holds a watermark, with only the watermarked owners', async () => {
    await launch({ [WORKSPACE]: 5 }, [cached('ticket', 1, [WORKSPACE, 'ticket:1'])])
    expect(pool.has('ticket', 1)).toBe(true)
    expect(owners('ticket', 1)).toEqual([WORKSPACE])
    expect(fake.store.deleted).toEqual([])
  })

  it('drops a row no watermarked group owns, from the pool and the cache', async () => {
    await launch({ [WORKSPACE]: 5 }, [cached('comment', 40, ['ticket:9'])])
    expect(pool.has('comment', 40)).toBe(false)
    expect(fake.store.deleted).toEqual(['comment:40'])
  })

  it('drops a row that was only `local` when cached', async () => {
    await launch({ [WORKSPACE]: 5 }, [
      cached('ticket', 1, []),
      cached('ticket', 2, [pool.LOCAL_OWNER]),
    ])
    expect(pool.has('ticket', 1)).toBe(false)
    expect(pool.has('ticket', 2)).toBe(false)
    expect(fake.store.deleted.sort()).toEqual(['ticket:1', 'ticket:2'])
  })

  it('keeps a `legacy` row only while a workspace group holds a watermark', async () => {
    await launch({ [WORKSPACE]: 5 }, [cached('ticket', 1)])
    expect(owners('ticket', 1)).toEqual([pool.LEGACY_OWNER])
    await tearDown()

    await launch({ 'ticket:1': 5 }, [cached('ticket', 1)])
    expect(pool.has('ticket', 1)).toBe(false)
    expect(fake.store.deleted).toEqual(['ticket:1'])
  })

  it('leaves `cycle` rows as cached, with or without watermarks', async () => {
    await launch({}, [cached('cycle', 3, []), cached('cycle', 4, ['project:2'])])
    expect(pool.has('cycle', 3)).toBe(true)
    expect(pool.has('cycle', 4)).toBe(true)
    expect(owners('cycle', 4)).toEqual(['project:2'])
    expect(fake.store.deleted).toEqual([])
  })
})

describe('the live-only aggregates', () => {
  it('are applied to the pool but never cached', async () => {
    await launch({}, [])
    pool.subscribe(WORKSPACE)
    const live = (aggregate: string, data: Record<string, unknown>): SyncAction => ({
      sync_id: 1,
      aggregate: aggregate as SyncAction['aggregate'],
      aggregate_id: String(data.id ?? data.ticket_id),
      op: 'I',
      event_type: `${aggregate}.created`,
      schema_version: 1,
      data,
      groups: [WORKSPACE],
    })
    applySseFrame(
      [
        live('asset_media', { id: 1, asset_id: 7 }),
        live('asset_lifecycle_event', { id: 2, asset_id: 7 }),
        live('plugin', { uuid: 'p1' }),
        live('group_membership', { id: 3 }),
        live('assignment', { ticket_id: 9 }),
        live('ticket', { id: 9, title: 'Cached' }),
      ],
      11,
      110,
    )
    expect(pool.has('asset_media', 1)).toBe(true)
    expect(pool.has('assignment', 9)).toBe(true)
    expect(fake.store.put.map((r) => r.aggregate)).toEqual(['ticket'])
  })

  it('are deleted from the cache when it is read', async () => {
    const real = await vi.importActual<typeof import('./idb')>('./idb')
    const rows = [
      cached('ticket', 1, [WORKSPACE]),
      cached('asset_media', 2, [WORKSPACE]),
      cached('user', 3, [WORKSPACE]),
    ]
    const deleted: unknown[] = []
    const tx: { oncomplete?: () => void; objectStore: () => unknown } = {
      objectStore: () => ({
        getAll: () => {
          const req: { result?: unknown; onsuccess?: () => void } = {}
          queueMicrotask(() => {
            req.result = rows
            req.onsuccess?.()
            queueMicrotask(() => tx.oncomplete?.())
          })
          return req
        },
        delete: (key: unknown) => void deleted.push(key),
      }),
    }
    const handle = { db: { transaction: () => tx }, name: 'test' } as unknown as IdbHandle

    const kept = await real.loadModels(handle, { ticket: 1, user: 2 })
    expect(kept.map((r) => r.aggregate)).toEqual(['ticket'])
    // Not cached any more, and cached under an older version.
    expect(deleted).toEqual([
      ['asset_media', '2'],
      ['user', '3'],
    ])
  })
})

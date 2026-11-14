/**
 * Row owners: the groups whose snapshot or live events put a row in the pool.
 * A snapshot stamps the groups the server granted, a live action the groups
 * it reaches that the pool subscribes to, a REST or optimistic write `local`,
 * and a row cached by a build without owners `legacy`. The cache keeps every
 * owner but `local`.
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
    /** What the next hydrate reads back. */
    cached: [] as unknown[],
    /** Every row written, in order. */
    put: [] as Array<{ aggregate: string; id: string; owners?: string[] }>,
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
      deleteModels: async () => {},
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

/** The server: which requested groups a bootstrap grants, and its rows. */
const server = vi.hoisted(() => ({
  denied: new Set<string>(),
  rows: [] as Array<Record<string, unknown>>,
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
    const groups = (params.get('groups') ?? '').split(',')
    if (url.startsWith('/sync/bootstrap')) {
      const end = { last_xid8: 10, last_sync_id: 100 }
      const granted = groups.filter((g) => !server.denied.has(g))
      const lines = [
        { __meta__: { server_schema: 'h', ...end, groups_granted: granted } },
        ...server.rows,
        { __end__: end },
      ]
      return new Response(lines.map((l) => JSON.stringify(l)).join('\n') + '\n')
    }
    if (url.startsWith('/sync/delta')) {
      return new Response(
        JSON.stringify({ actions: [], last_xid8: 10, last_sync_id: 100, has_more: false }),
      )
    }
    return new Response(null, { status: 404 })
  }),
}))

const { hydrate, subscribe, runBootstrap, applySseFrame, tearDown } = await import('./lifecycle')

const WORKSPACE = 'workspace:6'

const owners = (aggregate: 'ticket' | 'comment', id: number) =>
  [...pool.ownersOf(aggregate, id)].sort()

const cachedOwners = (aggregate: string, id: number) =>
  fake.store.put.filter((r) => r.aggregate === aggregate && r.id === String(id)).at(-1)?.owners

function action(
  aggregate: 'ticket' | 'comment',
  data: Record<string, unknown>,
  groups: Array<string | null>,
  op: 'I' | 'U' | 'D' = 'I',
): SyncAction {
  return {
    sync_id: 1,
    aggregate,
    aggregate_id: String(data.id),
    op,
    event_type: `${aggregate}.created`,
    schema_version: 1,
    data,
    groups,
  }
}

beforeEach(() => {
  fake.dbs.clear()
  fake.dbs.set('test:u1:h:acme', new Map<string, unknown>([['schema_hash', 'h']]))
  fake.store.cached = []
  fake.store.put = []
  server.denied = new Set()
  server.rows = []
})

afterEach(async () => {
  await tearDown()
})

describe('snapshot rows', () => {
  it('are owned by the groups the server granted', async () => {
    server.rows = [
      { __model__: 'ticket', id: 9, title: 'Printer' },
      { __model__: 'comment', id: 40, ticket_id: 9 },
    ]
    server.denied = new Set(['ticket:77'])
    await hydrate('u1', 'h', '', 'acme', '')
    pool.subscribe(WORKSPACE)
    pool.subscribe('ticket:77')
    await runBootstrap([WORKSPACE, 'ticket:77'])

    // ticket:77 was asked for alongside but not granted: it owns nothing.
    expect(owners('ticket', 9)).toEqual([WORKSPACE])
    expect(owners('comment', 40)).toEqual([WORKSPACE])
    expect(cachedOwners('ticket', 9)).toEqual([WORKSPACE])
  })

  it('add to the owners a row already has', async () => {
    server.rows = [{ __model__: 'ticket', id: 9, title: 'Printer' }]
    await hydrate('u1', 'h', '', 'acme', '')
    await subscribe(WORKSPACE)
    await subscribe('ticket:9')

    expect(owners('ticket', 9)).toEqual([WORKSPACE, 'ticket:9'].sort())
    expect(cachedOwners('ticket', 9)?.sort()).toEqual([WORKSPACE, 'ticket:9'].sort())
  })

  it('are not touched by a live action', async () => {
    server.rows = [{ __model__: 'ticket', id: 9, title: 'Printer' }]
    await hydrate('u1', 'h', '', 'acme', '')
    await subscribe(WORKSPACE)
    expect(pool.lastTouch('ticket', 9)).toBe(0)
  })
})

describe('live rows', () => {
  beforeEach(async () => {
    await hydrate('u1', 'h', '', 'acme', '')
    pool.subscribe(WORKSPACE)
  })

  it('are owned by the groups the action reaches that the pool subscribes to', () => {
    applySseFrame([action('comment', { id: 41, ticket_id: 9 }, [WORKSPACE, 'ticket:9', null])], 11, 110)
    expect(owners('comment', 41)).toEqual([WORKSPACE])
    expect(cachedOwners('comment', 41)).toEqual([WORKSPACE])
  })

  it('mark the row touched after the tick a bootstrap would read at its start', () => {
    pool.upsert('ticket', 8, { id: 8, title: 'Untouched' }, [WORKSPACE])
    const mark = pool.currentTouch()
    applySseFrame([action('ticket', { id: 9, title: 'Live' }, [WORKSPACE])], 11, 110)
    expect(pool.lastTouch('ticket', 9)).toBeGreaterThan(mark)
    expect(pool.lastTouch('ticket', 8)).toBe(0)
  })

  it('reaching no subscribed group create a row with no owners', () => {
    applySseFrame([action('comment', { id: 42, ticket_id: 5 }, ['ticket:5'])], 11, 110)
    expect(pool.has('comment', 42)).toBe(true)
    expect(owners('comment', 42)).toEqual([])
  })

  it('leave nothing behind when deleted', () => {
    applySseFrame([action('ticket', { id: 9, title: 'Live' }, [WORKSPACE])], 11, 110)
    applySseFrame([action('ticket', { id: 9 }, [WORKSPACE], 'D')], 12, 120)
    expect(owners('ticket', 9)).toEqual([])
    expect(pool.lastTouch('ticket', 9)).toBe(0)
  })
})

describe('local rows', () => {
  beforeEach(() => pool.reset())

  it('are owned `local` when a write without owners creates them', () => {
    pool.upsert('ticket', 9, { id: 9, title: 'From REST' })
    expect(owners('ticket', 9)).toEqual([pool.LOCAL_OWNER])
  })

  it('keep their owners when a write without owners updates them', () => {
    pool.upsert('ticket', 9, { id: 9, title: 'Streamed' }, [WORKSPACE])
    pool.upsert('ticket', 9, { title: 'Refetched' })
    expect(owners('ticket', 9)).toEqual([WORKSPACE])
  })

  it('lose `local` when a stream reaches them, and never cache it', async () => {
    await hydrate('u1', 'h', '', 'acme', '')
    pool.subscribe(WORKSPACE)
    pool.upsert('ticket', 9, { id: 9, title: 'Optimistic' })
    applySseFrame([action('ticket', { id: 9, title: 'Echo' }, [WORKSPACE])], 11, 110)
    expect(owners('ticket', 9)).toEqual([WORKSPACE])
    expect(cachedOwners('ticket', 9)).toEqual([WORKSPACE])
  })
})

describe('cached rows', () => {
  it('are owned `legacy` when cached without owners, and drop a cached `local`', async () => {
    fake.store.cached = [
      { aggregate: 'ticket', id: '9', schema_version: 1, data: { id: 9, title: 'From 1.1' } },
      {
        aggregate: 'comment',
        id: '40',
        schema_version: 1,
        data: { id: 40, ticket_id: 9 },
        owners: ['ticket:9', pool.LOCAL_OWNER],
      },
      { aggregate: 'comment', id: '41', schema_version: 1, data: { id: 41 }, owners: [] },
    ] satisfies ModelRow[]
    await hydrate('u1', 'h', '', 'acme', '')

    expect(owners('ticket', 9)).toEqual([pool.LEGACY_OWNER])
    expect(owners('comment', 40)).toEqual(['ticket:9'])
    expect(owners('comment', 41)).toEqual([])
  })
})

describe('the owner map', () => {
  beforeEach(() => pool.reset())

  it('is left alone by a patch', () => {
    pool.upsert('ticket', 9, { id: 9, title: 'Before' }, [WORKSPACE, 'ticket:9'])
    pool.patch('ticket', 9, { title: 'After' })
    expect(owners('ticket', 9)).toEqual([WORKSPACE, 'ticket:9'].sort())
  })

  it('strips one owner at a time and reports what is left', () => {
    pool.upsert('ticket', 9, { id: 9 }, [WORKSPACE, 'ticket:9'])
    expect(pool.stripOwner('ticket', 9, 'ticket:9')).toBe(1)
    expect(pool.stripOwner('ticket', 9, 'project:3')).toBe(1)
    expect(pool.stripOwner('ticket', 9, WORKSPACE)).toBe(0)
    expect(pool.stripOwner('ticket', 404, WORKSPACE)).toBe(0)
  })

  it('adds owners only to rows the pool holds', () => {
    pool.upsert('ticket', 9, { id: 9 }, [WORKSPACE])
    pool.addOwners('ticket', 9, ['ticket:9'])
    pool.addOwners('ticket', 404, ['ticket:404'])
    expect(owners('ticket', 9)).toEqual([WORKSPACE, 'ticket:9'].sort())
    expect(pool.has('ticket', 404)).toBe(false)
    expect(owners('ticket', 404)).toEqual([])
  })

  it('shares a set across rows without one row changing another', () => {
    const shared = new Set([WORKSPACE])
    pool.upsert('ticket', 1, { id: 1 }, shared)
    pool.upsert('ticket', 2, { id: 2 }, shared)
    pool.addOwners('ticket', 1, ['ticket:1'])
    pool.stripOwner('ticket', 2, WORKSPACE)
    expect(owners('ticket', 1)).toEqual([WORKSPACE, 'ticket:1'].sort())
    expect(owners('ticket', 2)).toEqual([])
    expect([...shared]).toEqual([WORKSPACE])
  })

  it('is cleared by a reset, with the touches', () => {
    pool.upsert('ticket', 9, { id: 9 }, [WORKSPACE])
    pool.touch('ticket', 9)
    pool.reset()
    expect(owners('ticket', 9)).toEqual([])
    expect(pool.lastTouch('ticket', 9)).toBe(0)
    expect(pool.currentTouch()).toBe(0)
    // A row created again starts over.
    pool.upsert('ticket', 9, { id: 9 })
    expect(owners('ticket', 9)).toEqual([pool.LOCAL_OWNER])
  })
})

describe('deleteModels', () => {
  /** A database stand-in that counts transactions and records deletes. */
  function fakeHandle() {
    const deleted: unknown[] = []
    const transaction = vi.fn(() => {
      const tx: { oncomplete?: () => void; onerror?: () => void; onabort?: () => void } & {
        objectStore: () => { delete: (key: unknown) => void }
      } = {
        objectStore: () => ({ delete: (key: unknown) => void deleted.push(key) }),
      }
      queueMicrotask(() => tx.oncomplete?.())
      return tx
    })
    const handle = { db: { transaction }, name: 'test' } as unknown as IdbHandle
    return { handle, transaction, deleted }
  }

  it('deletes every row in one transaction', async () => {
    const real = await vi.importActual<typeof import('./idb')>('./idb')
    const { handle, transaction, deleted } = fakeHandle()
    const keys = Array.from({ length: 2000 }, (_, i) => ['comment', String(i)] as const)

    await real.deleteModels(handle, keys)
    expect(transaction).toHaveBeenCalledTimes(1)
    expect(deleted).toHaveLength(2000)
    expect(deleted[0]).toEqual(['comment', '0'])
  })

  it('opens no transaction for no rows', async () => {
    const real = await vi.importActual<typeof import('./idb')>('./idb')
    const { handle, transaction } = fakeHandle()
    await real.deleteModels(handle, [])
    expect(transaction).not.toHaveBeenCalled()
  })
})

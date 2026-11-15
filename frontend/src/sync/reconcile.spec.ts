/**
 * The snapshot strip. At a bootstrap's end each granted group comes off every
 * row of a snapshot-backed aggregate the stream did not carry, and a row left
 * with no owners is removed from the pool and the cache. Groups torn by this
 * session's first cursor advance are stripped first, rows a live action wrote
 * after the bootstrap began are left alone, and `local` rows stay.
 */
import { ref } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import * as pool from '@nosdesk/core/sync/pool'
import type { SyncAction } from '@nosdesk/core/sync/types'
import type { ModelRow } from './idb'

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
    /** Every key deleted, as `aggregate:id`. */
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
      deleteModel: async (_h: unknown, aggregate: string, id: string) => {
        store.deleted.push(`${aggregate}:${id}`)
      },
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
      setGroupWatermarks: async (h: { name: string }, groups: string[], xid8: number) => {
        const current = { ...((meta(h).get('group_watermarks') as Record<string, number>) ?? {}) }
        for (const g of groups) current[g] = xid8
        meta(h).set('group_watermarks', current)
      },
      replaceGroupWatermarks: set('group_watermarks'),
      setModelVersionsTag: set('model_versions_tag'),
      getModelVersionsTag: get('model_versions_tag'),
    },
  }
})

/** The server: the rows each group's snapshot carries, and a hook that runs
 * once a bootstrap request is in flight. */
const server = vi.hoisted(() => ({
  rows: {} as Record<string, Array<Record<string, unknown>>>,
  duringBootstrap: null as null | (() => void),
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
      const during = server.duringBootstrap
      server.duringBootstrap = null
      during?.()
      const end = { last_xid8: 10, last_sync_id: 100 }
      const lines = [
        { __meta__: { server_schema: 'h', ...end, groups_granted: groups } },
        ...groups.flatMap((g) => server.rows[g] ?? []),
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
const DB = 'test:u1:h:acme'

type Aggregate = 'ticket' | 'comment' | 'documentation_page' | 'cycle'

const owners = (aggregate: Aggregate, id: number) => [...pool.ownersOf(aggregate, id)].sort()

function cached(
  aggregate: Aggregate,
  id: number,
  data: Record<string, unknown>,
  rowOwners?: string[],
): ModelRow {
  return { aggregate, id: String(id), schema_version: 1, data: { id, ...data }, owners: rowOwners }
}

function watermarks(marks: Record<string, number>): void {
  fake.dbs.get(DB)!.set('group_watermarks', marks)
}

function action(aggregate: Aggregate, data: Record<string, unknown>, groups: string[]): SyncAction {
  return {
    sync_id: 1,
    aggregate,
    aggregate_id: String(data.id),
    op: 'I',
    event_type: `${aggregate}.created`,
    schema_version: 1,
    data,
    groups,
  }
}

beforeEach(() => {
  fake.dbs.clear()
  fake.dbs.set(DB, new Map<string, unknown>([['schema_hash', 'h']]))
  fake.store.cached = []
  fake.store.put = []
  fake.store.deleted = []
  server.rows = {}
  server.duringBootstrap = null
})

afterEach(async () => {
  await tearDown()
})

describe('a workspace snapshot', () => {
  it('removes a ticket deleted while the stream was down, from the pool and the cache', async () => {
    watermarks({ [WORKSPACE]: 5 })
    fake.store.cached = [
      cached('ticket', 1, { title: 'Kept' }, [WORKSPACE]),
      cached('ticket', 3, { title: 'Deleted while away' }, [WORKSPACE]),
      cached('documentation_page', 5, { title: 'Deleted while away' }, [WORKSPACE]),
    ]
    server.rows[WORKSPACE] = [{ __model__: 'ticket', id: 1, title: 'Kept' }]
    await hydrate('u1', 'h', '', 'acme', '')
    await subscribe(WORKSPACE)

    expect(pool.has('ticket', 1)).toBe(true)
    expect(pool.has('ticket', 3)).toBe(false)
    expect(pool.has('documentation_page', 5)).toBe(false)
    expect(fake.store.deleted).toEqual(expect.arrayContaining(['ticket:3', 'documentation_page:5']))
    expect(fake.store.deleted).not.toContain('ticket:1')
  })

  it("removes a deleted ticket that last session's torn ticket group also owned, at the same end", async () => {
    // Last session had ticket 3 open: its watermark survives into this launch,
    // and the cached ticket is owned by both groups.
    watermarks({ [WORKSPACE]: 5, 'ticket:3': 5 })
    fake.store.cached = [cached('ticket', 3, { title: 'Deleted while away' }, [WORKSPACE, 'ticket:3'])]
    server.rows[WORKSPACE] = []
    await hydrate('u1', 'h', '', 'acme', '')
    expect(owners('ticket', 3)).toEqual([WORKSPACE, 'ticket:3'].sort())
    await subscribe(WORKSPACE)

    expect(pool.has('ticket', 3)).toBe(false)
    // Gone from the cache too, not left for a later hydrate.
    expect(fake.store.deleted).toContain('ticket:3')
    expect(fake.dbs.get(DB)!.get('group_watermarks')).toEqual({ [WORKSPACE]: 10 })
  })

  it("removes the comments of a ticket that isn't subscribed", async () => {
    watermarks({ [WORKSPACE]: 5, 'ticket:5': 5 })
    fake.store.cached = [cached('comment', 41, { ticket_id: 5 }, ['ticket:5'])]
    server.rows[WORKSPACE] = []
    await hydrate('u1', 'h', '', 'acme', '')
    expect(pool.has('comment', 41)).toBe(true)
    await subscribe(WORKSPACE)

    expect(pool.has('comment', 41)).toBe(false)
  })

  it('keeps a comment its ticket group also owns, and caches the owners it has left', async () => {
    // A two-group bootstrap stamps every row with both groups.
    server.rows[WORKSPACE] = [{ __model__: 'ticket', id: 9, title: 'Printer' }]
    server.rows['ticket:9'] = [{ __model__: 'comment', id: 40, ticket_id: 9 }]
    await hydrate('u1', 'h', '', 'acme', '')
    pool.subscribe(WORKSPACE)
    pool.subscribe('ticket:9')
    await runBootstrap([WORKSPACE, 'ticket:9'])
    expect(owners('comment', 40)).toEqual([WORKSPACE, 'ticket:9'].sort())

    // The workspace snapshot alone never carries comments.
    fake.store.put = []
    await runBootstrap([WORKSPACE])
    expect(pool.has('comment', 40)).toBe(true)
    expect(owners('comment', 40)).toEqual(['ticket:9'])
    expect(fake.store.put.find((r) => r.aggregate === 'comment')?.owners).toEqual(['ticket:9'])
  })

  it('replaces `legacy` on the rows it carries and removes the legacy rows it does not', async () => {
    watermarks({ [WORKSPACE]: 5 })
    fake.store.cached = [
      cached('ticket', 1, { title: 'Cached by 1.1' }),
      cached('comment', 40, { ticket_id: 1 }),
    ]
    server.rows[WORKSPACE] = [{ __model__: 'ticket', id: 1, title: 'Cached by 1.1' }]
    await hydrate('u1', 'h', '', 'acme', '')
    expect(owners('ticket', 1)).toEqual([pool.LEGACY_OWNER])
    await subscribe(WORKSPACE)

    expect(owners('ticket', 1)).toEqual([WORKSPACE])
    expect(fake.store.put.find((r) => r.aggregate === 'ticket')?.owners).toEqual([WORKSPACE])
    expect(pool.has('comment', 40)).toBe(false)
    expect(fake.store.deleted).toContain('comment:40')
  })

  it('overwrites a stale value and keeps the fields only REST sent', async () => {
    await hydrate('u1', 'h', '', 'acme', '')
    pool.upsert('ticket', 9, { id: 9, title: 'Stale', requester_avatar: 'from-rest.png' })
    server.rows[WORKSPACE] = [{ __model__: 'ticket', id: 9, title: 'Fresh' }]
    await subscribe(WORKSPACE)

    expect(pool.get('ticket', 9)).toMatchObject({ title: 'Fresh', requester_avatar: 'from-rest.png' })
    expect(owners('ticket', 9)).toEqual([WORKSPACE])
  })
})

describe('the strip', () => {
  it('keeps a row only `local` owns, and strips one a stream has reached', async () => {
    await hydrate('u1', 'h', '', 'acme', '')
    await subscribe(WORKSPACE)
    // Created by REST, no stream behind it yet.
    pool.upsert('ticket', 50, { id: 50, title: 'Just created' })
    // Created by REST, then reached by the workspace stream: no longer local.
    pool.upsert('ticket', 51, { id: 51, title: 'Echoed' })
    applySseFrame([action('ticket', { id: 51, title: 'Echoed' }, [WORKSPACE])], 11, 110)

    await runBootstrap([WORKSPACE])
    expect(owners('ticket', 50)).toEqual([pool.LOCAL_OWNER])
    expect(pool.has('ticket', 51)).toBe(false)
  })

  it('leaves alone a row a live action wrote after the bootstrap began', async () => {
    await hydrate('u1', 'h', '', 'acme', '')
    await subscribe(WORKSPACE)
    applySseFrame([action('ticket', { id: 60, title: 'Touched before' }, [WORKSPACE])], 11, 110)
    server.duringBootstrap = () =>
      applySseFrame([action('ticket', { id: 61, title: 'Touched during' }, [WORKSPACE])], 12, 120)

    await runBootstrap([WORKSPACE])
    expect(pool.has('ticket', 60)).toBe(false)
    expect(pool.has('ticket', 61)).toBe(true)
    expect(owners('ticket', 61)).toEqual([WORKSPACE])
  })

  it("takes only the granted group off another group's rows", async () => {
    await hydrate('u1', 'h', '', 'acme', '')
    await subscribe(WORKSPACE)
    pool.upsert('ticket', 9, { id: 9 }, [WORKSPACE, 'project:7'])
    pool.upsert('ticket', 10, { id: 10 }, ['project:7'])
    pool.subscribe('project:7')

    await runBootstrap(['project:7'])
    expect(owners('ticket', 9)).toEqual([WORKSPACE])
    expect(pool.has('ticket', 10)).toBe(false)
  })

  it('leaves cycles alone: no snapshot carries them', async () => {
    await hydrate('u1', 'h', '', 'acme', '')
    pool.upsert('cycle', 3, { id: 3, name: 'Sprint' }, [WORKSPACE])
    await subscribe(WORKSPACE)
    expect(pool.has('cycle', 3)).toBe(true)
  })
})

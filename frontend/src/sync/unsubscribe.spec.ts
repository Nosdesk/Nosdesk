/**
 * Unsubscribing a group. Its watermark goes, it comes off every pooled row's
 * owners, and the rows it owned alone leave the pool; rows another group owns
 * stay. The cache is left for the next hydrate. A bootstrap for it still in
 * flight stops stamping it, and its end neither strips nor watermarks the
 * group, even when the group was subscribed again meanwhile. The workspace
 * group is never unsubscribed.
 */
import { effectScope, ref } from 'vue'
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
  type Row = { aggregate: string; id: string; owners?: string[] }
  const store = {
    /** The cache as the next hydrate reads it, by `aggregate:id`. */
    rows: new Map<string, Row>(),
    /** Every row written, in order. */
    put: [] as Row[],
    /** Every key deleted, as `aggregate:id`. */
    deleted: [] as string[],
  }
  const drop = (keys: Array<[string, string]>) => {
    for (const [a, id] of keys) {
      store.rows.delete(`${a}:${id}`)
      store.deleted.push(`${a}:${id}`)
    }
  }
  return {
    dbs,
    store,
    /** Runs while a resync's wipe is in flight. */
    onWipe: null as null | (() => void),
    idb: {
      CACHE_GENERATION: 2,
      open: async (user: string, schema: string, slug: string | null) => ({
        db: { close() {} },
        name: `test:${user}:${schema}:${slug}`,
      }),
      wipe: async (name: string) => {
        dbs.delete(name)
        const during = fake.onWipe
        fake.onWipe = null
        during?.()
      },
      putModels: async (_h: unknown, rows: Row[]) => {
        store.put.push(...rows)
        for (const r of rows) store.rows.set(`${r.aggregate}:${r.id}`, r)
      },
      loadModels: async () => [...store.rows.values()],
      deleteModel: async (_h: unknown, aggregate: string, id: string) => drop([[aggregate, id]]),
      deleteModels: async (_h: unknown, keys: Array<[string, string]>) => drop(keys),
      deleteModelsOwnedOnlyBy: async (
        _h: unknown,
        keys: Array<[string, string]>,
        groups: ReadonlySet<string>,
      ) => {
        drop(
          keys.filter(([a, id]) => {
            const owners = store.rows.get(`${a}:${id}`)?.owners
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

/** A point in a snapshot stream where it waits for `release()`. */
const HOLD = { __hold__: true } as const

/**
 * The server: the lines each group's snapshot carries (HOLD pauses the stream
 * there), every request it served, and the xid8 the next snapshot ends at.
 */
const server = vi.hoisted(() => ({
  rows: {} as Record<string, Array<Record<string, unknown>>>,
  requests: [] as string[],
  xid8: 10,
  held: null as null | (() => void),
  /** The next delta answers `resync_required`. */
  resync: false,
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
    if (url.startsWith('/sync/bootstrap')) {
      const end = { last_xid8: server.xid8++, last_sync_id: 100 }
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
      const resync_required = server.resync
      server.resync = false
      return new Response(
        JSON.stringify({
          actions: [],
          last_xid8: 10,
          last_sync_id: 100,
          has_more: false,
          resync_required,
        }),
      )
    }
    return new Response(null, { status: 404 })
  }),
}))

const { hydrate, subscribe, unsubscribe, runBootstrap, pullDelta, applySseFrame, tearDown } =
  await import('./lifecycle')
const { useGroupSubscription, GROUP_RELEASE_GRACE_MS } = await import('./useGroupSubscription')

const WORKSPACE = 'workspace:6'
const TICKET = 'ticket:9'
const PROJECT = 'project:7'
const DB = 'test:u1:h:acme'

type Aggregate = 'ticket' | 'comment'

const owners = (aggregate: Aggregate, id: number) => [...pool.ownersOf(aggregate, id)].sort()
/** Each group's watermark xid8. */
const marks = () => {
  const stored = fake.dbs.get(DB)!.get('group_watermarks') as
    | Record<string, { xid8: number } | number>
    | undefined
  if (!stored) return undefined
  return Object.fromEntries(
    Object.entries(stored).map(([g, m]) => [g, typeof m === 'number' ? m : m.xid8]),
  )
}

/** Resolves once the stream reaches a HOLD; `release` lets it go on. */
async function reachHold(): Promise<() => void> {
  await vi.waitFor(() => {
    if (!server.held) throw new Error('stream not held yet')
  })
  const release = server.held!
  server.held = null
  return release
}

beforeEach(async () => {
  fake.dbs.clear()
  fake.dbs.set(DB, new Map<string, unknown>([['schema_hash', 'h']]))
  fake.store.rows.clear()
  fake.store.put = []
  fake.store.deleted = []
  server.rows = {
    [WORKSPACE]: [{ __model__: 'ticket', id: 9, title: 'Printer' }],
    [TICKET]: [
      { __model__: 'ticket', id: 9, title: 'Printer' },
      { __model__: 'comment', id: 40, ticket_id: 9 },
    ],
  }
  server.requests = []
  server.xid8 = 10
  server.held = null
  await hydrate('u1', 'h', '', 'acme', '')
  await subscribe(WORKSPACE)
})

afterEach(async () => {
  vi.useRealTimers()
  await tearDown()
})

describe('unsubscribe', () => {
  it("drops the group's watermark and keeps the others", async () => {
    await subscribe(TICKET)
    expect(Object.keys(marks()!).sort()).toEqual([WORKSPACE, TICKET].sort())

    await unsubscribe(TICKET)
    expect(Object.keys(marks()!)).toEqual([WORKSPACE])
    expect(pool.getSubscribedGroups().has(TICKET)).toBe(false)
  })

  it('removes the rows only the group owned, from the pool and from the cache', async () => {
    await subscribe(TICKET)
    expect(owners('comment', 40)).toEqual([TICKET])

    await unsubscribe(TICKET)
    expect(pool.has('comment', 40)).toBe(false)
    expect(fake.store.rows.has('comment:40')).toBe(false)
  })

  it('keeps a row another group owns, without rewriting its cached copy', async () => {
    await subscribe(TICKET)
    expect(owners('ticket', 9)).toEqual([WORKSPACE, TICKET].sort())
    fake.store.put = []

    await unsubscribe(TICKET)
    expect(pool.get('ticket', 9)).toMatchObject({ title: 'Printer' })
    expect(owners('ticket', 9)).toEqual([WORKSPACE])
    expect(fake.store.put).toEqual([])
    expect(fake.store.rows.get('ticket:9')?.owners?.sort()).toEqual([WORKSPACE, TICKET].sort())
  })

  it('keeps a cached row another group also owns there', async () => {
    await subscribe(TICKET)
    // Cached since under another group too (another tab's project board).
    fake.store.rows.set('comment:40', { aggregate: 'comment', id: '40', owners: [TICKET, PROJECT] })

    await unsubscribe(TICKET)
    expect(pool.has('comment', 40)).toBe(false)
    expect(fake.store.rows.get('comment:40')?.owners).toEqual([TICKET, PROJECT])
  })

  it('keeps a row another tab cached under its own group', async () => {
    await subscribe(TICKET)
    fake.store.rows.set('comment:40', { aggregate: 'comment', id: '40', owners: [PROJECT] })

    await unsubscribe(TICKET)
    expect(pool.has('comment', 40)).toBe(false)
    expect(fake.store.rows.get('comment:40')?.owners).toEqual([PROJECT])
  })

  it('does not bring back a row deleted while the group was let go, after it is taken again', async () => {
    await subscribe(TICKET)
    await unsubscribe(TICKET)
    // Comment 40 is deleted while nothing here follows ticket 9.
    server.rows[TICKET] = [{ __model__: 'ticket', id: 9, title: 'Printer' }]
    await subscribe(TICKET)
    expect(marks()![TICKET]).toBeDefined()
    expect(pool.has('comment', 40)).toBe(false)

    // The next launch hydrates from the cache.
    await tearDown()
    await hydrate('u1', 'h', '', 'acme', '')
    expect(pool.has('ticket', 9)).toBe(true)
    expect(pool.has('comment', 40)).toBe(false)
  })

  it('leaves the group out of the next delta request', async () => {
    await subscribe(TICKET)
    await unsubscribe(TICKET)
    server.requests = []

    await pullDelta()
    const delta = server.requests.find((u) => u.startsWith('/sync/delta'))!
    const groups = new URL(delta, 'http://x').searchParams.get('groups')!.split(',')
    expect(groups).toEqual([WORKSPACE])
  })

  it('re-streams a group taken again before its watermark drop has landed', async () => {
    await subscribe(TICKET)
    server.requests = []

    const leaving = unsubscribe(TICKET)
    await subscribe(TICKET)
    await leaving

    expect(server.requests.some((u) => u.startsWith('/sync/bootstrap'))).toBe(true)
    expect(owners('comment', 40)).toEqual([TICKET])
    expect(marks()![TICKET]).toBe(12)
  })

  it('never unsubscribes the workspace group', async () => {
    await unsubscribe(WORKSPACE)
    expect(pool.getSubscribedGroups().has(WORKSPACE)).toBe(true)
    expect(Object.keys(marks()!)).toEqual([WORKSPACE])
    expect(owners('ticket', 9)).toEqual([WORKSPACE])
  })
})

describe('a bootstrap that ends after its group was unsubscribed', () => {
  it('writes no watermark and leaves no rows behind', async () => {
    server.rows[TICKET] = [
      { __model__: 'comment', id: 40, ticket_id: 9 },
      HOLD,
      { __model__: 'comment', id: 41, ticket_id: 9 },
    ]
    const opening = subscribe(TICKET)
    const release = await reachHold()
    expect(pool.has('comment', 40)).toBe(true)

    await unsubscribe(TICKET)
    release()
    await opening

    expect(pool.has('comment', 40)).toBe(false)
    expect(pool.has('comment', 41)).toBe(false)
    expect(marks()).toEqual({ [WORKSPACE]: 10 })
    // The row it streamed before the unsubscribe is not cached afterwards.
    expect(fake.store.put.some((r) => r.aggregate === 'comment')).toBe(false)
  })

  it('does not stream a group let go while the caller was still partitioning', async () => {
    server.requests = []
    const opening = subscribe(TICKET)
    // The group goes before the bootstrap request is made.
    const leaving = unsubscribe(TICKET)
    await Promise.all([opening, leaving])

    expect(server.requests.some((u) => u.startsWith('/sync/bootstrap'))).toBe(false)
    expect(pool.has('comment', 40)).toBe(false)
    expect(owners('ticket', 9)).toEqual([WORKSPACE])
    expect(marks()).toEqual({ [WORKSPACE]: 10 })
  })

  it('caches a row as the pool holds it when the batch is written', async () => {
    server.rows[TICKET] = [{ __model__: 'ticket', id: 9, title: 'Printer' }, HOLD]
    const opening = subscribe(TICKET)
    const release = await reachHold()
    applySseFrame(
      [
        {
          sync_id: 101,
          aggregate: 'ticket',
          aggregate_id: '9',
          op: 'U',
          event_type: 'ticket.updated',
          schema_version: 1,
          data: { id: 9, title: 'Printer jammed' },
          groups: [WORKSPACE, TICKET],
        },
      ],
      11,
      101,
    )
    release()
    await opening

    const cachedTicket = fake.store.put.filter((r) => r.aggregate === 'ticket').at(-1) as
      | { data: Record<string, unknown> }
      | undefined
    expect(cachedTicket?.data.title).toBe('Printer jammed')
  })

  it('applies nothing when the group went before the stream began', async () => {
    pool.subscribe(TICKET)
    const streaming = runBootstrap([TICKET])
    await unsubscribe(TICKET)
    await streaming

    expect(pool.has('comment', 40)).toBe(false)
    expect(owners('ticket', 9)).toEqual([WORKSPACE])
    expect(marks()).toEqual({ [WORKSPACE]: 10 })
  })

  it('stamps only the groups still subscribed on the rows a shared stream carries on', async () => {
    server.rows[PROJECT] = [HOLD, { __model__: 'ticket', id: 12, title: 'Scanner' }]
    server.rows[TICKET] = [{ __model__: 'comment', id: 40, ticket_id: 9 }]
    pool.subscribe(PROJECT)
    pool.subscribe(TICKET)
    const streaming = runBootstrap([PROJECT, TICKET])
    const release = await reachHold()

    await unsubscribe(TICKET)
    release()
    await streaming

    expect(owners('ticket', 12)).toEqual([PROJECT])
    // Over-stamped like any row of a multi-group snapshot, by the group still
    // subscribed alone; that group's next snapshot strips it.
    expect(owners('comment', 40)).toEqual([PROJECT])
    expect(Object.keys(marks()!).sort()).toEqual([WORKSPACE, PROJECT].sort())
  })

  it('strips nothing and watermarks nothing when the group was subscribed again meanwhile', async () => {
    // The first snapshot of ticket 9 carries comment 40, then stalls.
    server.rows[TICKET] = [{ __model__: 'comment', id: 40, ticket_id: 9 }, HOLD]
    const first = subscribe(TICKET)
    const release = await reachHold()

    // Left and opened again. By now comment 40 was deleted and 41 posted, and
    // the second snapshot (ending at xid8 12) completes first.
    await unsubscribe(TICKET)
    server.rows[TICKET] = [{ __model__: 'comment', id: 41, ticket_id: 9 }]
    await subscribe(TICKET)
    expect(marks()![TICKET]).toBe(12)

    release()
    await first

    // The first stream's end did not take its group off comment 41, which it
    // never carried, nor put its older watermark back.
    expect(owners('comment', 41)).toEqual([TICKET])
    expect(pool.has('comment', 40)).toBe(false)
    expect(marks()![TICKET]).toBe(12)
  })
})

describe('the grace timer', () => {
  /** Hold the ticket group from a view, then leave it. */
  async function holdAndLeave(): Promise<void> {
    const scope = effectScope()
    const held = scope.run(() => useGroupSubscription(TICKET))!
    await held.settled()
    scope.stop()
  }

  it('unsubscribes the group once the grace runs out, and its watermark goes', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
    await holdAndLeave()
    expect(pool.has('comment', 40)).toBe(true)
    expect(marks()![TICKET]).toBe(11)

    vi.advanceTimersByTime(GROUP_RELEASE_GRACE_MS)
    expect(pool.getSubscribedGroups().has(TICKET)).toBe(false)
    expect(pool.has('comment', 40)).toBe(false)
    await vi.waitFor(() => expect(marks()).toEqual({ [WORKSPACE]: 10 }))
  })

  it('does not unsubscribe the next workspace after a tear-down', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
    await holdAndLeave()
    expect(vi.getTimerCount()).toBe(1)

    await tearDown()
    expect(vi.getTimerCount()).toBe(0)
    await hydrate('u1', 'h', '', 'acme', '')
    await subscribe(WORKSPACE)
    await subscribe(TICKET)

    vi.advanceTimersByTime(GROUP_RELEASE_GRACE_MS)
    expect(pool.getSubscribedGroups().has(TICKET)).toBe(true)
  })
})

describe('a resync', () => {
  it('brings back the groups subscribed when its wipe ends, not when it began', async () => {
    await subscribe(TICKET)
    server.rows[PROJECT] = [{ __model__: 'ticket', id: 12, title: 'Scanner' }]
    server.requests = []
    server.resync = true
    // During the wipe, a view lets the ticket go and another takes the project.
    fake.onWipe = () => {
      void unsubscribe(TICKET)
      pool.subscribe(PROJECT)
    }

    await pullDelta()
    expect([...pool.getSubscribedGroups()].sort()).toEqual([WORKSPACE, PROJECT].sort())
    const bootstrap = server.requests.find((u) => u.startsWith('/sync/bootstrap'))!
    const groups = new URL(bootstrap, 'http://x').searchParams.get('groups')!.split(',')
    expect(groups.sort()).toEqual([WORKSPACE, PROJECT].sort())
    expect(pool.has('comment', 40)).toBe(false)
    expect(owners('ticket', 12)).toEqual([WORKSPACE, PROJECT].sort())
  })
})

describe('the torn prune', () => {
  it('drops the cached rows the torn group alone owned, and keeps the shared ones', async () => {
    await tearDown()
    // Last session had ticket 3 open: its watermark survives into this launch.
    fake.dbs.get(DB)!.set('group_watermarks', { [WORKSPACE]: 5, 'ticket:3': 5 })
    fake.store.rows.set('comment:41', {
      aggregate: 'comment',
      id: '41',
      schema_version: 1,
      data: { id: 41, ticket_id: 3 },
      owners: ['ticket:3'],
    } as never)
    fake.store.rows.set('comment:42', {
      aggregate: 'comment',
      id: '42',
      schema_version: 1,
      data: { id: 42, ticket_id: 3 },
      // Also cached by another tab under a group this one never held.
      owners: ['ticket:3', 'project:8'],
    } as never)
    await hydrate('u1', 'h', '', 'acme', '')
    expect(pool.has('comment', 41)).toBe(true)

    // Only the workspace is subscribed when this session first moves the cursor.
    await subscribe(WORKSPACE)
    expect(pool.has('comment', 41)).toBe(false)
    expect(fake.store.rows.has('comment:41')).toBe(false)
    expect(pool.has('comment', 42)).toBe(false)
    expect(fake.store.rows.get('comment:42')?.owners).toEqual(['ticket:3', 'project:8'])
  })
})

/**
 * Workspace-scoped query keys: each workspace keeps its own cache entries,
 * the scoped cache only reaches the active workspace, and a live query moves
 * with a switch. Nothing runs or is cached before the workspace is known.
 */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h, ref, type Ref } from 'vue'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createPinia } from 'pinia'
import { PiniaColada, useQueryCache, type QueryCache } from '@pinia/colada'
import { activeWorkspaceSlug, setActiveWorkspaceSlug } from '@/services/activeWorkspace'
import {
  useWorkspaceInfiniteQuery,
  useWorkspaceQuery,
  useWorkspaceQueryCache,
  workspaceKey,
  type WorkspaceQueryCache,
} from '@/queries/workspaceQuery'

const config = vi.hoisted(() => ({
  routing: 'host' as 'host' | 'path',
  resolved: null as unknown as Ref<boolean>,
}))
vi.mock('@nosdesk/core/services/instanceConfig', async (importOriginal) => {
  const { ref } = await import('vue')
  config.resolved = ref(true)
  return {
    ...(await importOriginal<typeof import('@nosdesk/core/services/instanceConfig')>()),
    instanceConfigResolvedRef: config.resolved,
    getWorkspaceRouting: () => config.routing,
  }
})

/** Set the instance config the workspace-ready gate reads. */
function instanceConfig(routing: 'host' | 'path', resolved = true) {
  config.routing = routing
  // The gate reads routing without tracking it; a write to `resolved` re-runs it.
  config.resolved.value = !resolved
  config.resolved.value = resolved
}

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  setActiveWorkspaceSlug(null)
  instanceConfig('host')
})

/** Mount `setup` under a fresh Pinia + Colada; returns both caches. */
function host(setup: () => void = () => {}) {
  let raw!: QueryCache
  let scoped!: WorkspaceQueryCache
  const Host = defineComponent({
    setup() {
      setup()
      raw = useQueryCache()
      scoped = useWorkspaceQueryCache()
      return () => h('div')
    },
  })
  wrapper = mount(Host, { global: { plugins: [createPinia(), PiniaColada] } })
  return { raw, scoped }
}

/** A ticket list query whose fetcher answers with the workspace it ran in. */
function ticketsQuery() {
  const fetched: (string | null)[] = []
  const query = vi.fn(async () => {
    const slug = activeWorkspaceSlug()
    fetched.push(slug)
    return `tickets of ${slug ?? 'host'}`
  })
  return { fetched, query, setup: () => useWorkspaceQuery({ key: ['tickets'], query }) }
}

describe('workspaceKey', () => {
  it('prefixes the key with the active workspace', () => {
    setActiveWorkspaceSlug('acme')
    expect(workspaceKey('tickets', 7)).toEqual(['ws', 'acme', 'tickets', 7])
    expect(workspaceKey()).toEqual(['ws', 'acme'])
  })

  it("uses 'host' in host mode, where there is no slug", () => {
    expect(workspaceKey('tickets')).toEqual(['ws', 'host', 'tickets'])
  })
})

describe('useWorkspaceQuery', () => {
  it('keeps a separate cache entry per workspace', async () => {
    setActiveWorkspaceSlug('acme')
    const { raw, scoped } = host()
    scoped.setQueryData(['tickets'], 'acme list')
    setActiveWorkspaceSlug('globex')
    scoped.setQueryData(['tickets'], 'globex list')

    expect(scoped.getQueryData(['tickets'])).toBe('globex list')
    setActiveWorkspaceSlug('acme')
    expect(scoped.getQueryData(['tickets'])).toBe('acme list')
    expect(raw.getQueryData(['ws', 'globex', 'tickets'])).toBe('globex list')
    expect(raw.getQueryData(['tickets'])).toBeUndefined()
  })

  it('follows a workspace switch with the key, without serving the old entry', async () => {
    setActiveWorkspaceSlug('acme')
    const tickets = ticketsQuery()
    let data!: ReturnType<typeof tickets.setup>['data']
    const { raw } = host(() => {
      data = tickets.setup().data
    })
    await flushPromises()
    expect(data.value).toBe('tickets of acme')

    setActiveWorkspaceSlug('globex')
    await flushPromises()
    expect(data.value).toBe('tickets of globex')
    expect(tickets.fetched).toEqual(['acme', 'globex'])
    expect(raw.getQueryData(['ws', 'acme', 'tickets'])).toBe('tickets of acme')
    expect(raw.getQueryData(['ws', 'globex', 'tickets'])).toBe('tickets of globex')
  })

  it("works in host mode under the 'host' key", async () => {
    const tickets = ticketsQuery()
    const { raw } = host(tickets.setup)
    await flushPromises()
    expect(raw.getQueryData(['ws', 'host', 'tickets'])).toBe('tickets of host')
  })

  it('scopes an infinite query, options getter included', async () => {
    setActiveWorkspaceSlug('acme')
    const { raw } = host(() => {
      useWorkspaceInfiniteQuery(() => ({
        key: ['audit'],
        initialPageParam: 1,
        query: async () => ['entry'],
        getNextPageParam: () => null,
      }))
    })
    await flushPromises()
    expect(raw.getEntries({ key: ['ws', 'acme', 'audit'] })).toHaveLength(1)
    expect(raw.getEntries({ key: ['audit'] })).toHaveLength(0)
  })
})

describe('before the workspace is known', () => {
  it('waits for the slug in path mode, then fetches once under it', async () => {
    instanceConfig('path')
    const tickets = ticketsQuery()
    const { raw } = host(tickets.setup)
    await flushPromises()
    expect(tickets.fetched).toEqual([])
    expect(raw.getEntries({ key: ['ws', 'host'] }).filter((e) => e.when > 0)).toHaveLength(0)

    setActiveWorkspaceSlug('acme')
    await flushPromises()
    expect(tickets.fetched).toEqual(['acme'])
    expect(raw.getQueryData(['ws', 'acme', 'tickets'])).toBe('tickets of acme')
  })

  it('waits for the instance config in host mode', async () => {
    instanceConfig('host', false)
    const tickets = ticketsQuery()
    host(tickets.setup)
    await flushPromises()
    expect(tickets.fetched).toEqual([])

    instanceConfig('host')
    await flushPromises()
    expect(tickets.fetched).toEqual([null])
  })

  it('fetches nothing between the old workspace and the new one in a switch', async () => {
    instanceConfig('path')
    setActiveWorkspaceSlug('acme')
    const tickets = ticketsQuery()
    const { raw } = host(tickets.setup)
    await flushPromises()

    setActiveWorkspaceSlug(null)
    await flushPromises()
    setActiveWorkspaceSlug('globex')
    await flushPromises()
    expect(tickets.fetched).toEqual(['acme', 'globex'])
    expect(raw.getEntries({ key: ['ws', 'host'] }).filter((e) => e.when > 0)).toHaveLength(0)
  })

  it("keeps the caller's enabled once the workspace is known", async () => {
    instanceConfig('path')
    setActiveWorkspaceSlug('acme')
    const query = vi.fn(async () => 'list')
    const enabled = ref(false)
    host(() => {
      useWorkspaceQuery({ key: ['tickets'], query, enabled })
    })
    await flushPromises()
    expect(query).not.toHaveBeenCalled()

    enabled.value = true
    await flushPromises()
    expect(query).toHaveBeenCalledTimes(1)
  })

  it('holds an infinite query from an options getter until the slug is set', async () => {
    instanceConfig('path')
    const query = vi.fn(async () => ['entry'])
    const { raw } = host(() => {
      useWorkspaceInfiniteQuery(() => ({
        key: ['audit'],
        initialPageParam: 1,
        query,
        getNextPageParam: () => null,
      }))
    })
    await flushPromises()
    expect(query).not.toHaveBeenCalled()

    setActiveWorkspaceSlug('acme')
    await flushPromises()
    expect(query).toHaveBeenCalledTimes(1)
    expect(raw.getEntries({ key: ['ws', 'acme', 'audit'] })).toHaveLength(1)
  })

  it('drops cache writes and finds nothing on reads', async () => {
    instanceConfig('path')
    const { raw, scoped } = host()
    const invalidate = vi.spyOn(raw, 'invalidateQueries')
    const cancel = vi.spyOn(raw, 'cancelQueries')
    scoped.setQueryData(['tickets'], 'list')
    await scoped.invalidateQueries({ key: ['tickets'] })
    scoped.cancelQueries({ key: ['tickets'] })
    expect(scoped.getQueryData(['tickets'])).toBeUndefined()
    expect(raw.getQueryData(['ws', 'host', 'tickets'])).toBeUndefined()
    expect(invalidate).not.toHaveBeenCalled()
    expect(cancel).not.toHaveBeenCalled()
  })
})

describe('useWorkspaceQueryCache', () => {
  it('invalidates only the active workspace', async () => {
    setActiveWorkspaceSlug('acme')
    const tickets = ticketsQuery()
    const { raw, scoped } = host(tickets.setup)
    await flushPromises()
    setActiveWorkspaceSlug('globex')
    await flushPromises()
    raw.setQueryData(['my-workspaces', 'u1'], ['acme', 'globex'])
    expect(tickets.fetched).toEqual(['acme', 'globex'])

    await scoped.invalidateQueries({ key: ['tickets'] })
    await flushPromises()
    // The active entry refetched; acme's entry was not touched.
    expect(tickets.fetched).toEqual(['acme', 'globex', 'globex'])
    const [acme] = raw.getEntries({ key: ['ws', 'acme', 'tickets'], exact: true })
    expect(acme?.stale).toBe(false)

    // No key: everything in the active workspace, and nothing else.
    await scoped.invalidateQueries()
    await flushPromises()
    expect(tickets.fetched).toEqual(['acme', 'globex', 'globex', 'globex'])
    expect(raw.getEntries({ key: ['ws', 'acme', 'tickets'], exact: true })[0]?.stale).toBe(false)
    expect(raw.getEntries({ key: ['my-workspaces'] })[0]?.when).toBeGreaterThan(0)
  })

  it('reads the workspace at each call, not when it was set up', () => {
    setActiveWorkspaceSlug('acme')
    const { raw, scoped } = host()
    setActiveWorkspaceSlug('globex')
    scoped.setQueryData(['tickets'], 'globex list')
    expect(raw.getQueryData(['ws', 'globex', 'tickets'])).toBe('globex list')
    expect(raw.getQueryData(['ws', 'acme', 'tickets'])).toBeUndefined()
  })

  it('passes the rest of a filter through to the cache', () => {
    setActiveWorkspaceSlug('acme')
    const { raw, scoped } = host()
    const cancel = vi.spyOn(raw, 'cancelQueries')
    const invalidate = vi.spyOn(raw, 'invalidateQueries')
    scoped.cancelQueries({ key: ['tickets', 7], exact: true }, 'gone')
    void scoped.invalidateQueries({ key: ['tickets'], active: true }, 'all')
    expect(cancel).toHaveBeenCalledWith({ key: ['ws', 'acme', 'tickets', 7], exact: true }, 'gone')
    expect(invalidate).toHaveBeenCalledWith({ key: ['ws', 'acme', 'tickets'], active: true }, 'all')
  })
})

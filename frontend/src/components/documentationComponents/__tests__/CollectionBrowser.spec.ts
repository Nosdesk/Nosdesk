import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import * as pool from '@nosdesk/core/sync/pool'
import type { DocCollectionRow } from '@nosdesk/core/sync/stores/documentation'
import { mountWithProviders } from '@/test/mountWithProviders'

const service = vi.hoisted(() => ({
  getCollections: vi.fn(async () => []),
  reorderCollections: vi.fn(async () => true),
}))
vi.mock('@nosdesk/core/services/collectionService', () => service)
const auth = vi.hoisted(() => ({ isTechnician: false }))
vi.mock('@/stores/auth', () => ({ useAuthStore: () => auth }))

import CollectionBrowser from '@/components/documentationComponents/CollectionBrowser.vue'

const row = (id: number, name: string, extra: Partial<DocCollectionRow> = {}): DocCollectionRow => ({
  id,
  uuid: `c-${id}`,
  name,
  slug: name.toLowerCase(),
  description: null,
  icon: null,
  color: null,
  is_system: false,
  created_by: null,
  display_order: id,
  description_text: null,
  hide_titles_from_non_members: false,
  require_verification: false,
  restricted: false,
  grants: { groups: [], users: [] },
  created_at: '2026-10-01T00:00:00Z',
  updated_at: '2026-10-01T00:00:00Z',
  ...extra,
})

const put = (r: DocCollectionRow) => pool.upsert('documentation_collection', r.id, r)

let wrapper: VueWrapper | null = null
let router: Router

beforeEach(async () => {
  router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:pathMatch(.*)*', component: { template: '<div />' } }],
  })
  await router.push('/documentation')
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  pool.reset()
  vi.clearAllMocks()
  auth.isTechnician = false
})

const items = () => wrapper!.findAll('li')
const names = () =>
  wrapper!
    .findAll('li a')
    .filter((a) => a.attributes('href')?.startsWith('/documentation/collections/'))
    .map((a) => a.text())

async function mountBrowser() {
  wrapper = mountWithProviders(CollectionBrowser, {}, {}, [router])
  await flushPromises()
}

describe('CollectionBrowser', () => {
  it('drops a collection the pool no longer holds, without a reload', async () => {
    put(row(1, 'Alpha'))
    put(row(2, 'Bravo', { restricted: true, grants: { groups: [{ id: 9, name: 'Payroll team' }], users: [] } }))
    await mountBrowser()
    expect(names()).toEqual(['Alpha', 'Bravo'])

    // Losing access deletes the row from the pool (user.access_changed).
    pool.remove('documentation_collection', 2)
    await nextTick()

    expect(names()).toEqual(['Alpha'])
  })

  it('shows a collection new to the pool with who it is open to', async () => {
    put(row(1, 'Alpha'))
    await mountBrowser()

    put(row(3, 'Charlie', { restricted: true, grants: { groups: [{ id: 9, name: 'Payroll team' }], users: [] } }))
    await nextTick()

    expect(names()).toEqual(['Alpha', 'Charlie'])
    expect(items()[1]!.text()).toContain('Payroll team')
    expect(items()[0]!.text()).not.toContain('docs-collection-browser-restricted-badge')
  })

  it('names a person a collection is shared with from the pool', async () => {
    pool.upsert('user', 'u-1', { uuid: 'u-1', name: 'Priya Shah' })
    put(row(1, 'Alpha', { restricted: true, grants: { groups: [], users: ['u-1'] } }))
    put(row(2, 'Bravo', { restricted: true }))
    await mountBrowser()

    expect(items()[0]!.text()).toContain('Priya Shah')
    // Restricted to nobody: admins only.
    expect(items()[1]!.text()).toContain('docs-collection-browser-restricted-badge')
  })

  it('counts the active pages the pool holds for each collection', async () => {
    put(row(1, 'Alpha'))
    for (const [id, status] of [[10, 'published'], [11, 'draft'], [12, 'deleted']] as const) {
      pool.upsert('documentation_page', id, {
        id,
        uuid: `p-${id}`,
        collection_id: 1,
        title: `Page ${id}`,
        slug: `page-${id}`,
        status,
        parent_id: null,
        updated_at: '2026-10-01T00:00:00Z',
      })
    }
    await mountBrowser()

    // Fluent echoes the message id here, so read the count the row passes it.
    const vm = wrapper!.findComponent(CollectionBrowser).vm as unknown as {
      collections: DocCollectionRow[]
      pageCount: (c: DocCollectionRow) => number
    }
    expect(vm.collections.map((c) => vm.pageCount(c))).toEqual([2])
    expect(wrapper!.text()).not.toContain('Page 12')
  })

  it('follows the pool order and never asks REST for the list', async () => {
    put(row(1, 'Alpha', { display_order: 2 }))
    put(row(2, 'Bravo', { display_order: 1 }))
    await mountBrowser()

    expect(names()).toEqual(['Bravo', 'Alpha'])
    expect(service.getCollections).not.toHaveBeenCalled()
  })

  it('reorders at once and puts the order back when the save fails', async () => {
    auth.isTechnician = true
    put(row(1, 'Alpha'))
    put(row(2, 'Bravo'))
    put(row(3, 'Charlie'))
    let settle: (ok: boolean) => void = () => {}
    service.reorderCollections.mockImplementationOnce(
      () => new Promise<boolean>((resolve) => (settle = resolve)),
    )
    await mountBrowser()

    await items()[2]!.trigger('dragstart')
    await items()[0]!.trigger('drop')
    expect(names()).toEqual(['Charlie', 'Alpha', 'Bravo'])
    expect(service.reorderCollections).toHaveBeenCalledWith([
      { collection_id: 3, display_order: 0 },
      { collection_id: 1, display_order: 1 },
      { collection_id: 2, display_order: 2 },
    ])

    settle(false)
    await flushPromises()
    expect(names()).toEqual(['Alpha', 'Bravo', 'Charlie'])
  })
})

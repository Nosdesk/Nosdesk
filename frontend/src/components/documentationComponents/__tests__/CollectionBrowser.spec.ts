import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import * as pool from '@nosdesk/core/sync/pool'
import type { CollectionWithDetails } from '@nosdesk/core/services/collectionService'
import type { DocCollectionRow } from '@nosdesk/core/sync/stores/documentation'
import { mountWithProviders } from '@/test/mountWithProviders'

const service = vi.hoisted(() => ({
  getCollections: vi.fn(),
  reorderCollections: vi.fn(async () => true),
}))
vi.mock('@nosdesk/core/services/collectionService', () => service)
vi.mock('@/stores/auth', () => ({ useAuthStore: () => ({ isTechnician: false }) }))

import CollectionBrowser from '@/components/documentationComponents/CollectionBrowser.vue'

const syncRow = (id: number, name: string, extra: Partial<DocCollectionRow> = {}): DocCollectionRow => ({
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
  created_at: '2026-10-01T00:00:00Z',
  updated_at: '2026-10-01T00:00:00Z',
  ...extra,
})

const restRow = (row: DocCollectionRow, extra: Partial<CollectionWithDetails> = {}): CollectionWithDetails =>
  ({
    ...row,
    description_doc_id: `collection-${row.uuid}`,
    visible_to_groups: [],
    visible_to_users: [],
    is_public: !row.restricted,
    page_count: 0,
    ...extra,
  }) as CollectionWithDetails

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
})

const names = () => wrapper!.findAll('li a').filter((a) => a.attributes('href')?.startsWith('/documentation/collections/')).map((a) => a.text())

async function mountBrowser() {
  wrapper = mountWithProviders(CollectionBrowser, {}, {}, [router])
  await flushPromises()
}

describe('CollectionBrowser', () => {
  it('drops a collection the sync pool no longer holds, without a reload', async () => {
    const a = syncRow(1, 'Alpha')
    const b = syncRow(2, 'Bravo', { restricted: true })
    pool.upsert('documentation_collection', a.id, a)
    pool.upsert('documentation_collection', b.id, b)
    service.getCollections.mockResolvedValue([restRow(a), restRow(b)])

    await mountBrowser()
    expect(names()).toEqual(['Alpha', 'Bravo'])

    // Losing access deletes the row from the pool (user.access_changed).
    pool.remove('documentation_collection', b.id)
    await nextTick()

    expect(names()).toEqual(['Alpha'])
    expect(service.getCollections).toHaveBeenCalledTimes(1)
  })

  it('shows a collection that reaches the pool before REST, then fetches its details', async () => {
    const a = syncRow(1, 'Alpha')
    pool.upsert('documentation_collection', a.id, a)
    service.getCollections.mockResolvedValue([restRow(a)])

    await mountBrowser()
    expect(names()).toEqual(['Alpha'])

    const c = syncRow(3, 'Charlie', { restricted: true })
    service.getCollections.mockResolvedValue([
      restRow(a),
      restRow(c, { visible_to_groups: [{ id: 9, name: 'Payroll team' }], page_count: 4 }),
    ])
    pool.upsert('documentation_collection', c.id, c)
    await nextTick()

    // Renders straight from the sync row, locked, before its details land.
    expect(names()).toEqual(['Alpha', 'Charlie'])
    expect(wrapper!.text()).toContain('docs-collection-browser-restricted-badge')

    await flushPromises()
    expect(service.getCollections).toHaveBeenCalledTimes(2)
    expect(wrapper!.text()).toContain('Payroll team')

    // Settled: no further refetches once every pool row has its details.
    await flushPromises()
    expect(service.getCollections).toHaveBeenCalledTimes(2)
  })

  it('follows the pool order, not the REST order', async () => {
    const a = syncRow(1, 'Alpha', { display_order: 2 })
    const b = syncRow(2, 'Bravo', { display_order: 1 })
    pool.upsert('documentation_collection', a.id, a)
    pool.upsert('documentation_collection', b.id, b)
    service.getCollections.mockResolvedValue([restRow(a), restRow(b)])

    await mountBrowser()
    expect(names()).toEqual(['Bravo', 'Alpha'])
  })
})

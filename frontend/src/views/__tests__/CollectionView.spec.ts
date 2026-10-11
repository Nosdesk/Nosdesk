import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { mountWithProviders } from '@/test/mountWithProviders'

const collections = vi.hoisted(() => ({
  getCollectionBySlug: vi.fn(),
  addPageToCollection: vi.fn(),
  updateCollection: vi.fn(),
  deleteCollection: vi.fn(),
  getPageOverridesInCollection: vi.fn(async () => []),
}))
vi.mock('@nosdesk/core/services/collectionService', () => collections)
const docs = vi.hoisted(() => ({ createArticle: vi.fn() }))
vi.mock('@nosdesk/core/services/documentationService', () => ({ default: docs }))
vi.mock('@nosdesk/core/sync/stores/documentation', () => ({
  useSyncDocsStore: () => ({ allCollections: [], allPages: [] }),
}))
const auth = vi.hoisted(() => ({ isAdmin: false, isTechnician: false }))
vi.mock('@/stores/auth', () => ({ useAuthStore: () => auth }))
vi.mock('@/stores/documentationNav', () => ({ useDocumentationNavStore: () => ({ refreshPages: vi.fn() }) }))
vi.mock('@/composables/useTitleManager', () => ({ useTitleManager: () => ({ setCustomTitle: vi.fn() }) }))
vi.mock('@/components/CollaborativeEditor.vue', () => ({ default: { template: '<div />' } }))
vi.mock('@/components/documentationComponents/CollectionTreeList.vue', () => ({ default: { template: '<div />' } }))

import CollectionView from '@/views/CollectionView.vue'

const COLLECTION = {
  id: 7,
  uuid: 'c-7',
  name: 'Payroll',
  slug: 'payroll',
  description: null,
  icon: null,
  color: null,
  is_system: false,
  description_doc_id: 'ws-1_collection-c-7',
  pages: [],
  visible_to_groups: [],
  visible_to_users: [{ uuid: 'u-1', name: 'Priya Shah' }],
  is_public: false,
  page_count: 0,
}

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
  vi.clearAllMocks()
  auth.isAdmin = false
})

async function mountCollection() {
  collections.getCollectionBySlug.mockResolvedValue(COLLECTION)
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/documentation/collections/:slug', component: { template: '<div />' } },
      { path: '/documentation/:path', component: { template: '<div />' } },
      { path: '/:pathMatch(.*)*', component: { template: '<div />' } },
    ],
  })
  await router.push('/documentation/collections/payroll')
  wrapper = mountWithProviders(CollectionView, {}, {}, [[PiniaColada, {}] as never, router])
  await flushPromises()
}

describe('CollectionView', () => {
  it('creates a new page inside the collection in one request', async () => {
    docs.createArticle.mockResolvedValue({ id: 42, slug: 'new-page', title: 'New page' })
    await mountCollection()

    const create = wrapper!
      .findAll('button')
      .find((b) => b.text().includes('collection-action-new-page'))
    expect(create, 'the new page button').toBeTruthy()
    await create!.trigger('click')
    await flushPromises()

    expect(docs.createArticle).toHaveBeenCalledTimes(1)
    expect(docs.createArticle.mock.calls[0]![0]).toMatchObject({ collection_id: 7 })
    expect(collections.addPageToCollection).not.toHaveBeenCalled()
  })

  it('warns that deleting the collection moves its pages to the trash', async () => {
    auth.isAdmin = true
    await mountCollection()

    const remove = wrapper!
      .findAll('button')
      .find((b) => b.text().includes('collection-action-delete'))
    expect(remove, 'the delete button').toBeTruthy()
    await remove!.trigger('click')
    await flushPromises()

    expect(document.body.textContent).toContain('docs-nav-confirm-delete-collection-message')
    expect(collections.deleteCollection).not.toHaveBeenCalled()
  })
})

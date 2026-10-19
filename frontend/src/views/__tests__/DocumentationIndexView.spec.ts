import { afterEach, describe, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import * as pool from '@nosdesk/core/sync/pool'
import type { DocCollectionRow } from '@nosdesk/core/sync/stores/documentation'
import { mountWithProviders } from '@/test/mountWithProviders'

// The index view's empty state: with no collection and no page, it stands
// alone (no empty Library card above it) and offers New page and New
// collection. New collection opens the band's modal.

const createNewPage = vi.hoisted(() => vi.fn(async () => {}))
vi.mock('@/composables/useDocumentation', () => ({ useDocumentation: () => ({ createNewPage }) }))
vi.mock('@/composables/useTitleManager', () => ({ useTitleManager: () => ({ setCustomTitle: () => {} }) }))
vi.mock('@/composables/usePageCreateAction', () => ({ usePageCreateAction: () => {} }))
vi.mock('@/composables/useKnowledgeGaps', async () => {
  const { ref } = await import('vue')
  return {
    useKnowledgeGaps: () => ({ gaps: ref([]), isLoading: ref(false) }),
    useGapTitle: () => () => '',
  }
})
vi.mock('@/components/common/PullToRefresh.vue', () => ({
  default: defineComponent({ template: '<div />' }),
}))
const auth = vi.hoisted(() => ({ admin: false }))
vi.mock('@/stores/auth', async () => {
  const { ref } = await import('vue')
  return { useAuthStore: () => ({ isTechnician: ref(false), isAdmin: ref(auth.admin) }) }
})

const openCreateCollection = vi.hoisted(() => vi.fn())
vi.mock('@/components/documentationComponents/DocumentationIndexToolbar.vue', () => ({
  default: defineComponent({
    setup(_, { expose }) {
      expose({ openCreateCollection })
      return {}
    },
    template: '<header data-testid="band" />',
  }),
}))

import DocumentationIndexView from '@/views/DocumentationIndexView.vue'

let wrapper: VueWrapper | null = null

async function mountView() {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/admin/data-import/documentation', name: 'admin-docs-import', component: { template: '<div />' } },
      { path: '/:pathMatch(.*)*', name: 'documentation', component: { template: '<div />' } },
    ],
  })
  await router.push('/documentation')
  await router.isReady()
  wrapper = mountWithProviders(DocumentationIndexView, {}, {}, [router])
  await flushPromises()
  return router
}

afterEach(() => {
  auth.admin = false
  wrapper?.unmount()
  wrapper = null
  pool.reset()
  vi.clearAllMocks()
})

describe('DocumentationIndexView empty state', () => {
  it('stands alone and offers New page and New collection', async () => {
    await mountView()
    expect(wrapper!.find('[data-testid="band"]').exists()).toBe(true)
    expect(wrapper!.text()).not.toContain('docs-index-library-heading')

    const empty = wrapper!.find('[data-testid="docs-index-empty"]')
    expect(empty.exists()).toBe(true)
    const buttons = empty.findAll('button')
    expect(buttons.map((b) => b.text())).toEqual(['docs-index-new-page', 'docs-index-toolbar-new-collection'])

    await buttons[0].trigger('click')
    expect(createNewPage).toHaveBeenCalledTimes(1)
    await buttons[1].trigger('click')
    expect(openCreateCollection).toHaveBeenCalledTimes(1)
  })

  it('offers Import documents to an admin only, opening the documentation import', async () => {
    await mountView()
    expect(wrapper!.find('[data-testid="docs-index-empty-import"]').exists()).toBe(false)
    wrapper!.unmount()
    auth.admin = true
    const router = await mountView()
    const button = wrapper!.find('[data-testid="docs-index-empty-import"]')
    expect(button.text()).toBe('docs-index-toolbar-import')
    await button.trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.name).toBe('admin-docs-import')
  })

  it('shows the Library once a collection exists, without a New link in its header', async () => {
    const collection: DocCollectionRow = {
      id: 1,
      uuid: 'c-1',
      name: 'Runbooks',
      slug: 'runbooks',
      description: null,
      icon: null,
      color: null,
      is_system: false,
      created_by: null,
      display_order: 1,
      description_text: null,
      hide_titles_from_non_members: false,
      require_verification: false,
      restricted: false,
      grants: { groups: [], users: [] },
      created_at: '2026-10-01T00:00:00Z',
      updated_at: '2026-10-01T00:00:00Z',
    }
    pool.upsert('documentation_collection', 1, collection)
    await mountView()
    const library = wrapper!.find('section')
    expect(library.text()).toContain('docs-index-library-heading')
    expect(library.find('header button').exists()).toBe(false)
  })
})

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { reactive, ref } from 'vue'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import type { DocCollectionRow, DocPageRow } from '@nosdesk/core/sync/stores/documentation'

// The docs sidebar lights the open collection and starred page, and opens
// the collection holding the open page, with and without the workspace slug
// in the path.

let routing: 'host' | 'path' = 'host'
let slug: string | null = null
vi.mock('@nosdesk/core/services/instanceConfig', () => ({
  fetchInstanceConfig: async () => {},
  getWorkspaceRouting: () => routing,
}))
vi.mock('@/stores/workspaceReset', () => ({
  resetWorkspaceScopedState: async () => {},
  enterWorkspace: async () => {},
}))
vi.mock('@/services/activeWorkspace', () => ({
  beginWorkspaceSwitch: () => {},
  activeWorkspaceSlug: () => slug,
  setActiveWorkspaceSlug: (s: string | null) => {
    slug = s
  },
}))
vi.mock('@/stores/auth', () => ({ useAuthStore: () => ({ isAdmin: true, user: { uuid: 'me' } }) }))
vi.mock('@/sync/useWorkspaceGroup', () => ({ useWorkspaceGroupSubscription: () => ({ ready: ref(true) }) }))

const pool = vi.hoisted(() => ({
  collectionsSorted: [] as unknown[],
  allPages: [] as unknown[],
}))
vi.mock('@nosdesk/core/sync/stores/documentation', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nosdesk/core/sync/stores/documentation')>()),
  useSyncDocsStore: () => pool,
}))
vi.mock('@nosdesk/core/services/documentationService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nosdesk/core/services/documentationService')>()),
  getStarredPages: async () => [{ page_id: 2, slug: 'onboarding', title: 'Onboarding', icon: null }],
}))

import DocumentationNav from '@/components/documentationComponents/DocumentationNav.vue'
import { mountWithProviders } from '@/test/mountWithProviders'
import { makeWorkspaceRouter, routingModes, stubRoute } from '@/test/workspaceRouter'

const routes = [
  stubRoute('/documentation', 'documentation'),
  stubRoute('/documentation/drafts', 'documentation-drafts'),
  stubRoute('/documentation/collections/:slug', 'collection-view'),
  stubRoute('/documentation/:path', 'documentation-page'),
]

const collection = (id: number, name: string): DocCollectionRow =>
  ({ id, uuid: `c${id}`, name, slug: name.toLowerCase(), display_order: id }) as DocCollectionRow

const docPage = (id: number, title: string, collectionId: number | null): DocPageRow =>
  ({
    id,
    uuid: `p${id}`,
    collection_id: collectionId,
    title,
    slug: title.toLowerCase(),
    status: 'published',
    parent_id: null,
    display_order: id,
  }) as DocPageRow

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  localStorage.clear()
})

async function mountAt(path: string) {
  const router = makeWorkspaceRouter(routes)
  await router.push(path)
  wrapper = mountWithProviders(DocumentationNav, {}, {}, [router])
  await flushPromises()
  return router
}

const collectionHeader = (name: string) =>
  wrapper!.findAll('.collection-folder > div').find((d) => d.text().includes(name))!

const collectionExpanded = (name: string) =>
  wrapper!
    .findAll('.collection-folder')
    .find((f) => f.text().includes(name))!
    .find('.collapse-section')
    .classes()
    .includes('is-expanded')

describe.each(routingModes)('DocumentationNav ($mode routing)', ({ mode, prefix }) => {
  beforeEach(() => {
    routing = mode
    slug = null
    Object.assign(
      pool,
      reactive({
        collectionsSorted: [collection(1, 'Handbook'), collection(2, 'Runbooks')],
        allPages: [docPage(1, 'Welcome', 1), docPage(2, 'Onboarding', 2)],
      }),
    )
  })

  it('lights the open collection and only that one', async () => {
    await mountAt(`${prefix}/documentation/collections/runbooks`)
    expect(collectionHeader('Runbooks').classes()).toContain('font-medium')
    expect(collectionHeader('Handbook').classes()).not.toContain('font-medium')
  })

  it('lights the open starred page', async () => {
    await mountAt(`${prefix}/documentation/onboarding`)
    const starred = wrapper!.findAll('a').find((a) => a.text().includes('Onboarding'))!
    expect(starred.classes()).toContain('font-medium')
  })

  // Auto-expand reads the page id from the path, so these open pages by id.
  it('opens the collection that holds the open page', async () => {
    await mountAt(`${prefix}/documentation/2`)
    expect(collectionExpanded('Runbooks')).toBe(true)
    expect(collectionExpanded('Handbook')).toBe(false)
  })

  it('opens the collection when navigating to one of its pages', async () => {
    const router = await mountAt(`${prefix}/documentation`)
    expect(collectionExpanded('Handbook')).toBe(false)
    await router.push(`${prefix}/documentation/1`)
    await flushPromises()
    expect(collectionExpanded('Handbook')).toBe(true)
  })

  it('links to Drafts, with a count, when a page is in no collection', async () => {
    pool.allPages = [...pool.allPages, docPage(3, 'Loose', null), docPage(4, 'Gone', null)]
    ;(pool.allPages[3] as DocPageRow).status = 'deleted'
    const router = await mountAt(`${prefix}/documentation`)
    const drafts = wrapper!.findAll('a').find((a) => a.text().includes('docs-nav-drafts'))!
    expect(drafts.text()).toContain('1')
    expect(drafts.classes()).not.toContain('font-medium')

    await drafts.trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.path).toBe(`${prefix}/documentation/drafts`)
    expect(drafts.classes()).toContain('font-medium')
  })

  it('leaves Drafts out when every page is in a collection', async () => {
    await mountAt(`${prefix}/documentation`)
    expect(wrapper!.text()).not.toContain('docs-nav-drafts')
  })
})

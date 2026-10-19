import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import type { CollectionPage } from '@nosdesk/core/services/collectionService'

// The collection tree selects the open page, with and without the
// workspace slug in the path.

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

import CollectionTreeList from '@/components/documentationComponents/CollectionTreeList.vue'
import { mountWithProviders } from '@/test/mountWithProviders'
import { makeWorkspaceRouter, routingModes, stubRoute } from '@/test/workspaceRouter'

const routes = [
  stubRoute('/documentation/collections/:slug', 'collection-view'),
  stubRoute('/documentation/:path', 'documentation-page'),
]

const page = (id: number, title: string, extra: Partial<CollectionPage> = {}): CollectionPage =>
  ({
    id,
    uuid: `u${id}`,
    slug: title.toLowerCase(),
    title,
    parent_id: null,
    status: 'published',
    icon: null,
    display_order: id,
    ...extra,
  }) as CollectionPage

const PAGES = [page(1, 'Alpha'), page(2, 'Beta', { parent_id: 1 }), page(3, 'Gamma')]

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

const selected = () =>
  wrapper!
    .findAll('[role="treeitem"]')
    .filter((r) => r.attributes('aria-selected') === 'true')
    .map((r) => r.find('span').text())

describe.each(routingModes)('CollectionTreeList ($mode routing)', ({ mode, prefix }) => {
  beforeEach(() => {
    routing = mode
    slug = null
  })

  it('selects the open page', async () => {
    const router = makeWorkspaceRouter(routes)
    await router.push(`${prefix}/documentation/beta`)
    wrapper = mountWithProviders(CollectionTreeList, { pages: PAGES }, {}, [router])
    await flushPromises()
    expect(selected()).toEqual(['Beta'])
  })

  it('selects nothing on the collection page', async () => {
    const router = makeWorkspaceRouter(routes)
    await router.push(`${prefix}/documentation/collections/demo`)
    wrapper = mountWithProviders(CollectionTreeList, { pages: PAGES }, {}, [router])
    await flushPromises()
    expect(selected()).toEqual([])
  })
})

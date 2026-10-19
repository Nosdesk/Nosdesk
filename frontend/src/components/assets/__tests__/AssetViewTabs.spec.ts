import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import { mountWithProviders } from '@/test/mountWithProviders'

// The asset tabs navigate between /assets, /assets/catalog and
// /assets/groups. In the hosted agent app every route carries the
// workspace slug in its path (/acme/assets/catalog), so the active tab
// has to be read from the route, not from a slug-less path literal.

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

import AssetViewTabs from '@/components/assets/AssetViewTabs.vue'
import {
  installSlugCarrier,
  installWorkspaceGuard,
  withWorkspaceRouting,
} from '@/router/workspaceRouting'

function makeRouter(): Router {
  const page = { template: '<div />' }
  const router = createRouter({
    history: createMemoryHistory(),
    routes: withWorkspaceRouting([
      { path: '/assets/catalog', name: 'asset-catalog', component: page, meta: { requiresAuth: true } },
      { path: '/assets/groups', name: 'asset-groups', component: page, meta: { requiresAuth: true } },
      { path: '/assets', name: 'assets', component: page, meta: { requiresAuth: true } },
      { path: '/assets/:id', name: 'asset-view', component: page, meta: { requiresAuth: true } },
    ]),
  })
  installSlugCarrier(router)
  installWorkspaceGuard(router)
  return router
}

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

async function mountAt(router: Router, path: string) {
  await router.push(path)
  await router.isReady()
  wrapper = mountWithProviders(AssetViewTabs, {}, {}, [router])
  await flushPromises()
}

function selected(): string[] {
  return wrapper!
    .findAll('[role="tab"]')
    .filter((t) => t.attributes('aria-selected') === 'true')
    .map((t) => t.text())
}

async function clickTab(label: string) {
  const tab = wrapper!.findAll('[role="tab"]').find((t) => t.text() === label)
  if (!tab) throw new Error(`no tab ${label}`)
  await tab.trigger('mousedown', { button: 0, ctrlKey: false })
  await flushPromises()
}

const modes = [
  { mode: 'host' as const, prefix: '' },
  { mode: 'path' as const, prefix: '/acme' },
]

describe.each(modes)('AssetViewTabs ($mode routing)', ({ mode, prefix }) => {
  beforeEach(() => {
    routing = mode
    slug = null
  })

  it.each(['/assets/catalog', '/assets/groups'])(
    'switches from %s back to Inventory and highlights the open tab',
    async (start) => {
      const router = makeRouter()
      await mountAt(router, `${prefix}${start}`)
      expect(selected()).toEqual([start.endsWith('catalog') ? 'asset-tabs-catalog' : 'asset-tabs-groups'])

      await clickTab('asset-tabs-inventory')
      expect(router.currentRoute.value.name).toBe('assets')
      expect(router.currentRoute.value.path).toBe(`${prefix}/assets`)
      expect(selected()).toEqual(['asset-tabs-inventory'])
    },
  )

  it('walks Inventory, Catalog, Groups and back, and the back button retraces it', async () => {
    const router = makeRouter()
    await mountAt(router, `${prefix}/assets`)
    expect(selected()).toEqual(['asset-tabs-inventory'])

    await clickTab('asset-tabs-catalog')
    expect(router.currentRoute.value.path).toBe(`${prefix}/assets/catalog`)
    await clickTab('asset-tabs-groups')
    expect(router.currentRoute.value.path).toBe(`${prefix}/assets/groups`)
    await clickTab('asset-tabs-inventory')
    expect(router.currentRoute.value.path).toBe(`${prefix}/assets`)

    router.back()
    await flushPromises()
    expect(router.currentRoute.value.path).toBe(`${prefix}/assets/groups`)
    expect(selected()).toEqual(['asset-tabs-groups'])
    router.back()
    await flushPromises()
    expect(router.currentRoute.value.path).toBe(`${prefix}/assets/catalog`)
    expect(selected()).toEqual(['asset-tabs-catalog'])
    router.back()
    await flushPromises()
    expect(router.currentRoute.value.path).toBe(`${prefix}/assets`)
    expect(selected()).toEqual(['asset-tabs-inventory'])
  })

  it('keeps Inventory highlighted on an asset detail page', async () => {
    const router = makeRouter()
    await mountAt(router, `${prefix}/assets/42`)
    expect(selected()).toEqual(['asset-tabs-inventory'])
  })
})

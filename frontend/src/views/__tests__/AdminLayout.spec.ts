import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'

// The admin sidebar lights the open section and the mobile bar names it,
// with and without the workspace slug in the path.

let routing: 'host' | 'path' = 'host'
let slug: string | null = null
vi.mock('@nosdesk/core/services/instanceConfig', () => ({
  fetchInstanceConfig: async () => {},
  getWorkspaceRouting: () => routing,
  isHostedDeployment: () => routing === 'path',
  getControlPlaneUrl: () => '',
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
vi.mock('@/stores/auth', () => ({
  useAuthStore: () => ({ isAdmin: true, isAuditReviewer: false, isPlatformAdmin: true, isOwner: false }),
}))

import AdminLayout from '@/views/AdminLayout.vue'
import { mountWithProviders } from '@/test/mountWithProviders'
import { makeWorkspaceRouter, routingModes } from '@/test/workspaceRouter'

const page = { template: '<div />' }
const routes = [
  {
    path: '/admin',
    component: page,
    meta: { requiresAuth: true },
    children: [
      { path: '', name: 'admin-index', component: page },
      { path: 'groups', name: 'admin-groups', component: page },
      { path: 'groups/:uuid/configure', name: 'group-configuration', component: page },
      { path: 'categories', name: 'admin-categories', component: page },
    ],
  },
]

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

const litLinks = () =>
  wrapper!
    .findAll('aside nav a')
    .filter((a) => a.classes().includes('font-medium'))
    .map((a) => a.text())

const breadcrumb = () => wrapper!.find('span.truncate.font-medium')

describe.each(routingModes)('AdminLayout ($mode routing)', ({ mode, prefix }) => {
  beforeEach(() => {
    routing = mode
    slug = null
  })

  it.each([
    ['/admin/categories', 'admin-nav-categories-title'],
    ['/admin/groups', 'admin-nav-groups-title'],
    ['/admin/groups/abc/configure', 'admin-nav-groups-title'],
  ])('lights and names the section on %s', async (path, title) => {
    const router = makeWorkspaceRouter(routes)
    await router.push(`${prefix}${path}`)
    wrapper = mountWithProviders(AdminLayout, {}, {}, [router])
    await flushPromises()
    expect(litLinks()).toEqual([title])
    expect(breadcrumb().text()).toBe(title)
  })

  it('lights nothing on the admin index', async () => {
    const router = makeWorkspaceRouter(routes)
    await router.push(`${prefix}/admin`)
    wrapper = mountWithProviders(AdminLayout, {}, {}, [router])
    await flushPromises()
    expect(litLinks()).toEqual([])
  })
})

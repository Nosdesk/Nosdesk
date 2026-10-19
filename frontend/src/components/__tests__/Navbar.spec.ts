import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import { flushPromises, type VueWrapper } from '@vue/test-utils'

// The sidebar lights the section that is open, with and without the
// workspace slug in the path (`/acme/tickets/12` lights Tickets).

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
vi.mock('@/composables/useNotificationFeed', () => ({
  useNotificationFeed: () => ({ unreadCount: ref(0) }),
}))
vi.mock('@/composables/useWorkspaceCapabilities', () => ({
  useWorkspaceCapabilities: () => ({ assetsVisible: ref(true) }),
}))
vi.mock('@/plugins/loader', () => ({ getSlotRegistrations: () => [] }))
vi.mock('@/components/documentationComponents/DocumentationNav.vue', () => ({
  default: { template: '<div />' },
}))
vi.mock('@/components/RecentTickets.vue', () => ({ default: { template: '<div />' } }))

import Navbar from '@/components/Navbar.vue'
import { mountWithProviders } from '@/test/mountWithProviders'
import { makeWorkspaceRouter, routingModes, stubRoute } from '@/test/workspaceRouter'

const routes = [
  stubRoute('/', 'home'),
  stubRoute('/tickets', 'tickets'),
  stubRoute('/tickets/:id', 'ticket-view'),
  stubRoute('/projects', 'projects'),
  stubRoute('/assets', 'assets'),
  stubRoute('/assets/catalog', 'asset-catalog'),
  stubRoute('/users', 'users'),
  stubRoute('/documentation', 'documentation'),
  stubRoute('/documentation/:path', 'documentation-page'),
  stubRoute('/inbox', 'inbox'),
]

const noopDirective = { install: (app: import('vue').App) => void app.directive('prefetch', {}) }

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

/** Labels of the sidebar links drawn as the open section. */
const lit = () =>
  wrapper!
    .findAll('nav a')
    .filter((a) => a.classes().includes('text-primary') && a.classes().includes('font-medium'))
    .map((a) => a.text())

describe.each(routingModes)('Navbar ($mode routing)', ({ mode, prefix }) => {
  beforeEach(() => {
    routing = mode
    slug = null
    localStorage.setItem('navbarCollapsed', 'false')
  })

  it.each([
    ['/', 'nav-dashboard'],
    ['/tickets', 'nav-tickets'],
    ['/tickets/12', 'nav-tickets'],
    ['/assets/catalog', 'nav-assets'],
    ['/documentation/onboarding', 'nav-documentation'],
  ])('lights the section for %s', async (path, label) => {
    const router = makeWorkspaceRouter(routes)
    await router.push(path === '/' ? prefix || '/' : `${prefix}${path}`)
    wrapper = mountWithProviders(Navbar, {}, {}, [router, noopDirective])
    await flushPromises()
    expect(lit()).toEqual([label])
  })
})

import { beforeEach, describe, expect, it, vi } from 'vitest'

// workspacePath reads the route's path inside its workspace, so a bare app
// path compares equal to it in both routing modes.

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

import { workspacePath } from '@/router/workspaceRouting'
import { makeWorkspaceRouter, routingModes, stubRoute } from '@/test/workspaceRouter'

const routes = [
  stubRoute('/', 'home'),
  stubRoute('/tickets', 'tickets'),
  stubRoute('/tickets/:id', 'ticket'),
  stubRoute('/documentation/:path', 'documentation-page'),
  { path: '/login', name: 'login', component: { template: '<div />' }, meta: { requiresAuth: false } },
]

describe.each(routingModes)('workspacePath ($mode routing)', ({ mode, prefix }) => {
  beforeEach(() => {
    routing = mode
    slug = null
  })

  it.each([
    ['/tickets', '/tickets'],
    ['/tickets/42', '/tickets/42'],
    ['/documentation/acme', '/documentation/acme'],
  ])('reads %s as %s', async (path, expected) => {
    const router = makeWorkspaceRouter(routes)
    await router.push(`${prefix}${path}`)
    expect(router.currentRoute.value.path).toBe(`${prefix}${path}`)
    expect(workspacePath(router.currentRoute.value)).toBe(expected)
  })

  it('reads the workspace home as /', async () => {
    const router = makeWorkspaceRouter(routes)
    await router.push(prefix || '/')
    expect(router.currentRoute.value.name).toBe('home')
    expect(workspacePath(router.currentRoute.value)).toBe('/')
  })

  it('leaves a public path as it is', async () => {
    const router = makeWorkspaceRouter(routes)
    await router.push('/login')
    expect(workspacePath(router.currentRoute.value)).toBe('/login')
  })
})

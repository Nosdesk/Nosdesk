import { afterEach, describe, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

// A widget's links are offered only to people who can open them, and carry
// the workspace in the path like every other navigation in path mode.

vi.mock('@nosdesk/core/services/instanceConfig', async (importOriginal) => ({
  ...(await importOriginal<object>()),
  getWorkspaceRouting: () => 'path',
}))
vi.mock('@/services/activeWorkspace', async (importOriginal) => ({
  ...(await importOriginal<object>()),
  activeWorkspaceSlug: () => 'acme',
}))

const viewer = {
  isAdmin: false,
  isAuditReviewer: false,
  isPlatformAdmin: false,
  user: { workspace_role: 'agent' },
}
vi.mock('@/stores/auth', () => ({ useAuthStore: () => viewer }))

import { withWorkspaceRouting } from '@/router/workspaceRouting'
import DashboardWidgetShell from '@/views/dashboard/DashboardWidgetShell.vue'

function makeRouter(): Router {
  const page = { template: '<div />' }
  return createRouter({
    history: createMemoryHistory(),
    routes: withWorkspaceRouting([
      { path: '/', name: 'home', component: page },
      { path: '/tickets', name: 'tickets', component: page },
      {
        path: '/admin',
        component: page,
        meta: { adminRequired: true },
        children: [{ path: 'sla', name: 'admin-sla', component: page }],
      },
    ]),
  })
}

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

async function mountShell(props: Record<string, unknown>) {
  const router = makeRouter()
  await router.push('/acme')
  wrapper = mountWithProviders(
    DashboardWidgetShell,
    { title: 'Widget', actionLabel: 'Open', ...props },
    {},
    [router],
  )
  await flushPromises()
  return wrapper
}

function asAgent() {
  viewer.isAdmin = false
  viewer.user.workspace_role = 'agent'
}
function asAdmin() {
  viewer.isAdmin = true
  viewer.user.workspace_role = 'admin'
}

describe('DashboardWidgetShell links', () => {
  it('hides an admin link from an agent', async () => {
    asAgent()
    const w = await mountShell({ actionTo: '/admin/sla' })
    expect(w.find('a').exists()).toBe(false)
  })

  it('gives an admin the link in their workspace', async () => {
    asAdmin()
    const w = await mountShell({ actionTo: '/admin/sla' })
    const link = w.get('a')
    expect(link.attributes('href')).toBe('/acme/admin/sla')
    expect(link.text()).toBe('Open')
  })

  it('gives an agent an open link in their workspace', async () => {
    asAgent()
    const w = await mountShell({ actionTo: '/tickets' })
    expect(w.get('a').attributes('href')).toBe('/acme/tickets')
  })

  it('hides an empty-state link the viewer cannot open', async () => {
    asAgent()
    const w = await mountShell({ empty: true, emptyCtaTo: '/admin/sla', emptyCtaLabel: 'Set up' })
    expect(w.find('a').exists()).toBe(false)
    asAdmin()
    const a = await mountShell({ empty: true, emptyCtaTo: '/admin/sla', emptyCtaLabel: 'Set up' })
    expect(a.get('a').attributes('href')).toBe('/acme/admin/sla')
  })
})

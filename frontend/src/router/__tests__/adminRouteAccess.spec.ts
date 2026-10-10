import { beforeAll, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

// Every page under /admin turns a non-admin agent away the same way: back to
// the dashboard, never a blank page or a 404. The real route table and guards
// run here; only the session and the services behind them are stubbed.

vi.mock('extendable-media-recorder', () => ({ MediaRecorder: class {}, register: async () => {} }))
vi.mock('extendable-media-recorder-wav-encoder', () => ({ connect: async () => ({}) }))

vi.mock('@nosdesk/core/services/instanceConfig', async (importOriginal) => ({
  ...(await importOriginal<object>()),
  fetchInstanceConfig: async () => {},
  getWorkspaceRouting: () => 'path',
  isHostedDeployment: () => true,
}))
vi.mock('@nosdesk/core/services/authService', async (importOriginal) => {
  const actual = await importOriginal<{ default: object }>()
  return {
    ...actual,
    default: { ...actual.default, checkSetupStatus: async () => ({ requires_setup: false }) },
  }
})

const agent = {
  isAuthenticated: true,
  loading: false,
  user: { uuid: 'agent-1', workspace_role: 'agent', platform_role: null },
  isAdmin: false,
  isTechnician: true,
  isAuditReviewer: false,
  isPlatformAdmin: false,
  ensureWorkspaceIdentity: async () => {},
}
vi.mock('@/stores/auth', async (importOriginal) => ({
  ...(await importOriginal<object>()),
  useAuthStore: () => agent,
}))
vi.mock('@/stores/workspaceReset', () => ({
  resetWorkspaceScopedState: async () => {},
  enterWorkspace: async () => {},
}))
vi.mock('@nosdesk/core/stores/featureFlags', () => ({
  useFeatureFlagsStore: () => ({ loaded: true, loading: false, load: async () => {} }),
}))
vi.mock('@nosdesk/core/stores/workflowStates', () => ({
  useWorkflowStatesStore: () => ({ loaded: true, loading: false, load: async () => {} }),
}))
vi.mock('@/sync/lifecycle', () => ({ ensureSyncRuntime: async () => {} }))

import router from '@/router'

/** A concrete URL for a route record's path, in workspace `acme`. */
function concrete(path: string): string {
  return path
    .replace('/:workspace?', '/acme')
    .replace(/:pathMatch\([^)]*\)\*/, 'no-such-page')
    .replace(/:id\([^)]*\)/g, '1')
    .replace(/:[A-Za-z]+/g, 'x')
}

const adminPaths = router
  .getRoutes()
  .map((r) => r.path)
  .filter((p) => p === '/:workspace?/admin' || p.startsWith('/:workspace?/admin/'))
  .map(concrete)

beforeAll(async () => {
  setActivePinia(createPinia())
  await router.push('/acme')
})

describe('admin routes for a non-admin agent', () => {
  it('finds the admin route table', () => {
    expect(adminPaths.length).toBeGreaterThan(40)
  })

  it.each([
    ...adminPaths,
    // Not routes: a guessed or stale admin URL is gated like a real one.
    '/acme/admin/branding',
    '/acme/admin/does-not-exist/deeper',
    '/admin/branding',
  ])('%s goes back to the dashboard', async (path) => {
    await router.push('/acme/tickets')
    expect(router.currentRoute.value.name).toBe('tickets')
    await router.push(path)
    expect(router.currentRoute.value.name).toBe('home')
    expect(router.currentRoute.value.params.workspace).toBe('acme')
  })
})

describe('admin routes for an admin', () => {
  it('keeps the admin home and real pages ahead of the not-found page', () => {
    expect(router.resolve('/acme/admin').name).toBe('admin-index')
    expect(router.resolve('/acme/admin/settings/branding').name).toBe('admin-branding')
  })

  it('shows the 404 page for an /admin URL that is not a page', async () => {
    Object.assign(agent, { isAdmin: true, user: { ...agent.user, workspace_role: 'admin' } })
    try {
      await router.push('/acme/tickets')
      await router.push('/acme/admin/branding')
      expect(router.currentRoute.value.name).toBe('error')
      expect(router.currentRoute.value.params.code).toBe('404')
    } finally {
      Object.assign(agent, { isAdmin: false, user: { ...agent.user, workspace_role: 'agent' } })
    }
  })
})

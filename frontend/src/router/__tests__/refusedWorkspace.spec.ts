import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { AxiosError, type AxiosResponse, type InternalAxiosRequestConfig } from 'axios'

// On Nosdesk Cloud the workspace is the first segment of the URL, and the server
// refuses (403) every request under a workspace the account isn't in, or one
// that doesn't exist. A stale or mistyped link is a bad link, not a lost
// session: the person stays signed in and sees the not-found page, from which
// the way home lands in a workspace of theirs. The real route table, guards and
// auth store run here; only the server is stubbed.

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
  // The service is a class instance: keep its methods, answer setup status.
  const service = Object.create(actual.default)
  service.checkSetupStatus = async () => ({ requires_setup: false })
  return { ...actual, default: service }
})
// Leaving a workspace drops its slug, as the real reset does; the rest of the
// teardown (sync, collab, plugins) has its own specs.
vi.mock('@/stores/workspaceReset', () => ({
  resetWorkspaceScopedState: vi.fn(async () => {
    const { setActiveWorkspaceSlug } = await import('@/services/activeWorkspace')
    setActiveWorkspaceSlug(null)
  }),
  enterWorkspace: async () => {},
}))
// What a workspace loads once the person is in it: flags, workflow states and
// the sync runtime (which opens the live-updates stream). Never loaded, so
// every navigation into a workspace asks.
const loads = vi.hoisted(() => ({
  flags: vi.fn(async () => {}),
  workflowStates: vi.fn(async () => {}),
  syncRuntime: vi.fn(async (_user: string, _workspace: string | null) => {}),
}))
vi.mock('@nosdesk/core/stores/featureFlags', () => ({
  useFeatureFlagsStore: () => ({ loaded: false, loading: false, load: loads.flags, reset: () => {} }),
}))
vi.mock('@nosdesk/core/stores/workflowStates', () => ({
  useWorkflowStatesStore: () => ({
    loaded: false,
    loading: false,
    load: loads.workflowStates,
    reset: () => {},
  }),
}))
vi.mock('@/sync/lifecycle', () => ({ ensureSyncRuntime: loads.syncRuntime, tearDown: async () => {} }))

const MEMBER_OF = ['acme']

/** Requests the server was sent, as `METHOD url [workspace]`. */
const sent: string[] = []

function respond(config: InternalAxiosRequestConfig, status: number, data: unknown) {
  const response = { status, statusText: '', data, headers: {}, config } as AxiosResponse
  if (status >= 400) throw new AxiosError('refused', 'ERR_BAD_REQUEST', config, null, response)
  return response
}

/** The server: the membership gate refuses any workspace outside `MEMBER_OF`. */
async function server(config: InternalAxiosRequestConfig): Promise<AxiosResponse> {
  const workspace = config.headers?.get?.('X-Nosdesk-Workspace') as string | undefined
  sent.push(`${config.method?.toUpperCase()} ${config.url} [${workspace ?? ''}]`)
  if (workspace && !MEMBER_OF.includes(workspace)) {
    return respond(config, 403, { error: 'Not a member of this workspace', code: 'FORBIDDEN' })
  }
  if (config.url?.endsWith('/auth/me')) {
    return respond(config, 200, {
      uuid: 'agent-1',
      name: 'Agent',
      email: 'agent@example.com',
      platform_role: null,
      workspace_role: workspace ? 'admin' : null,
    })
  }
  if (config.url?.endsWith('/me/workspaces')) {
    return respond(config, 200, [{ workspace_id: 1, slug: 'acme', name: 'Acme', role: 'admin' }])
  }
  return respond(config, 200, {})
}

let router: typeof import('@/router').default
let auth: ReturnType<typeof import('@/stores/auth').useAuthStore>
let activeSlug: () => string | null

beforeAll(async () => {
  setActivePinia(createPinia())
  // Signed in: the page loads holding the session's CSRF cookie.
  document.cookie = 'csrf_token=present; path=/'
  const { configurePlatform } = await import('@/platform')
  await configurePlatform()
  const { default: apiClient } = await import('@nosdesk/core/apiClient')
  apiClient.defaults.adapter = server
  router = (await import('@/router')).default
  auth = (await import('@/stores/auth')).useAuthStore()
  activeSlug = (await import('@/services/activeWorkspace')).activeWorkspaceSlug
})

beforeEach(() => {
  sent.length = 0
  for (const load of Object.values(loads)) load.mockClear()
})

/** Nothing that belongs to a workspace was loaded with none selected. */
function expectNothingLoadedOutsideAWorkspace() {
  expect(loads.flags).not.toHaveBeenCalled()
  expect(loads.workflowStates).not.toHaveBeenCalled()
  expect(loads.syncRuntime).not.toHaveBeenCalled()
}

function expectStillSignedIn() {
  expect(sent.filter((s) => s.includes('/auth/logout'))).toEqual([])
  expect(auth.user?.uuid).toBe('agent-1')
}

describe('a link to a workspace the account cannot open', () => {
  it('shows the not-found page and keeps the person signed in on a fresh load', async () => {
    await router.push('/requester-portal')

    expect(router.currentRoute.value.name).toBe('error')
    expect(router.currentRoute.value.params.code).toBe('404')
    expectStillSignedIn()
    // The refused workspace is left: nothing more is asked of the server under it.
    expect(activeSlug()).toBeNull()
    // Nor outside it: the not-found page isn't in a workspace, so it loads none
    // of one, the live-updates stream included.
    expectNothingLoadedOutsideAWorkspace()
  })

  it('leads home to a workspace of theirs', async () => {
    await router.push('/')

    expect(router.currentRoute.value.name).toBe('home')
    expect(router.currentRoute.value.params.workspace).toBe('acme')
    expect(auth.user?.workspace_role).toBe('admin')
    expect(loads.syncRuntime).toHaveBeenCalledWith('agent-1', 'acme')
    expect(loads.flags).toHaveBeenCalled()
  })

  it('does the same when followed from inside a workspace', async () => {
    await router.push('/acme/tickets')
    expect(router.currentRoute.value.name).toBe('tickets')
    for (const load of Object.values(loads)) load.mockClear()

    await router.push('/someone-elses/tickets/5')

    expect(router.currentRoute.value.name).toBe('error')
    expect(router.currentRoute.value.params.code).toBe('404')
    expectStillSignedIn()
    expect(activeSlug()).toBeNull()
    expectNothingLoadedOutsideAWorkspace()
  })
})

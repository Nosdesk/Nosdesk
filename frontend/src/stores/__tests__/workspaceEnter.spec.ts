import { beforeEach, describe, expect, it, vi } from 'vitest'

// Entering a workspace (its branding, plugins and draft scope) happens once per
// session and workspace. The guard asks on every navigation into a workspace
// while signed in, so the first navigation after signing in enters even when
// the slug was already published by the signed-out visit before it.

const loadWorkspaceBranding = vi.fn(async () => {})
const loadPlugins = vi.fn(async () => {})
const setScope = vi.fn()
vi.mock('@/stores/branding', () => ({ useBrandingStore: () => ({ loadWorkspaceBranding }) }))
vi.mock('@/plugins', () => ({ loadPlugins, unloadAllPlugins: () => {} }))
// The reset's sync, stream and collab teardown, which need IndexedDB.
vi.mock('@/sync/lifecycle', () => ({ tearDown: async () => {} }))
vi.mock('@/sync/sseBridge', () => ({ detachSseBridge: () => {} }))
vi.mock('@/utils/collabLocalCache', () => ({ purgeAllCollabDocs: async () => {} }))
vi.mock('@/stores/collabSession', () => ({ useCollabSessionStore: () => ({ closeAll: async () => {} }) }))
vi.mock('@nosdesk/core/stores/ticketDrafts', () => ({ useTicketDraftsStore: () => ({ setScope }) }))

beforeEach(() => {
  vi.resetModules()
  loadWorkspaceBranding.mockClear()
  loadPlugins.mockClear()
  setScope.mockClear()
})

describe('entering a workspace', () => {
  it('loads once per workspace, and again for another', async () => {
    const { enterWorkspace } = await import('@/stores/workspaceReset')
    await enterWorkspace('mercury')
    await enterWorkspace('mercury')
    expect(loadPlugins).toHaveBeenCalledTimes(1)
    expect(loadWorkspaceBranding).toHaveBeenCalledTimes(1)
    expect(setScope).toHaveBeenCalledWith('mercury')

    await enterWorkspace('venus')
    expect(loadPlugins).toHaveBeenCalledTimes(2)
  })

  // The reset imports every workspace-scoped store, slow on a cold run.
  it('loads again after the workspace state is reset (a sign-out or a switch)', { timeout: 30_000 }, async () => {
    const { enterWorkspace, resetWorkspaceScopedState } = await import('@/stores/workspaceReset')
    await enterWorkspace('mercury')
    await resetWorkspaceScopedState('sign-out')
    await enterWorkspace('mercury')
    expect(loadPlugins).toHaveBeenCalledTimes(2)
  })
})

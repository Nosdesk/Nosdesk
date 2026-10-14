import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { effectScope, nextTick } from 'vue'
import { useQueryCache } from '@pinia/colada'

// A workspace switch clears the workspace's cached queries but keeps the
// account's: the list of workspaces the person belongs to is the same on both
// sides, and a collaborative note's doc id is built from it. Dropping it left
// the doc id null for a round trip, so the first keystrokes into an open note
// landed nowhere.

const { slug, auth, listMyWorkspaces } = await vi.hoisted(async () => {
  const { reactive, ref } = await import('vue')
  return {
    slug: ref<string | null>('mercury'),
    auth: reactive({ user: { uuid: 'user-1' } as { uuid: string } | null, isAuthenticated: true }),
    listMyWorkspaces: vi.fn(),
  }
})
vi.mock('@/services/activeWorkspace', () => ({
  activeWorkspaceSlugRef: slug,
  activeWorkspaceSlug: () => slug.value,
  setActiveWorkspaceSlug: (s: string | null) => (slug.value = s),
}))
vi.mock('@nosdesk/core/services/instanceConfig', () => ({
  getWorkspaceRouting: () => 'path',
}))
vi.mock('@/stores/auth', () => ({ useAuthStore: () => auth }))
vi.mock('@nosdesk/core/services/workspacesService', () => ({
  default: { listMyWorkspaces: () => listMyWorkspaces() },
}))

// The teardown steps the reset runs before the query cache; stubbed so the
// test exercises the cache handling only.
vi.mock('@/sync/lifecycle', () => ({ tearDown: vi.fn(async () => {}) }))
vi.mock('@/sync/sseBridge', () => ({ detachSseBridge: vi.fn() }))
vi.mock('@/utils/collabLocalCache', () => ({ purgeAllCollabDocs: vi.fn(async () => {}) }))
vi.mock('@/stores/collabSession', () => ({
  useCollabSessionStore: () => ({ closeAll: vi.fn(async () => {}) }),
}))
vi.mock('@nosdesk/core/stores/featureFlags', () => ({
  useFeatureFlagsStore: () => ({ reset: vi.fn() }),
}))
vi.mock('@nosdesk/core/stores/workflowStates', () => ({
  useWorkflowStatesStore: () => ({ reset: vi.fn() }),
}))
vi.mock('@/composables/useWorkspaceCapabilities', () => ({
  resetWorkspaceCapabilities: vi.fn(),
}))
vi.mock('@/stores/savedViews', () => ({ useSavedViewsStore: () => ({ reset: vi.fn() }) }))
vi.mock('@/stores/assetGroups', () => ({ useAssetGroupsStore: () => ({ reset: vi.fn() }) }))
vi.mock('@/stores/recentUsers', () => ({ useRecentUsersStore: () => ({ clear: vi.fn() }) }))
vi.mock('@nosdesk/core/stores/ticketDrafts', () => ({
  useTicketDraftsStore: () => ({ setScope: vi.fn() }),
}))
vi.mock('@/plugins', () => ({ unloadAllPlugins: vi.fn() }))
vi.mock('@/services/collabToken', () => ({ resetCollabToken: vi.fn() }))

import { resetWorkspaceScopedState } from '@/stores/workspaceReset'
import { MY_WORKSPACES_KEY, useMyWorkspacesStore } from '@/stores/myWorkspaces'
import { useCollabDocId } from '@/composables/useCollabDocId'
import type { MyWorkspaceEntry } from '@nosdesk/core/types/workspace'

const MERCURY_UUID = '11111111-1111-4111-8111-111111111111'
const VENUS_UUID = '22222222-2222-4222-8222-222222222222'
const TICKET_UUID = '33333333-3333-4333-8333-333333333333'

const entry = (workspace_id: number, workspace_uuid: string, slug: string): MyWorkspaceEntry => ({
  workspace_id,
  workspace_uuid,
  slug,
  name: slug,
  custom_domain: null,
  role: 'member',
  invited_at: '2026-01-01T00:00:00Z',
  accepted_at: '2026-01-01T00:00:00Z',
  logo_url: null,
})

const memberships = [entry(1, MERCURY_UUID, 'mercury'), entry(2, VENUS_UUID, 'venus')]

/**
 * The guard's sequence for a switch: reset, publish the new slug, then
 * re-resolve the signed-in user under the new workspace (a fresh /auth/me,
 * so a new user object with the same uuid).
 */
async function switchTo(next: string) {
  await resetWorkspaceScopedState('switch')
  slug.value = next
  auth.user = { uuid: 'user-1' }
  await nextTick()
}

let scope = effectScope()
let pinia = createPinia()
afterEach(() => {
  scope.stop()
  // A setup store lives in the pinia's own scope; stop it so one test's
  // store doesn't keep reacting to the shared auth and slug refs.
  pinia._e.stop()
})

beforeEach(() => {
  scope = effectScope()
  pinia = createPinia()
  setActivePinia(pinia)
  slug.value = 'mercury'
  auth.user = { uuid: 'user-1' }
  listMyWorkspaces.mockReset()
  listMyWorkspaces.mockResolvedValue(memberships)
})

describe('workspace switch and account-scoped queries', () => {
  it('keeps the workspace list and drops workspace-scoped queries', async () => {
    const cache = useQueryCache()
    cache.setQueryData([...MY_WORKSPACES_KEY, 'user-1'], memberships)
    cache.setQueryData(['tickets', 'recent'], [{ id: 1 }])

    await switchTo('venus')

    expect(cache.getQueryData([...MY_WORKSPACES_KEY, 'user-1'])).toEqual(memberships)
    expect(cache.getEntries({ key: ['tickets', 'recent'] })).toHaveLength(0)
  })

  it('still drops the workspace list on sign-out', async () => {
    const cache = useQueryCache()
    cache.setQueryData([...MY_WORKSPACES_KEY, 'user-1'], memberships)

    await resetWorkspaceScopedState('sign-out')

    expect(cache.getEntries({ key: [...MY_WORKSPACES_KEY] })).toHaveLength(0)
  })

  it('resolves the note doc id straight after a switch, without a fetch', async () => {
    const store = scope.run(() => useMyWorkspacesStore())!
    const docId = scope.run(() => useCollabDocId('ticket', TICKET_UUID))!
    // Let the first load land so the list is cached.
    await vi.waitFor(() => expect(store.workspaces).toHaveLength(2))
    expect(docId.value).toContain(MERCURY_UUID)

    // Any refetch the switch starts never answers: the doc id has to come
    // from what is already cached.
    listMyWorkspaces.mockReturnValue(new Promise(() => {}))
    await switchTo('venus')

    expect(store.activeWorkspace?.workspace_uuid).toBe(VENUS_UUID)
    expect(docId.value).toContain(VENUS_UUID)
  })

  it('refetches the list on a switch, so a just-joined workspace resolves', async () => {
    const store = scope.run(() => useMyWorkspacesStore())!
    await vi.waitFor(() => expect(store.workspaces).toHaveLength(2))

    const joined = entry(3, '44444444-4444-4444-8444-444444444444', 'mars')
    let answer!: (list: MyWorkspaceEntry[]) => void
    listMyWorkspaces.mockReturnValue(new Promise((resolve) => (answer = resolve)))
    await switchTo('mars')

    // Not in the cached list yet: no workspace, rather than another one's.
    expect(listMyWorkspaces).toHaveBeenCalledTimes(2)
    expect(store.activeWorkspace).toBeNull()
    answer([...memberships, joined])
    await vi.waitFor(() => expect(store.activeWorkspace?.slug).toBe('mars'))
  })
})

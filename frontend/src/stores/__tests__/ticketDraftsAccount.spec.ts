import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { AxiosRequestConfig } from 'axios'

// Reply drafts and staged files belong to the account that wrote them, on the
// server and in the workspace they were written in. Another account in the
// same tab, or the same ticket id on another server, starts empty; sign-out
// takes them away; and tabs write their own drafts without erasing each
// other's.

// Uploads hang until aborted, so a sign-out can be seen stopping them.
const aborted: string[] = []
vi.mock('@nosdesk/core/apiClient', () => ({
  default: {
    post: (_url: string, form: FormData, config: AxiosRequestConfig) =>
      new Promise((_resolve, reject) => {
        config.signal?.addEventListener('abort', () => {
          aborted.push((form.get('files') as File).name)
          reject(Object.assign(new Error('canceled'), { code: 'ERR_CANCELED' }))
        })
      }),
  },
}))

// The reset's other steps, stubbed so the drafts and staged files are what
// the sign-out test exercises.
vi.mock('@/services/activeWorkspace', () => ({
  activeWorkspaceSlug: () => null,
  setActiveWorkspaceSlug: () => {},
}))
vi.mock('@/sync/lifecycle', () => ({ tearDown: async () => {} }))
vi.mock('@/sync/sseBridge', () => ({ detachSseBridge: () => {} }))
vi.mock('@/utils/collabLocalCache', () => ({ purgeAllCollabDocs: async () => {} }))
vi.mock('@/stores/collabSession', () => ({
  useCollabSessionStore: () => ({ closeAll: async () => {} }),
}))
vi.mock('@nosdesk/core/stores/featureFlags', () => ({
  useFeatureFlagsStore: () => ({ reset: () => {} }),
}))
vi.mock('@nosdesk/core/stores/workflowStates', () => ({
  useWorkflowStatesStore: () => ({ reset: () => {} }),
}))
vi.mock('@/composables/useWorkspaceCapabilities', () => ({
  resetWorkspaceCapabilities: () => {},
}))
vi.mock('@/stores/savedViews', () => ({ useSavedViewsStore: () => ({ reset: () => {} }) }))
vi.mock('@/stores/assetGroups', () => ({ useAssetGroupsStore: () => ({ reset: () => {} }) }))
vi.mock('@/stores/recentUsers', () => ({ useRecentUsersStore: () => ({ clear: () => {} }) }))
vi.mock('@/plugins', () => ({ unloadAllPlugins: () => {} }))
vi.mock('@/services/collabToken', () => ({ resetCollabToken: () => {} }))
vi.mock('@pinia/colada', () => ({
  useQueryCache: () => ({ getEntries: () => [], remove: () => {}, invalidateQueries: async () => {} }),
}))

// Installs the in-memory localStorage shim the other specs rely on.
import '@/test/mountWithProviders'
import { configureStorage } from '@nosdesk/core/storage'
import { configureTransport, type AuthStrategy } from '@nosdesk/core/transport'
import { removeAllTicketDrafts, useTicketDraftsStore } from '@nosdesk/core/stores/ticketDrafts'
import { useTicketUiStore } from '@nosdesk/core/stores/ticketUi'
import { resetWorkspaceScopedState } from '@/stores/workspaceReset'

const ALICE = '11111111-1111-4111-8111-111111111111'
const BOB = '22222222-2222-4222-8222-222222222222'

const storedKeys = () =>
  Array.from({ length: localStorage.length }, (_, i) => localStorage.key(i)!)
const draftKeys = () => storedKeys().filter((key) => key.startsWith('nosdesk:ticket-draft'))

function useServer(origin: string): void {
  configureTransport({
    baseUrl: `${origin}/api`,
    collabWsBaseUrl: `${origin.replace(/^http/, 'ws')}/collaboration/ws`,
    auth: {} as AuthStrategy,
  })
}

/** A fresh page (or tab) on the same storage, signed in as `account`. */
function openTab(account: string | null, workspace: string | null = 'mercury') {
  setActivePinia(createPinia())
  const drafts = useTicketDraftsStore()
  drafts.setScope(workspace)
  drafts.setAccount(account)
  return drafts
}

beforeAll(() => {
  // jsdom has no object URLs; each staged file asks for one.
  let n = 0
  URL.createObjectURL = vi.fn(() => `blob:preview-${++n}`)
  URL.revokeObjectURL = vi.fn()
})
beforeEach(() => {
  localStorage.clear()
  configureStorage({
    getItem: (key) => localStorage.getItem(key),
    setItem: (key, value) => localStorage.setItem(key, value),
    removeItem: (key) => localStorage.removeItem(key),
    keys: storedKeys,
  })
  useServer('https://desk.example.com')
  aborted.length = 0
})
afterEach(() => vi.clearAllMocks())

describe('a reply draft', () => {
  it('is not shown to another account on the same workspace and ticket', () => {
    const alice = openTab(ALICE)
    alice.setDraft(12, { content: 'Internal: the VPN key is in the vault', isInternal: true })

    // Bob signs in on the same tab, and on a fresh page.
    alice.setAccount(BOB)
    expect(alice.getDraft(12).content).toBe('')
    expect(openTab(BOB).getDraft(12).content).toBe('')
  })

  it('stays with its account when no sign-out ran in between', () => {
    // Alice's session ended with the tab closed; Bob opens a fresh page.
    openTab(ALICE).setDraft(12, { content: 'Alice reply', isInternal: false })

    expect(openTab(BOB).getDraft(12).content).toBe('')
    expect(openTab(ALICE).getDraft(12).content).toBe('Alice reply')
  })

  it('is not shown on another server for the same ticket', () => {
    useServer('https://a.example.com')
    openTab(ALICE).setDraft(12, { content: 'Server A reply', isInternal: false })

    // The mobile app reloads onto the other server.
    useServer('https://b.example.com')
    expect(openTab(ALICE).getDraft(12).content).toBe('')

    useServer('https://a.example.com')
    expect(openTab(ALICE).getDraft(12).content).toBe('Server A reply')
  })

  it('waits for the account before it loads or stores anything', () => {
    openTab(ALICE).setDraft(12, { content: 'Mine', isInternal: false })

    // A cold load publishes the workspace before the profile arrives.
    setActivePinia(createPinia())
    const drafts = useTicketDraftsStore()
    drafts.setScope('mercury')
    expect(drafts.getScope()).toBeNull()
    expect(drafts.getDraft(12).content).toBe('')
    drafts.setDraft(14, { content: 'Typed before the profile', isInternal: false })
    expect(draftKeys().some((key) => key.endsWith(':14'))).toBe(false)

    drafts.setAccount(ALICE)
    expect(drafts.getDraft(12).content).toBe('Mine')
  })

  it('from before drafts carried an account is dropped, not shown', () => {
    localStorage.setItem('nosdesk:ticket-drafts', JSON.stringify({ 12: { content: 'Host mode', isInternal: false } }))
    localStorage.setItem(
      'nosdesk:ticket-drafts:mercury',
      JSON.stringify({ 12: { content: 'Someone\'s note', isInternal: true } }),
    )
    expect(openTab(ALICE).getDraft(12).content).toBe('')
    expect(storedKeys().filter((key) => key.startsWith('nosdesk:ticket-drafts'))).toEqual([])
  })
})

describe('a lost session', () => {
  it('unloads the drafts and staged files and keeps the stored drafts for the account', async () => {
    const drafts = openTab(ALICE)
    drafts.setDraft(12, { content: 'Half-written reply', isInternal: false })
    const ui = useTicketUiStore()
    ui.stageFiles(12, [new File(['log'], 'trace.log', { type: 'text/plain' })])

    // As `clearLocalSession` after an expiry: the user goes, then the reset.
    drafts.setAccount(null)
    await resetWorkspaceScopedState('session-lost')

    expect(drafts.getScope()).toBeNull()
    expect(drafts.getDraft(12).content).toBe('')
    expect(ui.getAttachments(12)).toEqual([])
    expect(aborted).toEqual(['trace.log'])

    // Another account never sees it; the same account signing back in does.
    expect(openTab(BOB).getDraft(12).content).toBe('')
    drafts.setAccount(BOB)
    expect(drafts.getDraft(12).content).toBe('')
    expect(openTab(ALICE).getDraft(12).content).toBe('Half-written reply')
  })

  it('in the same tab, loads the draft again when the account signs back in', async () => {
    const drafts = openTab(ALICE)
    drafts.setDraft(12, { content: 'Half-written reply', isInternal: false })
    drafts.setAccount(null)
    await resetWorkspaceScopedState('session-lost')

    drafts.setAccount(ALICE)
    drafts.setScope('mercury')
    expect(drafts.getDraft(12).content).toBe('Half-written reply')
  })
})

describe('sign-out', () => {
  it('removes the account\'s drafts and staged files', async () => {
    const drafts = openTab(ALICE)
    drafts.setDraft(12, { content: 'Half-written reply', isInternal: false })
    drafts.setScope('venus')
    drafts.setDraft(3, { content: 'Venus note', isInternal: true })
    const ui = useTicketUiStore()
    const [staged] = ui.stageFiles(12, [new File(['secret'], 'payroll.csv', { type: 'text/csv' })])

    // As `clearLocalSession`: the user goes first, then the reset.
    drafts.setAccount(null)
    await resetWorkspaceScopedState('sign-out')

    expect(draftKeys()).toEqual([])
    expect(drafts.getScope()).toBeNull()
    expect(ui.getAttachments(12)).toEqual([])
    expect(aborted).toEqual(['payroll.csv'])
    expect(URL.revokeObjectURL).toHaveBeenCalledWith(staged.previewUrl)

    // The same account signing in again starts empty too.
    expect(openTab(ALICE).getDraft(12).content).toBe('')
    expect(openTab(ALICE, 'venus').getDraft(3).content).toBe('')
  })

  it('removes stored drafts from the reset alone', async () => {
    openTab(ALICE).setDraft(12, { content: 'Reply', isInternal: false })
    await resetWorkspaceScopedState('sign-out')
    expect(draftKeys()).toEqual([])
  })

  it('removes the drafts of an account another signs in over', () => {
    const drafts = openTab(ALICE)
    drafts.setDraft(12, { content: 'Alice reply', isInternal: false })
    drafts.setAccount(BOB)
    drafts.setDraft(12, { content: 'Bob reply', isInternal: false })

    expect(draftKeys()).toHaveLength(1)
    expect(openTab(ALICE).getDraft(12).content).toBe('')
  })

  it('on the mobile app also clears storage with no store loaded', () => {
    openTab(ALICE).setDraft(12, { content: 'Reply', isInternal: false })
    localStorage.setItem('nosdesk:ticket-drafts', '{}')
    localStorage.setItem('nosdesk:recent-tickets:x', '[]')

    removeAllTicketDrafts()

    expect(draftKeys()).toEqual([])
    expect(storedKeys()).toEqual(['nosdesk:recent-tickets:x'])
  })
})

describe('drafts in two tabs', () => {
  it('keep each other\'s drafts', () => {
    // Both tabs loaded before either wrote.
    const tab1 = openTab(ALICE)
    const tab2 = openTab(ALICE)
    tab1.setDraft(5, { content: 'Tab 1 draft', isInternal: false })
    tab2.setDraft(7, { content: 'Tab 2 draft', isInternal: true })
    // Sending tab 2's reply clears only its own draft.
    tab2.setDraft(9, { content: 'Sent from tab 2', isInternal: false })
    tab2.clearDraft(9)

    const later = openTab(ALICE)
    expect(later.getDraft(5).content).toBe('Tab 1 draft')
    expect(later.getDraft(7)).toEqual({ content: 'Tab 2 draft', isInternal: true })
    expect(later.getDraft(9).content).toBe('')
  })
})

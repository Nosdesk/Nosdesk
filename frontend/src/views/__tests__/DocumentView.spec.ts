import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter, RouterView } from 'vue-router'
import { mountWithProviders } from '@/test/mountWithProviders'
import * as pool from '@nosdesk/core/sync/pool'

const WS_UUID = vi.hoisted(() => '0b6c3f0e-9a1d-4c2e-8f7a-5d4e3c2b1a09')

const docs = vi.hoisted(() => ({
  getPageByPath: vi.fn(),
  getArticleById: vi.fn(),
  getPageSubscription: vi.fn(async () => false),
  getPageStarred: vi.fn(async () => false),
  updatePage: vi.fn(async () => ({})),
}))
vi.mock('@nosdesk/core/services/documentationService', () => ({ default: docs }))
const collections = vi.hoisted(() => ({
  getCollections: vi.fn(),
  setPageCollections: vi.fn(),
}))
vi.mock('@nosdesk/core/services/collectionService', () => collections)
vi.mock('@/stores/collabSession', () => ({
  useCollabSessionStore: () => ({ connectionStatus: {}, connectionBadge: {}, connectionRefusal: {} }),
}))
vi.mock('@/stores/myWorkspaces', () => ({
  useMyWorkspacesStore: () => ({ activeWorkspace: { workspace_uuid: WS_UUID } }),
}))
vi.mock('@/stores/auth', () => ({ useAuthStore: () => ({ isAdmin: true, isTechnician: true }) }))
vi.mock('@/stores/documentationNav', () => ({
  useDocumentationNavStore: () => ({ refreshPages: vi.fn(), updatePageField: vi.fn() }),
}))
vi.mock('@/composables/useTitleManager', () => ({
  useTitleManager: () => ({
    setCustomTitle: vi.fn(),
    onDocumentTitleSave: vi.fn(),
    onDocumentIconSave: vi.fn(),
  }),
}))
vi.mock('@/components/Modal.vue', () => ({
  default: { template: '<div><slot /><slot name="footer" /></div>' },
}))
vi.mock('@/components/documentationComponents/DocumentActionsMenu.vue', () => ({
  default: {
    emits: ['collections'],
    template: '<button type="button" data-test="collections" @click="$emit(\'collections\')" />',
  },
}))
vi.mock('extendable-media-recorder', () => ({ MediaRecorder: class {}, register: async () => {} }))
vi.mock('extendable-media-recorder-wav-encoder', () => ({ connect: async () => ({}) }))
// The editor stub exposes `toolbarDocked` the way the real one does, so the
// view's reaction to the keyboard-docked toolbar can be driven from here.
const editorStub = vi.hoisted(() => ({ docked: { value: false } as { value: boolean } }))
vi.mock('@/components/CollaborativeEditor.vue', async () => {
  const { h, ref } = await import('vue')
  const docked = ref(false)
  editorStub.docked = docked
  return {
    default: {
      setup(_: unknown, { expose }: { expose: (o: Record<string, unknown>) => void }) {
        expose({ toolbarDocked: docked })
        return () => h('div')
      },
    },
  }
})
vi.mock('@/components/common/BackButton.vue', () => ({ default: { template: '<div />' } }))
vi.mock('@/components/common/PullToRefresh.vue', () => ({ default: { template: '<div />' } }))
vi.mock('@/components/editor/RevisionHistory.vue', () => ({ default: { template: '<div />' } }))
vi.mock('@/components/documentationComponents/DocumentInsightsPanel.vue', () => ({ default: { template: '<div />' } }))
vi.mock('@/components/documentationComponents/DocumentAuthorBadge.vue', () => ({ default: { template: '<div />' } }))
vi.mock('@/components/documentationComponents/PageTicketLinksPanel.vue', () => ({ default: { template: '<div />' } }))
vi.mock('@/components/documentationComponents/DocumentationBreadcrumb.vue', () => ({ default: { template: '<div />' } }))
vi.mock('@/components/documentationComponents/MoveDocumentModal.vue', () => ({ default: { template: '<div />' } }))
vi.mock('@/components/documentationComponents/PagePermissionsModal.vue', () => ({ default: { template: '<div />' } }))

import DocumentView from '@/views/DocumentView.vue'
import CollectionManager from '@/components/documentationComponents/CollectionManager.vue'

const PAGE = {
  id: 5,
  uuid: '6f1c2b8e-4d3a-4f5b-9c7d-2e1a0b9c8d7f',
  title: 'Payroll runbook',
  slug: 'payroll-runbook',
  status: 'published',
  content: '',
  icon: null,
  parent_id: null,
}

const ROW = {
  ...PAGE,
  collection_id: 3,
  cover_image: null,
  display_order: null,
  is_public: false,
  is_template: false,
  archived_at: null,
  deleted_at: null,
  created_by: 'u-1',
  last_edited_by: 'u-1',
  verified_by: null,
  verified_at: null,
  verify_interval_days: null,
  created_at: '2026-10-01T00:00:00Z',
  updated_at: '2026-10-01T00:00:00Z',
}

let wrapper: VueWrapper | null = null
const DESKTOP_WIDTH = window.innerWidth
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
  pool.reset()
  vi.clearAllMocks()
  window.innerWidth = DESKTOP_WIDTH
  editorStub.docked.value = false
})

async function openPage(page: typeof PAGE = PAGE): Promise<void> {
  docs.getPageByPath.mockResolvedValue({ id: page.id, title: page.title, slug: page.slug })
  docs.getArticleById.mockResolvedValue({ ...page })
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/documentation', component: { template: '<div />' } },
      { path: '/documentation/:path', name: 'documentation-page', component: DocumentView },
    ],
  })
  await router.push('/documentation/payroll-runbook')
  wrapper = mountWithProviders(RouterView, {}, {}, [[PiniaColada, {}] as never, router])
  await flushPromises()
}

describe('DocumentView, page collections', () => {
  it('ticks the page collection and can take the page out of it', async () => {
    pool.upsert('documentation_page', ROW.id, { ...ROW })
    collections.getCollections.mockResolvedValue([
      { id: 3, uuid: 'c-3', name: 'Payroll', slug: 'payroll', icon: null, color: null, is_system: false, page_count: 1 },
    ])
    collections.setPageCollections.mockResolvedValue([])
    await openPage()

    await wrapper!.find('[data-test="collections"]').trigger('click')
    await flushPromises()

    const manager = wrapper!.findComponent(CollectionManager)
    expect(manager.props('currentCollectionIds')).toEqual([3])

    const row = manager.findAll('button').find((b) => b.text().includes('Payroll'))
    expect(row, 'the collection row').toBeTruthy()
    await row!.trigger('click')
    const save = manager.findAll('button').find((b) => b.text().includes('docs-collection-manager-save'))
    expect(save, 'the save button').toBeTruthy()
    await save!.trigger('click')
    await flushPromises()

    expect(collections.setPageCollections).toHaveBeenCalledWith(5, [])
  })
})

// `useMobileDetection` reads window.innerWidth when its first consumer
// mounts, so the width is set before each mount and the view is the only
// consumer here.
function phoneSlot(): HTMLElement {
  window.innerWidth = 390
  const slot = document.createElement('span')
  slot.id = 'site-header-actions'
  document.body.appendChild(slot)
  return slot
}

describe('DocumentView on a phone', () => {
  it('moves the actions menu into the app bar slot and keeps no toolbar of its own', async () => {
    const slot = phoneSlot()
    await openPage()

    expect(slot.querySelector('[data-test="collections"]'), 'menu in the app bar').toBeTruthy()
    expect(wrapper!.find('[data-test="collections"]').exists(), 'menu in the view').toBe(false)
  })

  it('shows the draft strip for a draft and not for a published page', async () => {
    phoneSlot()
    await openPage({ ...PAGE, status: 'draft' })
    expect(wrapper!.find('[data-test="draft-strip"]').exists()).toBe(true)
    expect(wrapper!.text()).not.toContain('doc-detail-status-draft')

    wrapper!.unmount()
    wrapper = null
    document.body.innerHTML = ''
    phoneSlot()
    await openPage({ ...PAGE, status: 'published' })
    expect(wrapper!.find('[data-test="draft-strip"]').exists()).toBe(false)
    expect(wrapper!.text()).toContain('doc-detail-status-published')
  })

  it('gives the strip up to the keyboard-docked toolbar', async () => {
    phoneSlot()
    await openPage({ ...PAGE, status: 'draft' })
    expect(wrapper!.find('[data-test="draft-strip"]').exists()).toBe(true)

    editorStub.docked.value = true
    await flushPromises()
    expect(wrapper!.find('[data-test="draft-strip"]').exists()).toBe(false)
    // The chip does not pop back in while the keyboard borrows the edge.
    expect(wrapper!.text()).not.toContain('doc-detail-status-draft')

    editorStub.docked.value = false
    await flushPromises()
    expect(wrapper!.find('[data-test="draft-strip"]').exists()).toBe(true)
  })
})

describe('DocumentView on desktop', () => {
  it('keeps the menu in its own toolbar and shows no strip', async () => {
    window.innerWidth = 1280
    const slot = document.createElement('span')
    slot.id = 'site-header-actions'
    document.body.appendChild(slot)
    await openPage({ ...PAGE, status: 'draft' })

    expect(wrapper!.find('[data-test="collections"]').exists()).toBe(true)
    expect(slot.querySelector('[data-test="collections"]')).toBeNull()
    expect(wrapper!.find('[data-test="draft-strip"]').exists()).toBe(false)
    expect(wrapper!.text()).toContain('doc-detail-status-draft')
  })
})

describe('DocumentView title', () => {
  // The title saves 500ms after its blur.
  const afterDebounce = () => new Promise((resolve) => setTimeout(resolve, 600))

  it('saves nothing when a blur leaves the title as it was', async () => {
    window.innerWidth = 1280
    await openPage()
    const title = wrapper!.find('h1[contenteditable]')

    await title.trigger('blur')
    await afterDebounce()
    expect(docs.updatePage).not.toHaveBeenCalled()

    title.element.textContent = 'Payroll runbook 2026'
    await title.trigger('blur')
    await afterDebounce()
    expect(docs.updatePage).toHaveBeenCalledWith(5, { title: 'Payroll runbook 2026', slug: 'payroll-runbook-2026' })

    await title.trigger('blur')
    await afterDebounce()
    expect(docs.updatePage).toHaveBeenCalledTimes(1)
  })
})

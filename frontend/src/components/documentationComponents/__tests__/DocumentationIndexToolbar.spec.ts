import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import * as pool from '@nosdesk/core/sync/pool'
import type { DocPageRow } from '@nosdesk/core/sync/stores/documentation'
import { mountWithProviders } from '@/test/mountWithProviders'

// The band switches between four routes. In the hosted agent app every
// path carries the workspace slug (/acme/documentation/drafts), so both
// routing modes are exercised: the tab must come from the route name.

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

const auth = vi.hoisted(() => ({ admin: false, technician: true }))
vi.mock('@/stores/auth', async () => {
  const { ref } = await import('vue')
  const isAdmin = ref(false)
  const isTechnician = ref(true)
  return {
    useAuthStore: () => {
      isAdmin.value = auth.admin
      isTechnician.value = auth.technician
      return { isAdmin, isTechnician }
    },
  }
})

const portal = vi.hoisted(() => ({ publicDocs: false }))
vi.mock('@/composables/useWorkspacePortal', async () => {
  const { computed } = await import('vue')
  return {
    useWorkspacePortal: () => ({
      portal: computed(() => ({ public_docs_enabled: portal.publicDocs })),
      portalUrl: (p: string) => `https://portal.example${p}`,
    }),
  }
})

const search = vi.hoisted(() => ({ openSearch: vi.fn() }))
vi.mock('@/composables/useGlobalSearch', () => ({ useGlobalSearch: () => search }))

vi.mock('@/composables/useKnowledgeGaps', () => ({
  useDetectClustersMutation: () => ({ mutateAsync: vi.fn(async () => null) }),
}))

const viewport = vi.hoisted(() => ({ phone: false }))
vi.mock('@/composables/useMobileDetection', async () => {
  const { computed, ref } = await import('vue')
  return {
    useMobileDetection: () => ({ isMobile: computed(() => viewport.phone), windowWidth: ref(1280) }),
  }
})

vi.mock('@/components/documentationComponents/CollectionModal.vue', () => ({
  default: defineComponent({
    props: { show: Boolean, mode: String },
    template: '<div v-if="show" data-testid="collection-modal" />',
  }),
}))

import DocumentationIndexToolbar from '@/components/documentationComponents/DocumentationIndexToolbar.vue'
import MenuList, { type MenuItem } from '@/components/common/MenuList.vue'
import {
  installSlugCarrier,
  installWorkspaceGuard,
  withWorkspaceRouting,
} from '@/router/workspaceRouting'

let nextId = 1
function page(status: string, collectionId: number | null): DocPageRow {
  const id = nextId++
  return {
    id,
    uuid: `p-${id}`,
    collection_id: collectionId,
    title: `Page ${id}`,
    slug: `page-${id}`,
    icon: null,
    cover_image: null,
    status,
    parent_id: null,
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
}

function seed(counts: { library?: number; drafts?: number; archived?: number; trash?: number }) {
  const rows = [
    ...Array.from({ length: counts.library ?? 0 }, () => page('published', 1)),
    ...Array.from({ length: counts.drafts ?? 0 }, () => page('published', null)),
    ...Array.from({ length: counts.archived ?? 0 }, () => page('archived', 1)),
    ...Array.from({ length: counts.trash ?? 0 }, () => page('deleted', 1)),
  ]
  for (const r of rows) pool.upsert('documentation_page', r.id, r)
}

function makeRouter(): Router {
  const view = { template: '<div />' }
  const router = createRouter({
    history: createMemoryHistory(),
    routes: withWorkspaceRouting([
      { path: '/documentation', name: 'documentation', component: view, meta: { requiresAuth: true } },
      { path: '/documentation/drafts', name: 'documentation-drafts', component: view, meta: { requiresAuth: true } },
      { path: '/documentation/archived', name: 'documentation-archived', component: view, meta: { requiresAuth: true } },
      { path: '/documentation/trash', name: 'documentation-trash', component: view, meta: { requiresAuth: true } },
      { path: '/documentation/:path', name: 'documentation-page', component: view, meta: { requiresAuth: true } },
      { path: '/admin/guest-access', name: 'admin-guest-access', component: view, meta: { requiresAuth: true } },
    ]),
  })
  installSlugCarrier(router)
  installWorkspaceGuard(router)
  return router
}

let wrapper: VueWrapper | null = null
let router: Router

async function mountAt(path: string) {
  router = makeRouter()
  await router.push(path)
  await router.isReady()
  wrapper = mountWithProviders(DocumentationIndexToolbar, {}, {}, [router])
  await flushPromises()
}

const tabs = () => wrapper!.findAll('[role="tab"]')
function tabName(tab: ReturnType<typeof tabs>[number]): string {
  return tab.find('span').text().trim()
}
function tabLabel(tab: ReturnType<typeof tabs>[number]): string {
  const count = tab.find('[data-testid="docs-tab-count"]')
  return count.exists() ? `${tabName(tab)} ${count.text().trim()}` : tabName(tab)
}
const tabLabels = () => tabs().map(tabLabel)
const selected = () =>
  tabs()
    .filter((t) => t.attributes('aria-selected') === 'true')
    .map(tabName)

async function clickTab(label: string) {
  const tab = tabs().find((t) => tabName(t) === label)
  if (!tab) throw new Error(`no tab ${label}`)
  await tab.trigger('mousedown', { button: 0, ctrlKey: false })
  await tab.trigger('click')
  await flushPromises()
}

async function openMenu(): Promise<MenuItem[]> {
  await wrapper!.find('[data-testid="docs-index-more"]').trigger('click')
  await flushPromises()
  return wrapper!.findComponent(MenuList).props('items') as MenuItem[]
}

beforeEach(() => {
  routing = 'host'
  slug = null
  auth.admin = false
  auth.technician = true
  portal.publicDocs = false
  viewport.phone = false
  nextId = 1
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  pool.reset()
  vi.clearAllMocks()
})

const LIB = 'docs-index-tab-library'
const DRAFTS = 'docs-index-tab-drafts'
const ARCHIVED = 'docs-index-tab-archived'
const TRASH = 'docs-index-tab-trash'

describe('DocumentationIndexToolbar tabs', () => {
  it('shows Library, Drafts, Archived and Trash with their counts', async () => {
    seed({ library: 5, drafts: 2, archived: 3, trash: 1 })
    await mountAt('/documentation')
    expect(tabLabels()).toEqual([`${LIB} 5`, `${DRAFTS} 2`, `${ARCHIVED} 3`, `${TRASH} 1`])
  })

  it('hides Archived and Trash at zero and drops a zero count from Drafts', async () => {
    seed({ library: 4 })
    await mountAt('/documentation')
    expect(tabLabels()).toEqual([`${LIB} 4`, DRAFTS])
  })

  it('keeps an empty Archived or Trash tab while that view is open', async () => {
    seed({ library: 1 })
    await mountAt('/documentation/trash')
    expect(tabLabels()).toEqual([`${LIB} 1`, DRAFTS, TRASH])
    expect(selected()).toEqual([TRASH])
  })

  it('updates the counts as pages move between states', async () => {
    seed({ library: 1, drafts: 1 })
    await mountAt('/documentation')
    const row = pool.get<DocPageRow>('documentation_page', 1)!
    pool.upsert('documentation_page', 1, { ...row, status: 'deleted' })
    await flushPromises()
    expect(tabLabels()).toEqual([LIB, `${DRAFTS} 1`, `${TRASH} 1`])
  })
})

const modes = [
  { mode: 'host' as const, prefix: '' },
  { mode: 'path' as const, prefix: '/acme' },
]

const ROUTES = [
  { label: LIB, path: '/documentation', name: 'documentation' },
  { label: DRAFTS, path: '/documentation/drafts', name: 'documentation-drafts' },
  { label: ARCHIVED, path: '/documentation/archived', name: 'documentation-archived' },
  { label: TRASH, path: '/documentation/trash', name: 'documentation-trash' },
]

describe.each(modes)('DocumentationIndexToolbar navigation ($mode routing)', ({ mode, prefix }) => {
  beforeEach(() => {
    routing = mode
    seed({ library: 3, drafts: 2, archived: 1, trash: 1 })
  })

  it.each(ROUTES)('selects $label on $path', async ({ label, path }) => {
    await mountAt(`${prefix}${path}`)
    expect(selected()).toEqual([label])
  })

  it.each(ROUTES.flatMap((from) => ROUTES.filter((to) => to !== from).map((to) => ({ from, to }))))(
    'switches from $from.label to $to.label',
    async ({ from, to }) => {
      await mountAt(`${prefix}${from.path}`)
      await clickTab(to.label)
      expect(router.currentRoute.value.name).toBe(to.name)
      expect(router.currentRoute.value.path).toBe(`${prefix}${to.path}`)
      expect(selected()).toEqual([to.label])
    },
  )

  it('walks every tab forward and back on one mounted band', async () => {
    await mountAt(`${prefix}/documentation`)
    const order = [...ROUTES, ...ROUTES.slice(0, -1).reverse()]
    for (const r of order.slice(1)) {
      await clickTab(r.label)
      expect(router.currentRoute.value.path).toBe(`${prefix}${r.path}`)
      expect(selected()).toEqual([r.label])
    }
  })
})

describe('DocumentationIndexToolbar actions', () => {
  it('opens the documentation-scoped search', async () => {
    await mountAt('/documentation')
    await wrapper!.find('[data-testid="docs-index-search"]').trigger('click')
    expect(search.openSearch).toHaveBeenCalledWith('documentation')
  })

  it('opens the new collection modal from the band', async () => {
    await mountAt('/documentation')
    expect(wrapper!.find('[data-testid="collection-modal"]').exists()).toBe(false)
    await wrapper!.find('[data-testid="docs-index-new-collection"]').trigger('click')
    expect(wrapper!.find('[data-testid="collection-modal"]').exists()).toBe(true)
  })

  it('groups the menu as Knowledge, Publishing and Admin for an admin', async () => {
    auth.admin = true
    portal.publicDocs = true
    await mountAt('/documentation')
    const items = await openMenu()
    expect(items.map((i) => i.id)).toEqual([
      'knowledge-heading',
      'scan-gaps',
      'publish-heading',
      'public-site',
      'admin-heading',
      'guest-settings',
    ])
  })

  it('leaves the Admin group out for an agent, and Publishing when public docs are off', async () => {
    await mountAt('/documentation')
    const items = await openMenu()
    expect(items.map((i) => i.id)).toEqual(['knowledge-heading', 'scan-gaps'])
  })

  it('keeps the menu when archive and trash are empty', async () => {
    await mountAt('/documentation')
    expect(wrapper!.find('[data-testid="docs-index-more"]').exists()).toBe(true)
  })

  it('sends Guest access settings to the admin page by name', async () => {
    auth.admin = true
    await mountAt('/documentation')
    await openMenu()
    wrapper!.findComponent(MenuList).vm.$emit('select', 'guest-settings')
    await flushPromises()
    expect(router.currentRoute.value.name).toBe('admin-guest-access')
  })
})

describe('DocumentationIndexToolbar on a phone', () => {
  beforeEach(() => {
    viewport.phone = true
  })

  it('drops search and folds New collection into the menu sheet', async () => {
    seed({ library: 2, drafts: 1, archived: 1, trash: 1 })
    await mountAt('/documentation')
    expect(wrapper!.find('[data-testid="docs-index-search"]').exists()).toBe(false)
    expect(wrapper!.find('[data-testid="docs-index-new-collection"]').exists()).toBe(false)
    expect(tabLabels()).toHaveLength(4)

    const items = await openMenu()
    expect(items[0].id).toBe('new-collection')
    expect(items[1]).toMatchObject({ id: 'knowledge-heading', divider: true })

    wrapper!.findComponent(MenuList).vm.$emit('select', 'new-collection')
    await flushPromises()
    expect(wrapper!.find('[data-testid="collection-modal"]').exists()).toBe(true)
  })

  it('lets the tab strip scroll inside the band', async () => {
    await mountAt('/documentation')
    const list = wrapper!.find('[role="tablist"]')
    expect(list.classes()).toContain('overflow-x-auto')
  })
})


import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import type { DocsImportJob, DocsPageRow } from '@nosdesk/core/services/docsImportService'

import { mountWithProviders } from '@/test/mountWithProviders'

const api = vi.hoisted(() => ({ get: vi.fn(), fields: vi.fn(), allPages: vi.fn(), list: vi.fn(), answers: vi.fn() }))
vi.mock('@nosdesk/core/services/importService', () => ({
  importJobQueryKey: (id: string) => ['import-job', id],
  importFieldsQueryKey: (type: string) => ['import-fields', type],
  importJobsQueryKey: (limit: number) => ['import-jobs', limit],
  importAnswersQueryKey: (type: string) => ['import-answers', type],
  importService: {
    get: (...a: unknown[]) => api.get(...a),
    fields: (...a: unknown[]) => api.fields(...a),
    list: (...a: unknown[]) => api.list(...a),
    answers: (...a: unknown[]) => api.answers(...a),
  },
}))
vi.mock('@nosdesk/core/services/docsImportService', async (importOriginal) => {
  const real = await importOriginal<typeof import('@nosdesk/core/services/docsImportService')>()
  return { ...real, docsImportService: { allPages: (...a: unknown[]) => api.allPages(...a) } }
})
vi.mock('@nosdesk/core/sync/stores/documentation', () => ({
  isActivePage: () => true,
  useSyncDocsStore: () => ({ allCollections: [{ id: 7, name: 'IT Handbook', slug: 'it-handbook' }], allPages: [] }),
}))
vi.mock('@nosdesk/core/services/webhookService', () => ({ default: { listWebhooks: vi.fn() } }))
vi.mock('@/composables/useAssetKindsQuery', async () => {
  const { computed } = await import('vue')
  return { useAssetKindsQuery: () => ({ kinds: computed(() => []) }) }
})

import ImportDetailView from '@/views/ImportDetailView.vue'
import DataImportView from '@/views/DataImportView.vue'

const PAGES = [
  { index: 0, title: 'Network', path: 'Network', file: null, folder: 'Network', parent: null, order: 0, collection: 0 },
  { index: 1, title: 'VPN', path: 'Network/VPN.md', file: 'Network/VPN.md', folder: null, parent: 0, order: 1, collection: null },
  { index: 2, title: 'Printers', path: 'Network/Printers.md', file: 'Network/Printers.md', folder: null, parent: 0, order: 2, collection: null },
]

function docsJob(over: Partial<DocsImportJob> = {}): DocsImportJob {
  return {
    id: 'job-1',
    job_type: 'documentation',
    status: 'done',
    state: 'imported',
    pruned_at: null,
    created_by_name: 'Kyle Phillips',
    filename: 'IT-Handbook.zip',
    created_by: 'u-1',
    created_at: '2026-10-19T00:00:00Z',
    updated_at: '2026-10-19T00:00:00Z',
    completed_at: '2026-10-19T00:00:00Z',
    summary: {
      kind: 'documentation',
      row_count: 3,
      would_create: 2,
      would_update: 0,
      unchanged: 0,
      rows_with_errors: 1,
      skipped: 1,
      duplicates: 1,
      errors: 0,
      images: 4,
      image_problems: 0,
      web_images: 0,
      links_rewritten: 3,
      notes: [],
      section_pages: 1,
      left_out: 0,
      front_matter_not_imported: {},
      profile: 'notion',
      detected: null,
      destination: { mode: 'existing', new_collections: [], collection_id: 7, collection_name: 'IT Handbook', under_page_id: null },
      status: 'drafts',
      duplicates_choice: 'skip',
      web_images_choice: 'download',
    },
    records_committed: 2,
    error_message: null,
    source: {
      kind: 'documentation',
      size: 42 * 1024 * 1024,
      files: 1204,
      tree: { profile: 'notion', detected: null, collections: [], pages: PAGES, section_pages: 1, images: 4, web_images: 0, left_out: 0 },
    },
    options: { collections: 'existing', collection_id: 7, leave_out: ['**/archive/**', '/Network/Old.md'] },
    ...over,
  }
}

const ROWS: DocsPageRow[] = [
  { index: 0, title: 'Network', path: 'Network', parent: null, outcome: 'new', notes: {}, links: {} },
  { index: 1, title: 'VPN', path: 'Network/VPN.md', parent: 0, outcome: 'new', notes: { callouts_as_quotes: 1 }, links: {} },
  { index: 2, title: 'Printers', path: 'Network/Printers.md', parent: 0, outcome: 'skipped', duplicate: { page_id: 1, page_uuid: 'u', by: 'source' }, notes: {}, links: {} },
]

let wrapper: VueWrapper | null = null
let router: Router

async function mountAt(component: object, path: string) {
  const view = { template: '<div />' }
  router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/admin/data-import', name: 'admin-data-import', component: view },
      { path: '/admin/data-import/imports/:id', name: 'admin-import-detail', component: view },
      { path: '/admin/data-import/documentation', name: 'admin-docs-import', component: view },
      { path: '/documentation', name: 'documentation', component: view },
      { path: '/documentation/collections/:slug', name: 'collection-view', component: view },
      { path: '/:any(.*)*', component: view },
    ],
  })
  await router.push(path)
  await router.isReady()
  wrapper = mountWithProviders(component, {}, {}, [[PiniaColada, {}] as never, router])
  await flushPromises()
}

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.clearAllMocks()
})

describe('ImportDetailView for a documentation import', () => {
  it('shows its facts, counts and the tree with each outcome, without the spreadsheet reads', async () => {
    api.get.mockResolvedValue(docsJob())
    api.allPages.mockResolvedValue(ROWS)
    await mountAt(ImportDetailView, '/admin/data-import/imports/job-1')
    expect(api.fields).not.toHaveBeenCalled()
    expect(wrapper!.find('[data-test="docs-import-detail"]').exists()).toBe(true)
    expect(wrapper!.find('[data-test="import-detail-title"]').text()).toBe('IT-Handbook.zip')
    const facts = wrapper!.find('[data-test="import-detail-facts"]').text()
    expect(facts).toContain('docs-import-detail-state')
    expect(facts).toContain('docs-import-detail-left-out')
    expect(wrapper!.findAll('[data-test="docs-tree-outcome"]').map((p) => p.text())).toEqual([
      'docs-import-outcome-new',
      'docs-import-outcome-new',
      'docs-import-outcome-skipped',
    ])
    await wrapper!.find('[data-test="docs-detail-open"]').trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.params.slug).toBe('it-handbook')
  })

  it('keeps the counts and options once the detail is removed', async () => {
    api.get.mockResolvedValue(docsJob({ pruned_at: '2026-11-19T00:00:00Z' }))
    await mountAt(ImportDetailView, '/admin/data-import/imports/job-1')
    expect(api.allPages).not.toHaveBeenCalled()
    expect(wrapper!.find('[data-test="docs-detail-tiles"]').exists()).toBe(true)
    expect(wrapper!.find('[data-test="docs-detail-tree"]').exists()).toBe(false)
    expect(wrapper!.find('[data-test="import-detail-report"]').exists()).toBe(false)
    expect(wrapper!.find('[data-test="import-detail-kept"]').text()).toBe('import-detail-kept-30-days')
  })

  it('sends a draft to the documentation wizard', async () => {
    api.get.mockResolvedValue(docsJob({ state: 'draft', status: 'parsed' }))
    await mountAt(ImportDetailView, '/admin/data-import/imports/job-1')
    expect(router.currentRoute.value.name).toBe('admin-docs-import')
    expect(router.currentRoute.value.query.job).toBe('job-1')
  })
})

describe('the Data import hub', () => {
  it('has a Documentation card that opens the documentation import', async () => {
    api.list.mockResolvedValue([])
    api.answers.mockResolvedValue({ job_type: 'assets', entries: [], columns: null })
    await mountAt(DataImportView, '/admin/data-import')
    const card = wrapper!.findAll('a').find((a) => a.text().includes('admin-data-import-docs-title'))!
    expect(card.attributes('href')).toBe('/admin/data-import/documentation')
    expect(card.text()).toContain('admin-data-import-status-new')
    expect(card.text()).toContain('admin-data-import-docs-description')
  })
})

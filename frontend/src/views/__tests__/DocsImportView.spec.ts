import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import type {
  DocsImportJob,
  DocsPageRow,
  DocsSummary,
  DocsTreePage,
} from '@nosdesk/core/services/docsImportService'

import { mountWithProviders } from '@/test/mountWithProviders'

const api = vi.hoisted(() => ({
  get: vi.fn(),
  upload: vi.fn(),
  putOptions: vi.fn(),
  allPages: vi.fn(),
  preview: vi.fn(),
  report: vi.fn(),
  commit: vi.fn(),
  remove: vi.fn(),
  publishPlan: vi.fn(),
  publishAll: vi.fn(),
  removalPlan: vi.fn(),
  removePages: vi.fn(),
}))

vi.mock('@nosdesk/core/services/docsImportService', async (importOriginal) => {
  const real = await importOriginal<typeof import('@nosdesk/core/services/docsImportService')>()
  return {
    ...real,
    docsImportService: {
      get: (...a: unknown[]) => api.get(...a),
      upload: (...a: unknown[]) => api.upload(...a),
      putOptions: (...a: unknown[]) => api.putOptions(...a),
      allPages: (...a: unknown[]) => api.allPages(...a),
      preview: (...a: unknown[]) => api.preview(...a),
      report: (...a: unknown[]) => api.report(...a),
      commit: (...a: unknown[]) => api.commit(...a),
      publishPlan: (...a: unknown[]) => api.publishPlan(...a),
      publishAll: (...a: unknown[]) => api.publishAll(...a),
      removalPlan: (...a: unknown[]) => api.removalPlan(...a),
      removePages: (...a: unknown[]) => api.removePages(...a),
    },
  }
})
vi.mock('@nosdesk/core/services/importService', () => ({
  importService: { remove: (...a: unknown[]) => api.remove(...a) },
}))

const store = vi.hoisted(() => ({
  collections: [] as { id: number; name: string; slug: string; is_system: boolean }[],
}))
vi.mock('@nosdesk/core/sync/stores/documentation', () => ({
  isActivePage: () => true,
  useSyncDocsStore: () => ({
    get allCollections() {
      return store.collections
    },
    allPages: [],
  }),
}))

vi.mock('@/components/common/ConfirmModal.vue', () => ({
  default: {
    props: ['show', 'title', 'message', 'confirmLabel'],
    emits: ['confirm', 'close'],
    template: `<div v-if="show" data-test="confirm"><p data-test="confirm-message">{{ message }}</p><button type="button" data-test="confirm-ok" @click="$emit('confirm')">{{ confirmLabel }}</button></div>`,
  },
}))

import DocsImportView from '@/views/DocsImportView.vue'

function page(index: number, title: string, parent: number | null, file: string | null, folder: string | null = null): DocsTreePage {
  return { index, title, path: file ?? folder!, file, folder, parent, order: index, collection: parent == null ? 0 : null }
}
const TREE = [
  page(0, 'Onboarding', null, 'Onboarding.md', 'Onboarding'),
  page(1, 'Day one', 0, 'Onboarding/Day one.md'),
  page(2, 'Laptop', 0, 'Onboarding/Laptop.md'),
  page(3, 'VPN', null, 'VPN.md'),
]

const SUMMARY: DocsSummary = {
  kind: 'documentation',
  row_count: 4,
  would_create: 3,
  would_update: 0,
  unchanged: 0,
  rows_with_errors: 1,
  skipped: 1,
  duplicates: 1,
  errors: 0,
  images: 2,
  image_problems: 0,
  web_images: 0,
  links_rewritten: 5,
  notes: [{ code: 'callouts_as_quotes', count: 2, pages: 1 }],
  section_pages: 0,
  left_out: 0,
  front_matter_not_imported: {},
  profile: 'notion',
  detected: null,
  destination: { mode: 'new', new_collections: ['IT Handbook'], collection_id: null, collection_name: null, under_page_id: null },
  status: 'drafts',
  duplicates_choice: 'skip',
  web_images_choice: 'download',
}

const ROWS: DocsPageRow[] = TREE.map((p) => ({
  index: p.index,
  path: p.path,
  title: p.title,
  parent: p.parent,
  outcome: p.index === 3 ? 'skipped' : 'new',
  duplicate: p.index === 3 ? { page_id: 9, page_uuid: 'u9', by: 'source' } : undefined,
  notes: p.index === 1 ? { callouts_as_quotes: 2 } : {},
  links: { rewritten: 1 },
}))

function job(status: DocsImportJob['status'], over: Partial<DocsImportJob> = {}): DocsImportJob {
  const checked = !['parsed', 'checking'].includes(status)
  return {
    id: 'job-1',
    job_type: 'documentation',
    status,
    state: status === 'done' ? 'imported' : 'draft',
    pruned_at: null,
    filename: 'IT-Handbook.zip',
    created_by: null,
    created_at: '2026-10-19T00:00:00Z',
    updated_at: `2026-10-19T00:00:0${status.length % 10}Z`,
    completed_at: null,
    summary: checked ? SUMMARY : null,
    records_committed: status === 'committing' ? 1 : status === 'done' ? 3 : null,
    error_message: null,
    source: {
      kind: 'documentation',
      size: 1024,
      files: 6,
      tree: { profile: 'notion', detected: null, collections: ['IT Handbook'], pages: TREE, section_pages: 0, images: 2, web_images: 0, left_out: 0 },
    },
    options: checked ? { duplicates: 'skip' } : {},
    ...over,
  }
}

let wrapper: VueWrapper | null = null
let router: Router

async function mountAt(path: string) {
  const view = { template: '<div />' }
  router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/admin/data-import/documentation', name: 'admin-docs-import', component: view },
      { path: '/admin/data-import', name: 'admin-data-import', component: view },
      { path: '/admin/data-import/csv', name: 'admin-csv-import', component: view },
      { path: '/admin/data-import/imports/:id', name: 'admin-import-detail', component: view },
      { path: '/documentation', name: 'documentation', component: view },
      { path: '/documentation/collections/:slug', name: 'collection-view', component: view },
      { path: '/documentation/:path', name: 'documentation-page', component: view },
    ],
  })
  await router.push(path)
  await router.isReady()
  wrapper = mountWithProviders(DocsImportView, {}, {}, [[PiniaColada, {}] as never, router])
  await flushPromises()
}

const find = (sel: string) => wrapper!.find(`[data-test="${sel}"]`)

beforeEach(() => {
  store.collections = []
  api.allPages.mockResolvedValue(ROWS)
  api.preview.mockResolvedValue({ index: 0, title: 'Onboarding', doc: { type: 'doc', content: [{ type: 'paragraph' }] }, images: {} })
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.useRealTimers()
  vi.clearAllMocks()
})

describe('DocsImportView: Start', () => {
  it('uploads the dropped zip and opens Pages', async () => {
    await mountAt('/admin/data-import/documentation')
    expect(find('docs-start').exists()).toBe(true)
    api.upload.mockResolvedValue(job('parsed'))
    const zip = new File(['PK'], 'IT-Handbook.zip', { type: 'application/zip' })
    await find('docs-drop').trigger('drop', { dataTransfer: { files: [zip] } })
    await flushPromises()
    expect(api.upload).toHaveBeenCalledWith([zip], expect.any(Function))
    expect(find('docs-pages').exists()).toBe(true)
    expect(router.currentRoute.value.query.job).toBe('job-1')
  })

  it('refuses a mix of files, and a file over the cap, before uploading', async () => {
    await mountAt('/admin/data-import/documentation')
    await find('docs-drop').trigger('drop', {
      dataTransfer: { files: [new File(['x'], 'a.md'), new File(['x'], 'b.pdf')] },
    })
    expect(find('docs-upload-error').text()).toContain('docs-import-refused-mixed-files')
    const big = new File(['x'], 'big.zip', { type: 'application/zip' })
    Object.defineProperty(big, 'size', { value: 300 * 1024 * 1024 })
    await find('docs-drop').trigger('drop', { dataTransfer: { files: [big] } })
    expect(find('docs-upload-error').text()).toContain('docs-import-refused-too-large')
    expect(api.upload).not.toHaveBeenCalled()
  })

  it.each([
    ['docs_import_no_markdown', 'docs-import-refused-no-markdown'],
    ['archive_encrypted', 'docs-import-refused-encrypted'],
    ['docs_import_too_many_pages', 'docs-import-refused-too-many-pages'],
    ['docs_import_source_later', 'docs-import-refused-source-later'],
    ['archive_unreadable', 'docs-import-refused-not-a-zip'],
    ['upload_too_large', 'docs-import-refused-too-large'],
  ])('words the refusal %s', async (code, key) => {
    await mountAt('/admin/data-import/documentation')
    api.upload.mockRejectedValue({ response: { status: 400, data: { code, error: 'English', args: {} } } })
    await find('docs-drop').trigger('drop', { dataTransfer: { files: [new File(['PK'], 'x.zip')] } })
    await flushPromises()
    expect(find('docs-upload-error').text()).toContain(key)
    expect(find('docs-start').exists()).toBe(true)
  })
})

describe('DocsImportView: Pages', () => {
  it('resumes a parsed job on Pages and selects an existing collection of the same name', async () => {
    store.collections = [{ id: 7, name: 'IT Handbook', slug: 'it-handbook', is_system: false }]
    api.get.mockResolvedValue(job('parsed'))
    await mountAt('/admin/data-import/documentation?job=job-1')
    expect(find('docs-pages').exists()).toBe(true)
    expect(find('docs-where-suggested').exists()).toBe(true)
    expect(find('docs-where-mode-existing').find('input').element.checked).toBe(true)
    expect(find('docs-check').text()).toContain('docs-import-check')
  })

  it('checks the ticked pages, with unticked ones left out by index, then opens Review', async () => {
    vi.useFakeTimers()
    api.get.mockResolvedValueOnce(job('parsed'))
    await mountAt('/admin/data-import/documentation?job=job-1')
    // Untick Onboarding: its pages follow.
    await wrapper!.findAll('[data-test="docs-tree-tick"]')[0].trigger('change')
    expect(find('docs-tree-count').text()).toContain('docs-import-tree-count-unticked')

    api.putOptions.mockResolvedValue(job('checking'))
    api.get.mockResolvedValue(job('dry_run_done'))
    await find('docs-check').trigger('click')
    await flushPromises()
    const [, options, check] = api.putOptions.mock.calls[0]
    expect(check).toBe(true)
    expect(options).toMatchObject({ collections: 'new', status: 'drafts', leave_out: [], exclude_pages: [0] })
    expect(find('docs-checking').exists()).toBe(true)

    await vi.advanceTimersByTimeAsync(1600)
    await flushPromises()
    expect(find('docs-review').exists()).toBe(true)
  })

  it('stays on Pages with the reason when the check stops', async () => {
    vi.useFakeTimers()
    api.get.mockResolvedValueOnce(job('parsed'))
    await mountAt('/admin/data-import/documentation?job=job-1')
    api.putOptions.mockResolvedValue(job('checking'))
    api.get.mockResolvedValue(job('parsed', { error_message: 'Nothing was imported. The zip is damaged.' }))
    await find('docs-check').trigger('click')
    await vi.advanceTimersByTimeAsync(1600)
    await flushPromises()
    expect(find('docs-pages').exists()).toBe(true)
    expect(find('docs-import-error').text()).toContain('The zip is damaged')
  })

  it('reads the tree again as another profile, without patterns', async () => {
    api.get.mockResolvedValue(job('parsed'))
    await mountAt('/admin/data-import/documentation?job=job-1')
    api.putOptions.mockResolvedValue(job('parsed'))
    await find('docs-source-profile').setValue('obsidian')
    await flushPromises()
    const [, options, check] = api.putOptions.mock.calls[0]
    expect(check).toBe(false)
    expect(options).toMatchObject({ profile: 'obsidian', leave_out: [], exclude_pages: [] })
  })
})

describe('DocsImportView: Review', () => {
  it('counts the plan and recomputes it when pages already in Nosdesk are imported as copies', async () => {
    api.get.mockResolvedValue(job('dry_run_done'))
    await mountAt('/admin/data-import/documentation?job=job-1')
    expect(find('docs-review-headline').text()).toContain('docs-import-headline-skipped')
    expect(find('docs-review-import').text()).toContain('docs-import-review-import')
    expect(wrapper!.findAll('[data-test="docs-list-row"]')).toHaveLength(4)
    expect(find('docs-review-already').exists()).toBe(true)

    await find('docs-review-already').findAll('[role="radio"]')[1].trigger('click')
    await flushPromises()
    expect(find('docs-review-headline').text()).not.toContain('docs-import-headline-skipped')
    expect(wrapper!.findAll('[data-test="docs-list-outcome"]').map((p) => p.text())).toContain('docs-import-outcome-copy')

    api.commit.mockResolvedValue(job('committing'))
    api.get.mockResolvedValue(job('done'))
    await find('docs-review-import').trigger('click')
    expect(find('confirm-message').text()).toContain('docs-import-confirm-copies')
    await find('confirm-ok').trigger('click')
    await flushPromises()
    expect(api.commit).toHaveBeenCalledWith('job-1', 'copy')
    expect(find('docs-done-importing').exists()).toBe(true)
  })

  it('filters by a conversion note with Show pages', async () => {
    api.get.mockResolvedValue(job('dry_run_done'))
    await mountAt('/admin/data-import/documentation?job=job-1')
    await find('docs-show-callouts_as_quotes').trigger('click')
    await flushPromises()
    expect(find('docs-list-chip').exists()).toBe(true)
    expect(wrapper!.findAll('[data-test="docs-list-row"]').map((r) => r.text())).toEqual([
      expect.stringContaining('Day one'),
    ])
    await find('docs-list-chip-clear').trigger('click')
    expect(wrapper!.findAll('[data-test="docs-list-row"]')).toHaveLength(4)
  })

  it('heads the preview with the path below the archive root, ids shortened', async () => {
    const rooted = job('dry_run_done')
    rooted.source!.tree.root = 'Onboarding'
    api.get.mockResolvedValue(rooted)
    api.allPages.mockResolvedValue(
      ROWS.map((r) => (r.index === 0 ? { ...r, path: 'Onboarding/Day one 9f3c1e2d4b5a6c7d8e9f0a1b2c3d4e5f.md' } : r)),
    )
    await mountAt('/admin/data-import/documentation?job=job-1')
    await wrapper!.findAll('[data-test="docs-list-row"]')[0].trigger('click')
    await flushPromises()
    const paths = wrapper!.findAll('[data-test="docs-preview-path"]').map((p) => p.text())
    expect(paths).toContain('Day one 9f3c1e2d.md')
  })

  it('shows a changed plan in place when the commit is refused', async () => {
    api.get.mockResolvedValue(job('dry_run_done'))
    await mountAt('/admin/data-import/documentation?job=job-1')
    const moved = ROWS.map((r) => (r.index === 2 ? { ...r, duplicate: { page_id: 4, page_uuid: 'u4', by: 'source' as const } } : r))
    api.allPages.mockResolvedValue(moved)
    api.commit.mockRejectedValue({
      response: { status: 409, data: { code: 'import_plan_changed', error: 'changed', job: job('dry_run_done', { updated_at: '2026-10-19T01:00:00Z' }) } },
    })
    await find('docs-review-import').trigger('click')
    await find('confirm-ok').trigger('click')
    await flushPromises()
    expect(find('docs-plan-changed').text()).toContain('docs-import-plan-changed-more')
    expect(find('docs-list-chip').text()).toContain('docs-import-chip-changed')
    expect(find('docs-review').exists()).toBe(true)
  })
})

describe('DocsImportView: Done', () => {
  it('follows a commit still running, with the pages written so far, to the end', async () => {
    vi.useFakeTimers()
    api.get.mockResolvedValueOnce(job('committing'))
    await mountAt('/admin/data-import/documentation?job=job-1')
    expect(find('docs-done-progress').text()).toContain('docs-import-importing-progress')
    api.get.mockResolvedValue(job('done'))
    await vi.advanceTimersByTimeAsync(2100)
    await flushPromises()
    expect(find('docs-done').exists()).toBe(true)
    expect(find('docs-done-headline').text()).toContain('docs-import-done-summary')
  })

  it('goes back to Review with the reason when the commit wrote nothing', async () => {
    vi.useFakeTimers()
    api.get.mockResolvedValueOnce(job('committing'))
    await mountAt('/admin/data-import/documentation?job=job-1')
    api.get.mockResolvedValue(job('dry_run_done', { error_message: 'Nothing was imported. Storage is full.' }))
    await vi.advanceTimersByTimeAsync(2100)
    await flushPromises()
    expect(find('docs-review').exists()).toBe(true)
    expect(find('docs-import-error').text()).toContain('Storage is full')
  })

  it('offers Publish all and Remove imported pages, and shows the removal in place', async () => {
    api.get.mockResolvedValue(job('done', { removal_closes_at: '2099-11-18T00:00:00Z' }))
    api.publishPlan.mockResolvedValue({ publishable: 3, left: 0 })
    await mountAt('/admin/data-import/documentation?job=job-1')
    expect(find('docs-publish-button').text()).toBe('docs-import-publish-button')
    expect(find('docs-remove-window').text()).toBe('docs-import-remove-window')
    expect(find('docs-done').text()).toContain('docs-import-done-drafts')

    api.removalPlan.mockResolvedValue({ created: 3, deletable: 3, gone: 0, kept: [], collections: { deleted: [], kept: [] } })
    await find('docs-remove-button').trigger('click')
    await flushPromises()
    expect(find('confirm-message').text()).toContain('docs-import-remove-collection-if-empty')
    api.removePages.mockResolvedValue(
      job('done', {
        removal_closes_at: '2099-11-18T00:00:00Z',
        removed_at: '2026-10-21T00:00:00Z',
        remove_result: {
          kind: 'documentation',
          created: 3,
          deleted: 3,
          gone: 0,
          deleted_ids: [1, 2, 3],
          kept: [],
          collections: { deleted: [{ id: 3, name: 'IT Handbook', pages: 0 }], kept: [] },
        },
      }),
    )
    await find('confirm-ok').trigger('click')
    await flushPromises()
    expect(api.removePages).toHaveBeenCalledWith('job-1')
    expect(find('docs-remove-result').text()).toContain('docs-import-remove-result-collection-deleted')
    expect(find('docs-publish-button').exists()).toBe(false)
    expect(find('docs-done').text()).not.toContain('docs-import-done-drafts')
  })

  it('opens the collection the pages went into', async () => {
    store.collections = [{ id: 3, name: 'IT Handbook', slug: 'it-handbook', is_system: false }]
    api.get.mockResolvedValue(job('done'))
    await mountAt('/admin/data-import/documentation?job=job-1')
    await find('docs-done-open').trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.name).toBe('collection-view')
    expect(router.currentRoute.value.params.slug).toBe('it-handbook')
  })
})

describe('DocsImportView: resume', () => {
  it('restores the unticked pages and typed patterns a check was sent with', async () => {
    api.get.mockResolvedValue(job('parsed', { options: { exclude_pages: [0], leave_out: ['**/archive/**'] } }))
    await mountAt('/admin/data-import/documentation?job=job-1')
    const ticks = wrapper!.findAll<HTMLInputElement>('[data-test="docs-tree-tick"]')
    expect(ticks[0].element.checked).toBe(false)
    expect(ticks[ticks.length - 1].element.checked).toBe(true)
    expect(find('docs-pages').text()).toContain('**/archive/**')

    api.putOptions.mockResolvedValue(job('checking'))
    api.get.mockResolvedValue(job('checking'))
    await find('docs-check').trigger('click')
    await flushPromises()
    expect(api.putOptions.mock.calls[0][1]).toMatchObject({ exclude_pages: [0], leave_out: ['**/archive/**'] })
  })

  it('sends an import past its 30 days to its detail page, and another type to its wizard', async () => {
    api.get.mockResolvedValue(job('done', { pruned_at: '2026-11-20T00:00:00Z' }))
    await mountAt('/admin/data-import/documentation?job=job-1')
    expect(router.currentRoute.value.name).toBe('admin-import-detail')
    wrapper!.unmount()
    api.get.mockResolvedValue({ ...job('parsed'), job_type: 'assets' })
    await mountAt('/admin/data-import/documentation?job=job-1')
    expect(router.currentRoute.value.path).toBe('/admin/data-import/csv')
  })

  it('says an expired draft or a missing job cannot be resumed', async () => {
    api.get.mockResolvedValue(job('parsed', { state: 'expired' }))
    await mountAt('/admin/data-import/documentation?job=job-1')
    expect(find('docs-import-error').text()).toContain('import-history-expired-resume')
    wrapper!.unmount()
    api.get.mockRejectedValue({ response: { status: 404 } })
    await mountAt('/admin/data-import/documentation?job=job-2')
    expect(find('docs-import-error').text()).toContain('import-resume-failed')
    expect(find('docs-start').exists()).toBe(true)
  })
})

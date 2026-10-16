import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { ImportField, ImportJob } from '@nosdesk/core/services/importService'

const api = vi.hoisted(() => ({
  get: vi.fn(),
  fields: vi.fn(),
  rows: vi.fn(),
  errorReport: vi.fn(),
  previousValues: vi.fn(),
  nextSheet: vi.fn(),
}))
vi.mock('@nosdesk/core/services/importService', () => ({
  importJobQueryKey: (id: string) => ['import-job', id],
  importFieldsQueryKey: (type: string) => ['import-fields', type],
  importService: {
    get: (...a: unknown[]) => api.get(...a),
    fields: (...a: unknown[]) => api.fields(...a),
    rows: (...a: unknown[]) => api.rows(...a),
    errorReport: (...a: unknown[]) => api.errorReport(...a),
    previousValues: (...a: unknown[]) => api.previousValues(...a),
    nextSheet: (...a: unknown[]) => api.nextSheet(...a),
  },
}))
const hooks = vi.hoisted(() => ({ list: vi.fn() }))
vi.mock('@nosdesk/core/services/webhookService', () => ({
  default: { listWebhooks: (...a: unknown[]) => hooks.list(...a) },
}))
vi.mock('@/composables/useAssetKindsQuery', async () => {
  const { computed } = await import('vue')
  return { useAssetKindsQuery: () => ({ kinds: computed(() => []) }) }
})

import ImportDetailView from '@/views/ImportDetailView.vue'

const FIELDS: ImportField[] = [
  { id: 'name', label: 'Name', group: 'core', value_type: 'text', required: true },
  { id: 'kind', label: 'Kind', group: 'core', value_type: 'enum', required: true },
  { id: 'asset_tag', label: 'Asset tag', group: 'core', value_type: 'text', required: false },
]

function job(overrides: Partial<ImportJob> = {}): ImportJob {
  return {
    id: 'job-1',
    job_type: 'assets',
    status: 'done',
    state: 'imported',
    pruned_at: null,
    created_by_name: 'Priya Nair',
    filename: 'cameras.csv',
    created_by: 'u-1',
    created_at: '2026-10-15T00:00:00Z',
    updated_at: '2026-10-15T00:00:00Z',
    completed_at: '2026-10-15T00:00:00Z',
    summary: {
      row_count: 12,
      would_create: 8,
      would_update: 2,
      unchanged: 0,
      errors: [],
      errors_truncated: false,
      errors_total: 2,
      rows_with_errors: 2,
    },
    records_committed: 10,
    error_message: null,
    source: {
      format: 'csv',
      sheets: [],
      sheet: null,
      header_row: 1,
      headers: [
        { index: 0, name: 'Device', samples: [] },
        { index: 1, name: 'Type', samples: [] },
        { index: 2, name: 'Asset #', samples: [] },
        { index: 3, name: 'Comments', samples: [] },
      ],
      row_count: 12,
      delimiter: ',',
    },
    suggested_mapping: [],
    mapping: { name: 0, kind: 1, asset_tag: 2 },
    ignored: [3],
    options: { match_by: 'asset_tag', mode: 'create_update', date_order: 'dmy' },
    ...overrides,
  }
}

let wrapper: VueWrapper | null = null
let router: Router

beforeEach(() => {
  for (const fn of Object.values(api)) fn.mockReset()
  hooks.list.mockReset()
  hooks.list.mockResolvedValue([])
  api.fields.mockResolvedValue(FIELDS)
  api.rows.mockResolvedValue({
    phase: 'committed',
    total: 0,
    offset: 0,
    limit: 25,
    counts: { all: 12, create: 8, update: 2, unchanged: 0, skipped: 2 },
    rows: [],
  })
  router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/admin/data-import/imports/:id', component: ImportDetailView },
      { path: '/:any(.*)*', component: { template: '<div />' } },
    ],
  })
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

async function open(found: ImportJob) {
  api.get.mockResolvedValue(found)
  await router.push(`/admin/data-import/imports/${found.id}`)
  await router.isReady()
  wrapper = mountWithProviders({ template: '<RouterView />' }, {}, {}, [[PiniaColada, {}] as never, router])
  await flushPromises()
  return wrapper
}

describe('ImportDetailView', () => {
  it('shows who, when, the options, the mapping, the cards, the rows and the report', async () => {
    await open(job())
    expect(api.get).toHaveBeenCalledWith('job-1')
    expect(wrapper!.find('[data-test="import-detail-title"]').text()).toBe('import-detail-title')
    expect(wrapper!.text()).toContain('cameras.csv · Priya Nair')
    expect(wrapper!.find('[data-test="import-detail-state"]').text()).toBe('import-history-state-imported')
    const mapping = wrapper!.find('[data-test="import-detail-mapping"]').findAll('tr').map((r) => r.text())
    expect(mapping).toEqual(['DeviceName', 'TypeKind', 'Asset #Asset tag', 'Commentsimport-map-ignore'])
    expect(wrapper!.find('[data-test="import-review-tiles"]').exists()).toBe(true)
    expect(wrapper!.find('[data-test="import-not-imported"]').exists()).toBe(true)
    expect(wrapper!.find('[data-test="import-rows"]').exists()).toBe(true)
    expect(api.rows).toHaveBeenCalled()
    expect(wrapper!.find('[data-test="import-detail-report"]').exists()).toBe(true)
    expect(wrapper!.find('[data-test="import-commit"]').exists()).toBe(false)
    expect(wrapper!.find('[data-test="import-detail-kept"]').exists()).toBe(false)
  })

  it('past 30 days shows counts, options and mapping, and says the details are gone', async () => {
    await open(job({ pruned_at: '2026-11-15T00:00:00Z' }))
    expect(wrapper!.find('[data-test="import-detail-kept"]').text()).toBe('import-detail-kept-30-days')
    expect(wrapper!.find('[data-test="import-review-tiles"]').exists()).toBe(true)
    expect(wrapper!.find('[data-test="import-detail-mapping"]').exists()).toBe(true)
    expect(wrapper!.find('[data-test="import-not-imported"]').exists()).toBe(false)
    expect(wrapper!.find('[data-test="import-what-changes"]').exists()).toBe(false)
    expect(wrapper!.find('[data-test="import-rows"]').exists()).toBe(false)
    expect(wrapper!.find('[data-test="import-detail-report"]').exists()).toBe(false)
    expect(api.rows).not.toHaveBeenCalled()
  })

  it('offers the previous values with their help, and removal with its window', async () => {
    await open(
      job({
        removal_closes_at: '2999-11-14T00:00:00Z',
        previous_values: { rows: 2, blank_before: 3 },
      }),
    )
    const previous = wrapper!.find('[data-test="import-detail-previous"]')
    expect(previous.text()).toBe('import-previous-download')
    expect(previous.attributes('disabled')).toBeUndefined()
    expect(wrapper!.find('[data-test="import-detail-previous-help"]').text()).toBe('import-previous-help-blank')
    expect(wrapper!.find('[data-test="import-what-changes"]').text()).toContain('import-review-changes-blank-before')
    expect(wrapper!.find('[data-test="import-remove-button"]').exists()).toBe(true)
    expect(hooks.list).toHaveBeenCalled()

    api.previousValues.mockResolvedValue({ blob: new Blob(['x']), filename: 'cameras-previous-values.csv' })
    URL.createObjectURL = vi.fn(() => 'blob:x')
    URL.revokeObjectURL = vi.fn()
    await previous.trigger('click')
    await flushPromises()
    expect(api.previousValues).toHaveBeenCalledWith('job-1')
  })

  it('has nothing to put back for an import without updates', async () => {
    await open(job({ previous_values: { rows: 0, blank_before: 0 } }))
    expect(wrapper!.find('[data-test="import-detail-previous"]').attributes('disabled')).toBeDefined()
    expect(wrapper!.find('[data-test="import-detail-previous-none"]').text()).toBe('import-previous-none')
    expect(wrapper!.find('[data-test="import-detail-previous-help"]').exists()).toBe(false)
  })

  it('shows the removal result in the state and in place of the button', async () => {
    await open(
      job({
        removal_closes_at: '2999-11-14T00:00:00Z',
        removed_at: '2026-10-15T00:00:00Z',
        removed_by_name: 'Priya Nair',
        remove_result: { created: 8, deleted: 7, gone: 0, deleted_ids: [], kept: [] },
      }),
    )
    expect(wrapper!.find('[data-test="import-detail-state"]').text()).toBe('import-detail-state-removed')
    expect(wrapper!.find('[data-test="import-remove-result"]').exists()).toBe(true)
    expect(wrapper!.find('[data-test="import-remove-button"]').exists()).toBe(false)
  })

  it('says the removal window closed on an import past its 30 days', async () => {
    await open(job({ pruned_at: '2026-11-15T00:00:00Z', removal_closes_at: '2026-11-14T00:00:00Z' }))
    expect(wrapper!.find('[data-test="import-remove-closed"]').text()).toBe('import-remove-window-closed')
    expect(wrapper!.find('[data-test="import-detail-previous"]').exists()).toBe(false)
  })

  it('imports the next sheet of the workbook in the wizard', async () => {
    api.nextSheet.mockResolvedValue(job({ id: 'job-2', status: 'parsed', state: 'draft' }))
    await open(job({ next_sheet: 'Vehicles' }))
    const next = wrapper!.find('[data-test="import-detail-next-sheet"]')
    expect(next.text()).toBe('import-done-next-sheet')
    await next.trigger('click')
    await flushPromises()
    expect(api.nextSheet).toHaveBeenCalledWith('job-1', 'Vehicles')
    expect(router.currentRoute.value.path).toBe('/admin/data-import/csv')
    expect(router.currentRoute.value.query.job).toBe('job-2')
  })

  it('says so when the file for the next sheet is gone', async () => {
    api.nextSheet.mockRejectedValue({
      response: { status: 409, data: { code: 'import_file_missing', error: 'gone' } },
    })
    await open(job({ next_sheet: 'Vehicles' }))
    await wrapper!.find('[data-test="import-detail-next-sheet"]').trigger('click')
    await flushPromises()
    expect(wrapper!.text()).toContain('import-file-missing')
    expect(router.currentRoute.value.path).toBe('/admin/data-import/imports/job-1')
  })

  it('sends a draft to the wizard and says an expired one is over', async () => {
    await open(job({ status: 'dry_run_done', state: 'draft' }))
    expect(router.currentRoute.value.path).toBe('/admin/data-import/csv')
    expect(router.currentRoute.value.query.job).toBe('job-1')

    wrapper!.unmount()
    wrapper = null
    await open(job({ id: 'job-2', status: 'dry_run_done', state: 'expired' }))
    expect(wrapper!.find('[data-test="import-detail-state"]').text()).toBe('import-history-state-expired')
    expect(wrapper!.find('[data-test="import-detail-expired"]').exists()).toBe(true)
    expect(wrapper!.find('[data-test="import-review-tiles"]').exists()).toBe(false)
  })
})

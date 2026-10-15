import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { ImportField, ImportJob } from '@nosdesk/core/services/importService'

const api = vi.hoisted(() => ({
  upload: vi.fn(),
  putMapping: vi.fn(),
  commit: vi.fn(),
  get: vi.fn(),
  fields: vi.fn(),
}))

vi.mock('@nosdesk/core/services/importService', () => ({
  importFieldsQueryKey: (type: string) => ['import-fields', type],
  importService: {
    upload: (...a: unknown[]) => api.upload(...a),
    putMapping: (...a: unknown[]) => api.putMapping(...a),
    commit: (...a: unknown[]) => api.commit(...a),
    get: (...a: unknown[]) => api.get(...a),
    fields: (...a: unknown[]) => api.fields(...a),
    templateUrl: (type: string) => `/api/admin/import/template/${type}`,
  },
}))

vi.mock('@/composables/useAssetKindsQuery', async () => {
  const { computed } = await import('vue')
  return {
    useAssetKindsQuery: () => ({
      kinds: computed(() => [
        { slug: 'generic', label: 'Generic asset' },
        { slug: 'camera', label: 'Camera' },
      ]),
    }),
  }
})

// Pickers reduced to plain controls: the target picker is a <select>, the
// default kind picker one button per option.
vi.mock('@/components/common/SearchableDropdown.vue', () => ({
  default: {
    props: ['modelValue', 'options', 'invalid', 'ariaLabel'],
    emits: ['update:modelValue'],
    template: `<select data-test="target" :aria-label="ariaLabel" :data-invalid="invalid ? 'true' : undefined" :value="modelValue" @change="$emit('update:modelValue', $event.target.value)">
      <option value=""></option>
      <option v-for="o in options" :key="o.value" :value="o.value">{{ o.label }}</option>
    </select>`,
  },
}))
vi.mock('@/components/common/BaseDropdown.vue', () => ({
  default: {
    props: ['options', 'modelValue', 'label'],
    emits: ['update:modelValue'],
    template: `<div data-test="dropdown" :data-label="label"><button v-for="o in options" :key="o.value" type="button" :data-value="o.value" @click="$emit('update:modelValue', o.value)">{{ o.label }}</button></div>`,
  },
}))
vi.mock('@/components/common/ConfirmModal.vue', () => ({
  default: {
    props: ['show', 'title', 'message', 'confirmLabel'],
    emits: ['confirm', 'close'],
    template: `<div v-if="show" data-test="confirm"><p data-test="confirm-title">{{ title }}</p><p data-test="confirm-message">{{ message }}</p><button type="button" data-test="confirm-ok" @click="$emit('confirm')">{{ confirmLabel }}</button></div>`,
  },
}))

import CsvImportView from '@/views/CsvImportView.vue'

const FIELDS: ImportField[] = [
  { id: 'name', label: 'Name', group: 'core', value_type: 'text', required: true },
  { id: 'kind', label: 'Kind', group: 'core', value_type: 'enum', required: true },
  { id: 'asset_tag', label: 'Asset tag', group: 'core', value_type: 'text', required: false },
  { id: 'serial_number', label: 'Serial number', group: 'core', value_type: 'text', required: false },
  { id: 'quantity', label: 'Quantity', group: 'core', value_type: 'number', required: false },
]

function parsedJob(overrides: Partial<ImportJob> = {}): ImportJob {
  return {
    id: 'job-1',
    job_type: 'assets',
    status: 'parsed',
    filename: 'cameras.csv',
    created_by: null,
    created_at: '2026-10-15T00:00:00Z',
    updated_at: '2026-10-15T00:00:00Z',
    completed_at: null,
    summary: null,
    records_committed: null,
    error_message: null,
    source: {
      format: 'csv',
      sheets: [],
      sheet: null,
      header_row: 1,
      headers: [
        { index: 0, name: 'Device', samples: ['Cam 1'] },
        { index: 1, name: 'Type', samples: ['camera'] },
        { index: 2, name: 'Asset #', samples: ['T-1'] },
        { index: 3, name: 'S/N', samples: ['SN-1'] },
        { index: 4, name: 'Owner', samples: [] },
      ],
      row_count: 12,
      delimiter: ',',
    },
    suggested_mapping: [
      { index: 0, field: 'name', reason: 'previous' },
      { index: 1, field: 'kind', reason: 'synonym' },
      { index: 2, field: 'asset_tag', reason: 'synonym' },
      { index: 3, field: 'serial_number', reason: 'synonym' },
    ],
    mapping: null,
    options: null,
    ...overrides,
  }
}

function checkedJob(overrides: Partial<ImportJob> = {}): ImportJob {
  return parsedJob({
    status: 'dry_run_done',
    mapping: { name: 0, kind: 1, asset_tag: 2, serial_number: 3 },
    options: { match_by: 'asset_tag', decimal: '.', date_order: 'iso' },
    summary: {
      row_count: 12,
      would_create: 8,
      would_update: 2,
      errors: [{ row: 5, column: 'quantity', source_column: 'Qty', message: "'lots' is not a valid decimal" }],
      errors_truncated: false,
      errors_total: 2,
      rows_with_errors: 2,
      error_groups: [{ column: 'quantity', source_column: 'Qty', message: "'lots' is not a valid decimal", count: 2 }],
    },
    ...overrides,
  })
}

let wrapper: VueWrapper | null = null
let router: Router

beforeEach(() => {
  for (const fn of Object.values(api)) fn.mockReset()
  api.fields.mockResolvedValue(FIELDS)
  router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/:any(.*)*', component: { template: '<div />' } }],
  })
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

async function mountAt(path = '/admin/data-import/csv') {
  await router.push(path)
  await router.isReady()
  wrapper = mountWithProviders(CsvImportView, {}, {}, [[PiniaColada, {}] as never, router])
  await flushPromises()
  return wrapper
}

function button(text: string) {
  const found = wrapper!.findAll('button').find((b) => b.text() === text)
  expect(found, text).toBeTruthy()
  return found!
}

function currentStep(): string | undefined {
  return wrapper!.find('[aria-current="step"]').attributes('data-step')
}

function targets() {
  return wrapper!.findAll('select[data-test="target"]')
}

const checkButton = () => wrapper!.find('[data-test="import-check"]')

describe('CsvImportView step machine', () => {
  it('walks Start, Upload, Map, Review and Done', async () => {
    api.upload.mockResolvedValue(parsedJob())
    api.putMapping.mockResolvedValue(checkedJob())
    api.commit.mockResolvedValue(checkedJob({ status: 'done', records_committed: 10 }))
    await mountAt()
    expect(currentStep()).toBe('start')

    await wrapper!.find('[data-test="import-continue"]').trigger('click')
    expect(currentStep()).toBe('upload')

    const input = wrapper!.find('[data-test="import-file"]')
    const file = new File(['name\nx\n'], 'cameras.csv', { type: 'text/csv' })
    Object.defineProperty(input.element, 'files', { value: [file] })
    await input.trigger('change')
    await wrapper!.find('[data-test="import-upload"]').trigger('click')
    await flushPromises()
    expect(api.upload).toHaveBeenCalledWith('assets', file)
    expect(currentStep()).toBe('map')
    expect(router.currentRoute.value.query.job).toBe('job-1')

    // Suggestions pre-fill the targets; Owner has no suggestion.
    expect(targets().map((s) => (s.element as HTMLSelectElement).value)).toEqual([
      'name',
      'kind',
      'asset_tag',
      'serial_number',
      '',
    ])
    expect(checkButton().text()).toBe('import-map-check')
    await checkButton().trigger('click')
    await flushPromises()
    expect(api.putMapping).toHaveBeenCalledWith(
      'job-1',
      { name: 0, kind: 1, asset_tag: 2, serial_number: 3 },
      { match_by: 'asset_tag', date_order: 'iso', decimal: '.' },
    )
    expect(currentStep()).toBe('review')

    await wrapper!.find('[data-test="import-commit"]').trigger('click')
    expect(wrapper!.find('[data-test="confirm-title"]').text()).toBe('import-review-confirm-title')
    await wrapper!.find('[data-test="confirm-ok"]').trigger('click')
    await flushPromises()
    expect(api.commit).toHaveBeenCalledWith('job-1')
    expect(currentStep()).toBe('done')
  })

  it('goes back from Review to Map keeping the mapping', async () => {
    api.get.mockResolvedValue(checkedJob({ mapping: { name: 0, kind: 1, serial_number: 4 } }))
    await mountAt('/admin/data-import/csv?job=job-1')
    expect(currentStep()).toBe('review')
    await button('import-review-back').trigger('click')
    expect(currentStep()).toBe('map')
    expect(targets().map((s) => (s.element as HTMLSelectElement).value)).toEqual([
      'name',
      'kind',
      '__ignore__',
      '__ignore__',
      'serial_number',
    ])
  })
})

describe('CsvImportView resume', () => {
  it.each([
    ['parsed', parsedJob(), 'map'],
    ['dry_run_done', checkedJob(), 'review'],
    ['done', checkedJob({ status: 'done', records_committed: 10 }), 'done'],
  ] as const)('opens a %s job at %s', async (_status, found, expected) => {
    api.get.mockResolvedValue(found)
    await mountAt('/admin/data-import/csv?job=job-1')
    expect(api.get).toHaveBeenCalledWith('job-1')
    expect(currentStep()).toBe(expected)
  })

  it('opens a failed job at Upload with its message', async () => {
    api.get.mockResolvedValue(parsedJob({ status: 'failed', error_message: 'file is not valid UTF-8' }))
    await mountAt('/admin/data-import/csv?job=job-1')
    expect(currentStep()).toBe('upload')
    expect(wrapper!.text()).toContain('file is not valid UTF-8')
  })

  it('starts over when the job is gone or predates mapping', async () => {
    api.get.mockRejectedValue({ response: { status: 404 } })
    await mountAt('/admin/data-import/csv?job=gone')
    expect(currentStep()).toBe('start')
    expect(wrapper!.text()).toContain('import-resume-failed')
    expect(router.currentRoute.value.query.job).toBeUndefined()

    wrapper!.unmount()
    api.get.mockResolvedValue(checkedJob({ source: null }))
    await mountAt('/admin/data-import/csv?job=old')
    expect(currentStep()).toBe('start')
  })
})

describe('CsvImportView Map step', () => {
  async function openMap(job = parsedJob()) {
    api.get.mockResolvedValue(job)
    await mountAt('/admin/data-import/csv?job=job-1')
    expect(currentStep()).toBe('map')
  }

  it('blocks a field mapped from two columns', async () => {
    await openMap()
    await targets()[4].setValue('serial_number')
    expect(checkButton().attributes('disabled')).toBeDefined()
    expect(wrapper!.text()).toContain('import-map-problem-duplicate')
    expect(wrapper!.text()).toContain('import-map-blocked')
    expect(targets()[3].attributes('data-invalid')).toBe('true')
    expect(targets()[4].attributes('data-invalid')).toBe('true')

    await targets()[4].setValue('__ignore__')
    expect(checkButton().attributes('disabled')).toBeUndefined()
  })

  it('blocks an uncovered required field', async () => {
    await openMap()
    await targets()[0].setValue('__ignore__')
    expect(checkButton().attributes('disabled')).toBeDefined()
    expect(wrapper!.text()).toContain('import-map-problem-required')
  })

  it('lets a default kind cover the Kind column', async () => {
    api.putMapping.mockResolvedValue(checkedJob())
    await openMap()
    await targets()[1].setValue('__ignore__')
    expect(checkButton().attributes('disabled')).toBeDefined()
    expect(wrapper!.text()).toContain('import-map-problem-kind')

    // The picker inside the problem callout.
    await wrapper!.find('[data-test="dropdown"] [data-value="camera"]').trigger('click')
    expect(wrapper!.text()).not.toContain('import-map-problem-kind')
    expect(checkButton().attributes('disabled')).toBeUndefined()

    await checkButton().trigger('click')
    await flushPromises()
    expect(api.putMapping).toHaveBeenCalledWith(
      'job-1',
      { name: 0, asset_tag: 2, serial_number: 3 },
      expect.objectContaining({ default_kind: 'camera', match_by: 'asset_tag' }),
    )
  })

  it('filters rows without reordering them', async () => {
    await openMap()
    const names = () => wrapper!.findAll('tbody th').map((th) => th.find('span').text())
    expect(names()).toEqual(['Device', 'Type', 'Asset #', 'S/N', 'Owner'])

    // Needs attention: guesses and the unchosen column.
    await setFilter('attention')
    expect(names()).toEqual(['Type', 'Asset #', 'S/N', 'Owner'])

    await setFilter('ignored')
    expect(names()).toEqual(['Owner'])

    await setFilter('all')
    expect(names()).toHaveLength(5)
  })
})

/** Drive the filter SegmentedControl by its option label. */
async function setFilter(value: string) {
  const radio = wrapper!
    .findAll('[role="radio"]')
    .find((r) => r.text() === `import-map-filter-${value}`)
  expect(radio, value).toBeTruthy()
  await radio!.trigger('click')
  await flushPromises()
}

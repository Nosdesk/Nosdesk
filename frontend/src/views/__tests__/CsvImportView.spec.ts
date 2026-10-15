import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import { mountWithProviders } from '@/test/mountWithProviders'
import type {
  ImportField,
  ImportJob,
  ImportRowsPage,
  ImportSource,
} from '@nosdesk/core/services/importService'

const api = vi.hoisted(() => ({
  upload: vi.fn(),
  putSource: vi.fn(),
  putMapping: vi.fn(),
  commit: vi.fn(),
  get: vi.fn(),
  fields: vi.fn(),
  rows: vi.fn(),
  errorReport: vi.fn(),
}))

vi.mock('@nosdesk/core/services/importService', () => ({
  importFieldsQueryKey: (type: string) => ['import-fields', type],
  importService: {
    upload: (...a: unknown[]) => api.upload(...a),
    putSource: (...a: unknown[]) => api.putSource(...a),
    putMapping: (...a: unknown[]) => api.putMapping(...a),
    commit: (...a: unknown[]) => api.commit(...a),
    get: (...a: unknown[]) => api.get(...a),
    fields: (...a: unknown[]) => api.fields(...a),
    rows: (...a: unknown[]) => api.rows(...a),
    errorReport: (...a: unknown[]) => api.errorReport(...a),
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

function rowsPage(overrides: Partial<ImportRowsPage> = {}): ImportRowsPage {
  return {
    phase: 'checked',
    total: 2,
    offset: 0,
    limit: 50,
    counts: { all: 12, create: 8, update: 2, unchanged: 0, skipped: 2 },
    rows: [
      {
        row: 5,
        outcome: 'skipped',
        key: 'T-5',
        label: 'Cam 5',
        errors: [{ column: 'quantity', source_column: 'Qty', message: "'lots' is not a valid decimal" }],
      },
      { row: 9, outcome: 'skipped', key: 'T-9', errors: [{ column: null, message: 'whole row' }] },
    ],
    ...overrides,
  }
}

let wrapper: VueWrapper | null = null
let router: Router

beforeEach(() => {
  for (const fn of Object.values(api)) fn.mockReset()
  api.fields.mockResolvedValue(FIELDS)
  api.rows.mockResolvedValue(rowsPage())
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
    const base = parsedJob()
    const extra = ['Room', 'Dept', 'Supplier', 'PO #'].map((name, i) => ({
      index: 5 + i,
      name,
      samples: [],
    }))
    await openMap(
      parsedJob({ source: { ...base.source!, headers: [...base.source!.headers, ...extra] } }),
    )
    const names = () => wrapper!.findAll('tbody th').map((th) => th.find('span').text())
    expect(names()).toEqual(['Device', 'Type', 'Asset #', 'S/N', 'Owner', 'Room', 'Dept', 'Supplier', 'PO #'])

    // Needs attention: guesses and the unchosen columns.
    await setFilter('attention')
    expect(names()).toEqual(['Type', 'Asset #', 'S/N', 'Owner', 'Room', 'Dept', 'Supplier', 'PO #'])

    await setFilter('ignored')
    expect(names()).toEqual(['Owner', 'Room', 'Dept', 'Supplier', 'PO #'])

    await setFilter('all')
    expect(names()).toHaveLength(9)
  })

  it('leaves the summary strip off a file of 8 columns or fewer', async () => {
    await openMap()
    expect(wrapper!.find('[data-test="import-map-strip"]').exists()).toBe(false)
    expect(wrapper!.findAll('tbody th')).toHaveLength(5)
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

describe('CsvImportView Review rows', () => {
  async function openReview(job = checkedJob()) {
    api.get.mockResolvedValue(job)
    await mountAt('/admin/data-import/csv?job=job-1')
    expect(currentStep()).toBe('review')
  }

  it('lists skipped rows first, with their problems under the file headers', async () => {
    await openReview()
    expect(api.rows).toHaveBeenCalledWith('job-1', { filter: 'skipped', offset: 0, limit: 50 })
    const rows = wrapper!.findAll('[data-test="import-row"]')
    expect(rows).toHaveLength(2)
    expect(rows[0].text()).toContain('Cam 5')
    expect(rows[0].text()).toContain("Qty: 'lots' is not a valid decimal")
    expect(rows[1].text()).toContain('T-9')
    expect(rows[1].text()).toContain('whole row')
  })

  it('switches the list by outcome', async () => {
    await openReview()
    const radio = wrapper!
      .findAll('[role="radio"]')
      .find((r) => r.text() === 'import-review-filter-create')
    expect(radio).toBeTruthy()
    await radio!.trigger('click')
    await flushPromises()
    expect(api.rows).toHaveBeenLastCalledWith('job-1', { filter: 'create', offset: 0, limit: 50 })
  })

  it('pages through a long list', async () => {
    api.rows.mockResolvedValue(rowsPage({ total: 120 }))
    await openReview()
    await wrapper!.find('[data-test="import-rows-next"]').trigger('click')
    await flushPromises()
    expect(api.rows).toHaveBeenLastCalledWith('job-1', { filter: 'skipped', offset: 50, limit: 50 })
  })

  it('shows notices and downloads the error report', async () => {
    const base = checkedJob()
    await openReview(
      checkedJob({
        summary: {
          ...base.summary!,
          notices: [{ code: 'no_key', count: 3, message: 'fallback' }],
        },
      }),
    )
    expect(wrapper!.text()).toContain('import-review-notice-no-key')

    api.errorReport.mockResolvedValue({ blob: new Blob(['Row,Errors\n']), filename: 'x-errors.csv' })
    const createObjectURL = vi.fn(() => 'blob:report')
    const revokeObjectURL = vi.fn()
    vi.stubGlobal('URL', { ...URL, createObjectURL, revokeObjectURL })
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => {})
    try {
      await wrapper!.find('[data-test="import-error-report"]').trigger('click')
      await flushPromises()
      expect(api.errorReport).toHaveBeenCalledWith('job-1')
      expect(click).toHaveBeenCalled()
    } finally {
      click.mockRestore()
      vi.unstubAllGlobals()
    }
  })

  it('offers the report on Done when rows were skipped', async () => {
    api.get.mockResolvedValue(checkedJob({ status: 'done', records_committed: 10 }))
    await mountAt('/admin/data-import/csv?job=job-1')
    expect(currentStep()).toBe('done')
    expect(wrapper!.find('[data-test="import-done-report"]').exists()).toBe(true)
  })
})

function workbookJob(overrides: Partial<ImportSource> = {}): ImportJob {
  const base = parsedJob()
  return parsedJob({
    filename: 'TVS Assets 2026.xlsx',
    source: {
      ...base.source!,
      format: 'xlsx',
      delimiter: null,
      sheets: [
        { name: 'Cameras', visible: true, rows: 412 },
        { name: 'Vehicles', visible: true, rows: 40 },
        { name: 'Lookups', visible: false, rows: 12 },
      ],
      sheet: 'Cameras',
      header_row: 3,
      preview: [
        { row: 1, cells: ['TVS Assets 2026', ''] },
        { row: 2, cells: ['', ''] },
        { row: 3, cells: ['Device', 'Type'] },
        { row: 4, cells: ['Cam 1', 'camera'] },
      ],
      ...overrides,
    },
  })
}

describe('CsvImportView sheet panel', () => {
  it('accepts a workbook and opens on its sheets', async () => {
    api.upload.mockResolvedValue(workbookJob())
    await mountAt()
    await wrapper!.find('[data-test="import-continue"]').trigger('click')
    const input = wrapper!.find('[data-test="import-file"]')
    const file = new File(['PK'], 'TVS Assets 2026.xlsx', {
      type: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
    })
    Object.defineProperty(input.element, 'files', { value: [file] })
    await input.trigger('change')
    expect(wrapper!.find('[data-test="import-upload"]').attributes('disabled')).toBeUndefined()
    await wrapper!.find('[data-test="import-upload"]').trigger('click')
    await flushPromises()
    expect(api.upload).toHaveBeenCalledWith('assets', file)
    expect(currentStep()).toBe('map')

    const panel = wrapper!.find('[data-test="import-sheet-panel"]')
    expect(panel.exists()).toBe(true)
    const options = panel.findAll('[data-test="import-sheet-option"]')
    expect(options.map((o) => o.text().includes('Lookups'))).toEqual([false, false, true])
    expect(panel.findAll('[data-test="import-sheet-hidden"]')).toHaveLength(1)
    const current = panel.find('tr[aria-current="true"]')
    expect(current.text()).toContain('Device')
  })

  it('switches the sheet and remounts the columns', async () => {
    api.get.mockResolvedValue(workbookJob())
    api.putSource.mockResolvedValue(
      workbookJob({
        sheet: 'Vehicles',
        header_row: 1,
        headers: [
          { index: 0, name: 'Rego', samples: ['ABC-123'] },
          { index: 1, name: 'Make', samples: ['Toyota'] },
        ],
        preview: [],
      }),
    )
    await mountAt('/admin/data-import/csv?job=job-1')
    expect(targets()).toHaveLength(5)

    const radios = wrapper!.findAll('input[name="import-sheet"]')
    await radios[1].setValue(true)
    await flushPromises()
    expect(api.putSource).toHaveBeenCalledWith('job-1', 'Vehicles', null)
    expect(targets()).toHaveLength(2)
  })

  it('takes a header row from the preview', async () => {
    api.get.mockResolvedValue(workbookJob())
    api.putSource.mockResolvedValue(workbookJob({ header_row: 4 }))
    await mountAt('/admin/data-import/csv?job=job-1')
    const rowButtons = wrapper!.findAll('[data-test="import-sheet-preview-row"] th button')
    await rowButtons[3].trigger('click')
    await flushPromises()
    expect(api.putSource).toHaveBeenCalledWith('job-1', 'Cameras', 4)
  })

  it('folds to one line for a one-sheet workbook with its header on row 1', async () => {
    api.get.mockResolvedValue(
      workbookJob({
        sheets: [{ name: 'Sheet1', visible: true, rows: 12 }],
        sheet: 'Sheet1',
        header_row: 1,
      }),
    )
    await mountAt('/admin/data-import/csv?job=job-1')
    expect(wrapper!.find('[data-test="import-sheet-summary"]').exists()).toBe(true)
    expect(wrapper!.find('input[name="import-sheet"]').exists()).toBe(false)
    await wrapper!.find('[data-test="import-sheet-change"]').trigger('click')
    expect(wrapper!.find('[data-test="import-sheet-apply"]').exists()).toBe(true)
  })

  it('shows no sheet panel for a CSV', async () => {
    api.get.mockResolvedValue(parsedJob())
    await mountAt('/admin/data-import/csv?job=job-1')
    expect(wrapper!.find('[data-test="import-sheet-panel"]').exists()).toBe(false)
  })
})

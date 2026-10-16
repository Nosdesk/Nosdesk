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
  ImportValues,
} from '@nosdesk/core/services/importService'

const api = vi.hoisted(() => ({
  upload: vi.fn(),
  putSource: vi.fn(),
  putType: vi.fn(),
  remove: vi.fn(),
  putMapping: vi.fn(),
  commit: vi.fn(),
  get: vi.fn(),
  fields: vi.fn(),
  rows: vi.fn(),
  errorReport: vi.fn(),
  values: vi.fn(),
  putValues: vi.fn(),
  previewValues: vi.fn(),
}))

vi.mock('@nosdesk/core/services/importService', () => ({
  importFieldsQueryKey: (type: string) => ['import-fields', type],
  importValuesQueryKey: (id: string, at: string) => ['import-values', id, at],
  importValuesPreviewQueryKey: (id: string, at: string, key: string) => ['import-values-preview', id, at, key],
  importService: {
    upload: (...a: unknown[]) => api.upload(...a),
    putSource: (...a: unknown[]) => api.putSource(...a),
    putType: (...a: unknown[]) => api.putType(...a),
    remove: (...a: unknown[]) => api.remove(...a),
    putMapping: (...a: unknown[]) => api.putMapping(...a),
    commit: (...a: unknown[]) => api.commit(...a),
    get: (...a: unknown[]) => api.get(...a),
    fields: (...a: unknown[]) => api.fields(...a),
    rows: (...a: unknown[]) => api.rows(...a),
    errorReport: (...a: unknown[]) => api.errorReport(...a),
    values: (...a: unknown[]) => api.values(...a),
    putValues: (...a: unknown[]) => api.putValues(...a),
    previewValues: (...a: unknown[]) => api.previewValues(...a),
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

// The signed-in admin, for "your import".
vi.mock('@/stores/auth', () => ({ useAuthStore: () => ({ user: { uuid: 'me' } }) }))

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

const SAVED_BY_ME = { job_id: 'job-0', by: 'me', by_name: 'Me', at: '2026-07-12T00:00:00Z' }
const SAVED_BY_SAM = { job_id: 'job-0', by: 'sam', by_name: 'Sam', at: '2026-07-12T00:00:00Z' }

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

/** A checked job's values with nothing to ask. */
function noValues(): ImportValues {
  return { rows_checked: 12, rows_skipped: 2, skipped_other: [5, 9], columns: [] }
}

/** Type has a value with no match and a guess; Qty a number that doesn't
 *  read. Rows 5 and 9 are skipped for other reasons. */
function valuesToAnswer(): ImportValues {
  return {
    rows_checked: 12,
    rows_skipped: 5,
    skipped_other: [5, 9],
    columns: [
      {
        field: 'quantity',
        index: 5,
        header: 'Qty',
        value_type: 'number',
        required: false,
        choices: [],
        distinct: 3,
        matched: 2,
        values: [
          {
            key: 'two',
            text: 'two',
            blank: false,
            rows: [6, 7],
            match: 'no_match',
            reason: { code: 'unreadable' },
            suggested: { action: 'skip' },
            skipped_rows: [6, 7],
          },
        ],
      },
      {
        field: 'kind',
        index: 1,
        header: 'Type',
        value_type: 'enum',
        required: true,
        choices: [
          { value: 'generic', label: 'Generic asset' },
          { value: 'camera', label: 'Camera' },
        ],
        distinct: 3,
        matched: 1,
        values: [
          {
            key: 'tripod',
            text: 'tripod',
            blank: false,
            rows: [8, 10, 11],
            match: 'no_match',
            suggested: { action: 'skip' },
            skipped_rows: [8, 10, 11],
          },
          {
            key: 'cam',
            text: 'Cam',
            blank: false,
            rows: [3],
            match: 'guessed',
            reason: { code: 'synonym' },
            suggested: { action: 'match', target: 'camera' },
            skipped_rows: [],
          },
          {
            key: 'camera',
            text: 'camera',
            blank: false,
            rows: [2, 4],
            match: 'exact',
            suggested: { action: 'match', target: 'camera' },
            skipped_rows: [],
          },
        ],
      },
    ],
  }
}

let wrapper: VueWrapper | null = null
let router: Router

beforeEach(() => {
  for (const fn of Object.values(api)) fn.mockReset()
  api.fields.mockResolvedValue(FIELDS)
  api.rows.mockResolvedValue(rowsPage())
  api.values.mockResolvedValue(noValues())
  localStorage.clear()
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

/** Pick `file` with the Start screen's file input. */
async function pick(file: File) {
  const input = wrapper!.find('[data-test="import-file"]')
  Object.defineProperty(input.element, 'files', { value: [file], configurable: true })
  await input.trigger('change')
}

const csvFile = (name = 'cameras.csv') =>
  new File(['name\nx\n'], name, { type: 'text/csv' })

describe('CsvImportView step machine', () => {
  it('walks Start, Map, Review and Done', async () => {
    api.upload.mockResolvedValue(parsedJob())
    api.putMapping.mockResolvedValue(checkedJob())
    api.commit.mockResolvedValue(checkedJob({ status: 'done', records_committed: 10 }))
    await mountAt()
    expect(currentStep()).toBe('start')
    expect(wrapper!.findAll('[data-step]').map((s) => s.attributes('data-step'))).toEqual([
      'start',
      'map',
      'review',
      'done',
    ])

    // Picking the file uploads it; there is no separate Upload step.
    const file = csvFile()
    await pick(file)
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
      { match_by: 'asset_tag', mode: 'create_update', date_order: 'iso', decimal: '.', constants: [] },
      [],
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
    api.get.mockResolvedValue(
      checkedJob({ mapping: { name: 0, kind: 1, serial_number: 4 }, ignored: [2] }),
    )
    await mountAt('/admin/data-import/csv?job=job-1')
    expect(currentStep()).toBe('review')
    await button('import-review-back').trigger('click')
    expect(currentStep()).toBe('map')
    // Don't import comes back as chosen; a column left unchosen stays so.
    expect(targets().map((s) => (s.element as HTMLSelectElement).value)).toEqual([
      'name',
      'kind',
      '__ignore__',
      '',
      'serial_number',
    ])
  })
})

describe('CsvImportView commit guards', () => {
  async function importFromReview() {
    api.get.mockResolvedValueOnce(checkedJob())
    await mountAt('/admin/data-import/csv?job=job-1')
    expect(currentStep()).toBe('review')
    await wrapper!.find('[data-test="import-commit"]').trigger('click')
    await wrapper!.find('[data-test="confirm-ok"]').trigger('click')
  }

  it('swaps in the fresh plan when it changed since the check', async () => {
    const fresh = checkedJob({
      updated_at: '2026-10-15T01:00:00Z',
      summary: { ...checkedJob().summary!, would_create: 7, would_update: 3 },
    })
    api.commit.mockRejectedValue({
      response: { status: 409, data: { code: 'import_plan_changed', error: 'server words', job: fresh } },
    })
    await importFromReview()
    await flushPromises()
    expect(currentStep()).toBe('review')
    expect(wrapper!.find('[data-test="import-commit-notice"]').text()).toBe(
      'import-review-plan-changed',
    )
    expect(wrapper!.text()).not.toContain('server words')
    expect(wrapper!.text()).toContain('import-review-tile-create7')
    // The rows are read again for the fresh plan.
    expect(api.rows.mock.calls.length).toBeGreaterThan(1)
  })

  describe('when the request ends before the commit does', () => {
    beforeEach(() => {
      vi.useFakeTimers()
    })
    afterEach(() => {
      vi.useRealTimers()
    })

    it('checks the job until it is imported', async () => {
      api.commit.mockRejectedValue(new Error('timeout of 0ms exceeded'))
      await importFromReview()
      await flushPromises()
      expect(wrapper!.find('[data-test="import-commit-notice"]').text()).toBe(
        'import-commit-still-running',
      )
      api.get.mockResolvedValueOnce(checkedJob({ status: 'committing' }))
      await vi.advanceTimersByTimeAsync(2000)
      expect(currentStep()).toBe('review')
      api.get.mockResolvedValueOnce(checkedJob({ status: 'done', records_committed: 10 }))
      await vi.advanceTimersByTimeAsync(2000)
      await flushPromises()
      expect(currentStep()).toBe('done')
      expect(wrapper!.find('[data-test="import-commit-notice"]').exists()).toBe(false)
    })

    it('waits on a 504 and reports a commit that never finished', async () => {
      api.commit.mockRejectedValue({ response: { status: 504, data: {} } })
      await importFromReview()
      await flushPromises()
      api.get.mockResolvedValueOnce(checkedJob())
      await vi.advanceTimersByTimeAsync(2000)
      await flushPromises()
      expect(currentStep()).toBe('review')
      expect(wrapper!.text()).toContain('import-commit-not-finished')
    })

    it('resumes waiting on a job that is committing', async () => {
      api.get.mockResolvedValueOnce(checkedJob({ status: 'committing' }))
      await mountAt('/admin/data-import/csv?job=job-1')
      expect(currentStep()).toBe('review')
      expect(wrapper!.find('[data-test="import-commit-notice"]').text()).toBe(
        'import-commit-still-running',
      )
      api.get.mockResolvedValueOnce(checkedJob({ status: 'done', records_committed: 10 }))
      await vi.advanceTimersByTimeAsync(2000)
      await flushPromises()
      expect(currentStep()).toBe('done')
    })
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

  it('opens a failed job at Start with its message', async () => {
    api.get.mockResolvedValue(parsedJob({ status: 'failed', error_message: 'file is not valid UTF-8' }))
    await mountAt('/admin/data-import/csv?job=job-1')
    expect(currentStep()).toBe('start')
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

  it('sets the kind for every row when no column gives it', async () => {
    api.putMapping.mockResolvedValue(checkedJob())
    await openMap()
    await targets()[1].setValue('__ignore__')
    expect(checkButton().attributes('disabled')).toBeDefined()
    expect(wrapper!.text()).toContain('import-map-problem-kind')

    // The picker inside the problem callout adds a value for every row.
    await wrapper!
      .find('[data-test="import-kind-for-every-row"] [data-value="camera"]')
      .trigger('click')
    expect(wrapper!.text()).not.toContain('import-map-problem-kind')
    expect(wrapper!.findAll('[data-test="import-constant"]')).toHaveLength(1)
    expect(checkButton().attributes('disabled')).toBeUndefined()

    await checkButton().trigger('click')
    await flushPromises()
    expect(api.putMapping).toHaveBeenCalledWith(
      'job-1',
      { name: 0, asset_tag: 2, serial_number: 3 },
      expect.objectContaining({
        constants: [{ field: 'kind', value: 'camera', apply_to: 'new' }],
        match_by: 'asset_tag',
      }),
      [1],
    )
    expect(api.putMapping.mock.calls[0][2]).not.toHaveProperty('default_kind')
  })

  it('pills only guesses, and says where a saved match came from', async () => {
    await openMap(
      parsedJob({
        suggested_mapping: [
          { index: 0, field: 'name', reason: 'exact' },
          { index: 1, field: 'kind', reason: 'synonym' },
          { index: 2, field: 'asset_tag', reason: 'previous', provenance: SAVED_BY_ME },
          { index: 3, field: null, reason: 'previous', provenance: SAVED_BY_SAM },
        ],
      }),
    )
    const message = (i: number) => wrapper!.find(`#import-map-message-${i}`)
    expect(message(0).text()).toBe('')
    expect(message(1).find('[data-test="pill-guessed"]').exists()).toBe(true)
    expect(message(2).find('[data-test="pill-saved"]').attributes('data-provenance')).toBe(
      'import-map-saved-you',
    )
    expect(message(3).find('[data-test="pill-saved"]').attributes('data-provenance')).toBe(
      'import-map-saved-from',
    )
    // Reachable by keyboard, so the provenance shows on focus too.
    expect(message(3).find('[data-test="pill-saved"]').attributes('tabindex')).toBe('0')
    expect((targets()[3].element as HTMLSelectElement).value).toBe('__ignore__')
  })

  it('sets a value for every row, on new rows unless ticked', async () => {
    api.putMapping.mockResolvedValue(checkedJob())
    await openMap()
    const add = () => wrapper!.find('[data-test="import-constant-add"]').trigger('click')
    const row = () => wrapper!.find('[data-test="import-constant"]')
    await add()
    await wrapper!.find('[data-test="import-constant-remove"]').trigger('click')
    expect(row().exists()).toBe(false)
    await add()
    // Only fields no column feeds are offered.
    const offered = row()
      .find('[data-label="import-constants-field-label"]')
      .findAll('button')
      .map((b) => b.attributes('data-value'))
    expect(offered).toEqual(['quantity'])
    await row().find('[data-label="import-constants-field-label"] [data-value="quantity"]').trigger('click')
    await row().find('input[type="number"]').setValue('2')
    await row().find('[role="checkbox"]').trigger('click')
    expect(row().text()).toContain('import-constants-apply-assets')

    await checkButton().trigger('click')
    await flushPromises()
    expect(api.putMapping.mock.calls[0][2].constants).toEqual([
      { field: 'quantity', value: '2', apply_to: 'all' },
    ])
  })

  it('sends the rows to import, with no Don\'t match choice', async () => {
    api.putMapping.mockResolvedValue(checkedJob())
    await openMap()
    const radios = () => wrapper!.find('[data-test="import-mode"]').findAll('[role="radio"]')
    expect(radios().map((r) => r.text())).toEqual([
      'import-options-mode-create-update',
      'import-options-mode-create-only',
      'import-options-mode-update-only',
    ])
    expect(wrapper!.text()).not.toContain('import-options-match-none')
    await radios()[2].trigger('click')
    expect(wrapper!.text()).toContain('import-options-mode-update-only-help')
    await checkButton().trigger('click')
    await flushPromises()
    expect(api.putMapping.mock.calls[0][2]).toMatchObject({
      match_by: 'asset_tag',
      mode: 'update_only',
    })
  })

  it('reads a stored legacy Don\'t match as only creating new', async () => {
    await openMap(parsedJob({ options: { match_by: 'none' as never } }))
    const checked = wrapper!
      .find('[data-test="import-mode"]')
      .findAll('[role="radio"]')
      .find((r) => r.attributes('aria-checked') === 'true')
    expect(checked?.text()).toBe('import-options-mode-create-only')
  })

  describe('when every column is settled', () => {
    const settled = () =>
      parsedJob({
        suggested_mapping: [
          { index: 0, field: 'name', reason: 'previous', provenance: SAVED_BY_SAM },
          { index: 1, field: 'kind', reason: 'previous', provenance: SAVED_BY_SAM },
          { index: 2, field: 'asset_tag', reason: 'exact' },
          { index: 3, field: 'serial_number', reason: 'label' },
          { index: 4, field: null, reason: 'previous', provenance: SAVED_BY_SAM },
        ],
      })

    it('folds to one line with the check focused, so Enter checks', async () => {
      api.putMapping.mockResolvedValue(checkedJob())
      await openMap(settled())
      const folded = wrapper!.find('[data-test="import-map-collapsed"]')
      expect(folded.exists()).toBe(true)
      expect(folded.text()).toContain('import-map-collapsed-from')
      expect(folded.text()).toContain('import-map-summary-match-assets')
      expect(targets()).toHaveLength(0)
      expect(document.activeElement).toBe(checkButton().element)

      await checkButton().trigger('click')
      await flushPromises()
      expect(api.putMapping).toHaveBeenCalledWith(
        'job-1',
        { name: 0, kind: 1, asset_tag: 2, serial_number: 3 },
        expect.objectContaining({ mode: 'create_update', constants: [] }),
        [4],
      )
    })

    it('opens the columns on Show columns', async () => {
      await openMap(settled())
      await wrapper!.find('[data-test="import-map-expand"]').trigger('click')
      await flushPromises()
      expect(wrapper!.find('[data-test="import-map-collapsed"]').exists()).toBe(false)
      expect(targets()).toHaveLength(5)
    })

    it('stays open coming back from Review', async () => {
      await openMap(settled())
      wrapper!.unmount()
      api.get.mockResolvedValue({ ...settled(), mapping: { name: 0, kind: 1, asset_tag: 2, serial_number: 3 }, ignored: [4] })
      await mountAt('/admin/data-import/csv?job=job-1')
      expect(wrapper!.find('[data-test="import-map-collapsed"]').exists()).toBe(false)
    })
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
    const file = new File(['PK'], 'TVS Assets 2026.xlsx', {
      type: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
    })
    await pick(file)
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

  it('stays folded when the only other sheet with rows is hidden', async () => {
    api.get.mockResolvedValue(
      workbookJob({
        sheets: [
          { name: 'Cameras', visible: true, rows: 412 },
          { name: 'Empty', visible: true, rows: 0 },
          { name: 'Lookups', visible: false, rows: 12 },
        ],
        header_row: 1,
      }),
    )
    await mountAt('/admin/data-import/csv?job=job-1')
    expect(wrapper!.find('[data-test="import-sheet-summary"]').exists()).toBe(true)
    // Opened by hand, the hidden sheet is still listed and marked.
    await wrapper!.find('[data-test="import-sheet-change"]').trigger('click')
    const options = wrapper!.findAll('[data-test="import-sheet-option"]')
    expect(options).toHaveLength(3)
    expect(options[2].find('[data-test="import-sheet-hidden"]').exists()).toBe(true)
  })

  it('opens for a header guessed below row 1 even with one visible sheet', async () => {
    api.get.mockResolvedValue(
      workbookJob({
        sheets: [
          { name: 'Cameras', visible: true, rows: 412 },
          { name: 'Lookups', visible: false, rows: 12 },
        ],
      }),
    )
    await mountAt('/admin/data-import/csv?job=job-1')
    expect(wrapper!.find('[data-test="import-sheet-summary"]').exists()).toBe(false)
    expect(wrapper!.findAll('[data-test="import-sheet-option"]')).toHaveLength(2)
  })

  it('shows no sheet panel for a CSV', async () => {
    api.get.mockResolvedValue(parsedJob())
    await mountAt('/admin/data-import/csv?job=job-1')
    expect(wrapper!.find('[data-test="import-sheet-panel"]').exists()).toBe(false)
  })
})

describe('CsvImportView Start', () => {
  it('opens on the type last used and remembers a new one', async () => {
    localStorage.setItem('nosdesk.import.lastType', 'users')
    api.upload.mockResolvedValue(parsedJob({ job_type: 'tickets' }))
    await mountAt()
    const checked = wrapper!.find('[role="radio"][data-state="checked"]')
    expect(checked.text()).toBe('csv-import-type-users')

    const tickets = wrapper!
      .findAll('[role="radio"]')
      .find((r) => r.text() === 'csv-import-type-tickets')
    await tickets!.trigger('click')
    await flushPromises()
    const file = csvFile('tickets.csv')
    await pick(file)
    await flushPromises()
    expect(api.upload).toHaveBeenCalledWith('tickets', file)
    expect(localStorage.getItem('nosdesk.import.lastType')).toBe('tickets')
  })

  it('shows the file while it is read', async () => {
    let finish: (job: ImportJob) => void = () => {}
    api.upload.mockReturnValue(new Promise<ImportJob>((resolve) => (finish = resolve)))
    await mountAt()
    await pick(new File(['x'.repeat(2048)], 'cameras.csv', { type: 'text/csv' }))
    await flushPromises()
    const reading = wrapper!.find('[data-test="import-reading"]')
    expect(reading.exists()).toBe(true)
    expect(reading.text()).toContain('import-start-reading')
    expect(reading.text()).toContain('cameras.csv · 2 KB')
    expect(wrapper!.find('[data-test="import-drop"]').exists()).toBe(false)

    finish(parsedJob())
    await flushPromises()
    expect(currentStep()).toBe('map')
  })

  it('uploads a dropped file', async () => {
    api.upload.mockResolvedValue(parsedJob())
    await mountAt()
    const file = csvFile()
    await wrapper!.find('[data-test="import-drop"]').trigger('drop', { dataTransfer: { files: [file] } })
    await flushPromises()
    expect(api.upload).toHaveBeenCalledWith('assets', file)
    expect(currentStep()).toBe('map')
  })

  it('opens the file picker from the drop zone', async () => {
    await mountAt()
    const input = wrapper!.find('[data-test="import-file"]').element as HTMLInputElement
    const click = vi.spyOn(input, 'click').mockImplementation(() => {})
    await wrapper!.find('[data-test="import-drop"]').trigger('click')
    expect(click).toHaveBeenCalled()
  })

  it('keeps a refused file on Start with its name and size', async () => {
    api.upload.mockRejectedValue({
      response: { status: 400, data: { error: 'The file has a header row and no data.' } },
    })
    await mountAt()
    await pick(new File(['name\n'], 'empty.csv', { type: 'text/csv' }))
    await flushPromises()
    expect(currentStep()).toBe('start')
    const banner = wrapper!.find('[data-test="import-upload-error"]')
    expect(banner.text()).toContain('The file has a header row and no data.')
    expect(banner.text()).toContain('empty.csv · 5 B')
    expect(router.currentRoute.value.query.job).toBeUndefined()

    // Another file takes its place.
    api.upload.mockResolvedValue(parsedJob())
    await pick(csvFile())
    await flushPromises()
    expect(currentStep()).toBe('map')
  })

  it('refuses a file that is not a spreadsheet without uploading it', async () => {
    await mountAt()
    await pick(new File(['%PDF'], 'scan.pdf', { type: 'application/pdf' }))
    await flushPromises()
    expect(api.upload).not.toHaveBeenCalled()
    expect(wrapper!.find('[data-test="import-upload-error"]').text()).toContain(
      'import-error-not-csv',
    )
  })
})

function usersLookingJob(overrides: Partial<ImportJob> = {}): ImportJob {
  const base = parsedJob()
  return parsedJob({
    filename: 'New starters.csv',
    source: {
      ...base.source!,
      headers: [
        { index: 0, name: 'First', samples: ['Alex'] },
        { index: 1, name: 'Last', samples: ['Smith'] },
        { index: 2, name: 'Email', samples: ['alex@example.com'] },
        { index: 3, name: 'Dept', samples: ['HR'] },
      ],
      row_count: 8,
    },
    suggested_mapping: [],
    ...overrides,
  })
}

describe('CsvImportView type switch', () => {
  it('offers the type the headers look like and switches to it', async () => {
    api.get.mockResolvedValue(usersLookingJob())
    api.putType.mockResolvedValue(
      usersLookingJob({
        job_type: 'users',
        suggested_mapping: [{ index: 2, field: 'email', reason: 'exact' }],
      }),
    )
    api.fields.mockImplementation(async (type: string) =>
      type === 'users'
        ? [
            { id: 'email', label: 'Email', group: 'core', value_type: 'email', required: true },
            { id: 'name', label: 'Name', group: 'core', value_type: 'text', required: true },
          ]
        : FIELDS,
    )
    await mountAt('/admin/data-import/csv?job=job-1')
    expect(currentStep()).toBe('map')
    const hint = wrapper!.find('[data-test="import-type-hint"]')
    expect(hint.text()).toContain('import-map-detect-users')
    expect(hint.text()).toContain('import-map-switch-users-help')

    await wrapper!.find('[data-test="import-switch-type"]').trigger('click')
    await flushPromises()
    expect(api.putType).toHaveBeenCalledWith('job-1', 'users')
    expect(api.fields).toHaveBeenCalledWith('users')
    expect(currentStep()).toBe('map')
    // The Map step is rebuilt for users: no hint, the new suggestion.
    expect(wrapper!.find('[data-test="import-type-hint"]').exists()).toBe(false)
    expect(targets().map((s) => (s.element as HTMLSelectElement).value)).toEqual([
      '',
      '',
      'email',
      '',
    ])
    expect(localStorage.getItem('nosdesk.import.lastType')).toBe('users')
  })

  it('says nothing when the headers fit the type', async () => {
    api.get.mockResolvedValue(parsedJob())
    await mountAt('/admin/data-import/csv?job=job-1')
    expect(wrapper!.find('[data-test="import-type-hint"]').exists()).toBe(false)
  })

  it('stays on Map with the reason when the switch is refused', async () => {
    api.get.mockResolvedValue(usersLookingJob())
    api.putType.mockRejectedValue({ response: { status: 409, data: { error: 'committed' } } })
    await mountAt('/admin/data-import/csv?job=job-1')
    await wrapper!.find('[data-test="import-switch-type"]').trigger('click')
    await flushPromises()
    expect(currentStep()).toBe('map')
    expect(wrapper!.text()).toContain('committed')
  })
})

describe('CsvImportView Change file', () => {
  async function changeFile() {
    await wrapper!.find('[data-test="import-change-file"]').trigger('click')
    expect(wrapper!.find('[data-test="confirm-message"]').text()).toBe(
      'import-map-change-file-message',
    )
    await wrapper!.find('[data-test="confirm-ok"]').trigger('click')
    await flushPromises()
  }

  it('deletes the draft and returns to Start on its type', async () => {
    api.get.mockResolvedValue(parsedJob({ job_type: 'users' }))
    api.remove.mockResolvedValue(undefined)
    await mountAt('/admin/data-import/csv?job=job-1')
    expect(currentStep()).toBe('map')
    await changeFile()
    expect(api.remove).toHaveBeenCalledWith('job-1')
    expect(currentStep()).toBe('start')
    expect(router.currentRoute.value.query.job).toBeUndefined()
    expect(wrapper!.find('[role="radio"][data-state="checked"]').text()).toBe(
      'csv-import-type-users',
    )
    expect(wrapper!.find('[data-test="import-drop"]').exists()).toBe(true)
  })

  it('treats a draft already gone as deleted', async () => {
    api.get.mockResolvedValue(parsedJob())
    api.remove.mockRejectedValue({ response: { status: 404, data: {} } })
    await mountAt('/admin/data-import/csv?job=job-1')
    await changeFile()
    expect(currentStep()).toBe('start')
  })

  it('stays on Map when the delete is refused', async () => {
    api.get.mockResolvedValue(parsedJob())
    api.remove.mockRejectedValue({ response: { status: 409, data: { error: 'already imported' } } })
    await mountAt('/admin/data-import/csv?job=job-1')
    await changeFile()
    expect(currentStep()).toBe('map')
    expect(wrapper!.text()).toContain('already imported')
    expect(router.currentRoute.value.query.job).toBe('job-1')
  })

  it('leaves an imported job alone when starting another from Done', async () => {
    api.get.mockResolvedValue(checkedJob({ status: 'done', records_committed: 10 }))
    await mountAt('/admin/data-import/csv?job=job-1')
    expect(currentStep()).toBe('done')
    await button('import-done-new').trigger('click')
    await flushPromises()
    expect(api.remove).not.toHaveBeenCalled()
    expect(currentStep()).toBe('start')
  })
})

describe('CsvImportView Match values', () => {
  async function checkFromMap() {
    api.upload.mockResolvedValue(parsedJob())
    api.putMapping.mockResolvedValue(checkedJob())
    await mountAt()
    await pick(csvFile())
    await flushPromises()
    await checkButton().trigger('click')
    await flushPromises()
  }

  it('opens after the check when a value asks for an answer, cards in source order', async () => {
    api.values.mockResolvedValue(valuesToAnswer())
    await checkFromMap()
    expect(api.values).toHaveBeenCalledWith('job-1')
    expect(currentStep()).toBe('values')
    expect(wrapper!.findAll('[data-step]').map((s) => s.attributes('data-step'))).toEqual([
      'start',
      'map',
      'values',
      'review',
      'done',
    ])
    const cards = wrapper!.findAll('[data-test="import-values-card"]')
    expect(cards.map((c) => c.attributes('data-field'))).toEqual(['kind', 'quantity'])
    // No match first, then the guess; the exact match is a count.
    const kindRows = cards[0].findAll('[data-test="import-value-row"]')
    expect(kindRows.map((r) => r.attributes('data-value-id'))).toEqual(['kind:tripod', 'kind:cam'])
    expect(kindRows[0].find('[data-test="pill-no-match"]').exists()).toBe(true)
    expect(kindRows[1].find('[data-test="pill-guessed"]').exists()).toBe(true)
    expect(cards[0].find('[data-test="import-values-matched"]').text()).toContain('import-values-matched')
  })

  it('counts live in the strip and stores the answers on Check again', async () => {
    api.values.mockResolvedValue(valuesToAnswer())
    api.putValues.mockResolvedValue(checkedJob({ updated_at: '2026-10-15T02:00:00Z' }))
    await checkFromMap()
    const needing = () => wrapper!.find('[data-test="import-values-needing"]')
    const skipped = () => wrapper!.find('[data-test="import-values-skipped"]')
    // Rows 5 and 9, three tripods and two "two" rows.
    expect(skipped().text()).toBe('import-values-strip-skipped')
    expect(skipped().classes()).toContain('text-status-error')
    expect(needing().text()).toBe('import-values-strip-columns')

    const rows = wrapper!.findAll('[data-test="import-value-row"]')
    const tripod = rows.find((r) => r.attributes('data-value-id')!.endsWith('tripod'))!
    await tripod.find('select').setValue('match:camera')
    const two = rows.find((r) => r.attributes('data-value-id')!.endsWith('two'))!
    await two.find('select').setValue('replace')
    await two.find('[data-test="import-value-replace"]').setValue('2')
    await flushPromises()
    // Only rows 5 and 9 are left; tripod no longer would be skipped.
    expect(tripod.find('[data-test="import-value-consequence"]').exists()).toBe(false)

    await wrapper!.find('[data-test="import-values-check"]').trigger('click')
    await flushPromises()
    expect(api.putValues).toHaveBeenCalledWith('job-1', {
      kind: { '': { tripod: { action: 'match', target: 'camera' } } },
      quantity: { '': { two: { action: 'replace', value: '2' } } },
    })
    expect(currentStep()).toBe('review')
  })

  it('holds Check again while a Replace with is empty', async () => {
    api.values.mockResolvedValue(valuesToAnswer())
    await checkFromMap()
    const two = wrapper!
      .findAll('[data-test="import-value-row"]')
      .find((r) => r.attributes('data-value-id')!.endsWith('two'))!
    await two.find('select').setValue('replace')
    await wrapper!.find('[data-test="import-values-check"]').trigger('click')
    await flushPromises()
    expect(api.putValues).not.toHaveBeenCalled()
    expect(wrapper!.text()).toContain('import-values-replace-missing')
  })

  it('steps to the next problem with Next problem', async () => {
    api.values.mockResolvedValue(valuesToAnswer())
    await checkFromMap()
    wrapper!.element.ownerDocument.body.appendChild(wrapper!.element)
    const next = wrapper!.find('[data-test="import-values-next"]')
    await next.trigger('click')
    await flushPromises()
    const focused = () =>
      (document.activeElement as HTMLElement | null)?.closest('[data-value-id]')?.getAttribute('data-value-id')
    expect(focused()).toBe('kind:tripod')
    await next.trigger('click')
    await flushPromises()
    expect(focused()).toBe('quantity:two')
  })

  it('skips the step when every value matched, and Review offers to show them', async () => {
    const matched = valuesToAnswer()
    matched.columns = [{ ...matched.columns[1], values: [matched.columns[1].values[2]] }]
    api.values.mockResolvedValue(matched)
    await checkFromMap()
    expect(currentStep()).toBe('review')
    expect(wrapper!.findAll('[data-step]').map((s) => s.attributes('data-step'))).not.toContain('values')
    await wrapper!.find('[data-test="import-review-see-values"]').trigger('click')
    await flushPromises()
    expect(currentStep()).toBe('values')
    expect(wrapper!.find('[data-test="import-values-settled-card"]').exists()).toBe(true)
  })

  it('goes back to Map from Match values', async () => {
    api.values.mockResolvedValue(valuesToAnswer())
    await checkFromMap()
    await wrapper!.find('[data-test="import-values-back"]').trigger('click')
    expect(currentStep()).toBe('map')
  })
})

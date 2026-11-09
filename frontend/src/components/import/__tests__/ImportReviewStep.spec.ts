import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { ChangeCount, ImportField, ImportJob, ImportJobType } from '@nosdesk/core/services/importService'

vi.mock('@nosdesk/core/services/importService', () => ({
  importService: { rows: vi.fn() },
}))

import ImportReviewStep from '@/components/import/ImportReviewStep.vue'

const FIELDS: ImportField[] = [
  { id: 'location', label: 'Location', group: 'core', value_type: 'text', required: false },
  { id: 'notes', label: 'Notes', group: 'core', value_type: 'text', required: false },
  { id: 'role', label: 'Role', group: 'core', value_type: 'enum', required: false },
]

function job(changes: ChangeCount[], jobType: ImportJobType = 'assets'): ImportJob {
  const updated = Math.max(0, ...changes.map((c) => c.count))
  return {
    id: 'job-1',
    job_type: jobType,
    status: 'done',
    filename: 'assets.csv',
    created_by: null,
    created_at: '2026-11-08T00:00:00Z',
    updated_at: '2026-11-08T00:00:00Z',
    completed_at: '2026-11-08T00:00:00Z',
    summary: {
      row_count: updated,
      would_create: 0,
      would_update: updated,
      errors: [],
      errors_truncated: false,
      changes,
    },
    records_committed: updated,
    error_message: null,
    source: null,
    suggested_mapping: [],
    mapping: null,
    options: null,
  }
}

let wrapper: VueWrapper | null = null

function mount(j: ImportJob) {
  wrapper = mountWithProviders(ImportReviewStep, { job: j, fields: FIELDS, mode: 'done' }, {}, [
    [PiniaColada, {}] as never,
  ])
  return wrapper
}

const line = (field: string) =>
  wrapper!.findAll('[data-test="import-change"]').find((l) => l.text().startsWith(field))!

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

describe('ImportReviewStep What changes', () => {
  it('shows each field with its most common change and how many others', () => {
    mount(
      job([
        {
          field: 'location',
          count: 150,
          example: { before: 'Studio A', after: 'Field kit 2' },
          others: 3,
        },
        { field: 'notes', count: 2, example: { before: null, after: 'Checked' }, others: 0 },
      ]),
    )
    const location = line('Location').find('[data-test="import-change-example"]')
    expect(location.find('[data-test="import-change-before"]').text()).toBe('Studio A')
    expect(location.find('[data-test="import-change-after"]').text()).toBe('Field kit 2')
    expect(location.find('[data-test="import-change-after"]').attributes('title')).toBe('Field kit 2')
    expect(location.find('[data-test="import-change-others"]').text()).toBe('import-review-changes-others')
    // Phones: the example takes its own line.
    expect(location.classes()).toContain('basis-full')
    expect(location.classes()).toContain('md:basis-auto')

    const notes = line('Notes').find('[data-test="import-change-example"]')
    const before = notes.find('[data-test="import-change-before"]')
    expect(before.text()).toBe('import-review-blank')
    expect(before.classes()).toContain('italic')
    expect(before.attributes('title')).toBeUndefined()
    expect(notes.find('[data-test="import-change-others"]').exists()).toBe(false)
  })

  it('reads values as rows read them', () => {
    mount(job([{ field: 'role', count: 4, example: { before: 'user', after: 'technician' }, others: 1 }], 'users'))
    const example = line('Role').find('[data-test="import-change-example"]')
    expect(example.find('[data-test="import-change-before"]').text()).toBe('user-mgmt-role-user')
    expect(example.find('[data-test="import-change-after"]').text()).toBe('user-mgmt-role-technician')
  })

  it("shows a constant's value alone", () => {
    mount(
      job([
        {
          field: 'location',
          source_column: null,
          count: 3,
          constant: true,
          example: { before: null, after: 'Depot' },
          others: 0,
        },
      ]),
    )
    const example = line('Location').find('[data-test="import-change-example"]')
    expect(example.find('[data-test="import-change-before"]').exists()).toBe(false)
    expect(example.find('[data-test="import-change-after"]').text()).toBe('Depot')
    expect(line('Location').text()).toContain('import-review-changes-constant')
  })

  it('shows counts only for an import checked before examples', () => {
    mount(job([{ field: 'location', source_column: 'Location', count: 3, constant: false }]))
    expect(line('Location').text()).toContain('import-review-changes-records')
    expect(line('Location').find('[data-test="import-change-example"]').exists()).toBe(false)
  })
})

describe('ImportReviewStep notices', () => {
  it('words the values a restore leaves as they are', () => {
    const j = job([{ field: 'location', source_column: 'location', count: 2, constant: false }])
    j.status = 'dry_run_done'
    j.summary!.notices = [{ code: 'restore_changed_since', count: 3, message: '3 values changed since this import.' }]
    wrapper = mountWithProviders(ImportReviewStep, { job: j, fields: FIELDS, mode: 'review' }, {}, [
      [PiniaColada, {}] as never,
    ])
    expect(wrapper.text()).toContain('import-review-notice-restore-changed-since')
    expect(wrapper.text()).not.toContain('3 values changed since this import.')
  })
})

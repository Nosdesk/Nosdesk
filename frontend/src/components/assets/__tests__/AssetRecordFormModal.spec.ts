import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { computed, defineComponent, h } from 'vue'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { FluentBundle } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import { createPinia } from 'pinia'
import { PiniaColada } from '@pinia/colada'
import type { AssetRecord, AssetRecordType } from '@nosdesk/core/types/asset'
import { getDateConfig, setDateConfig } from '@nosdesk/core/utils/dateUtils'
import '@/test/mountWithProviders'

// The record form: a type brings its own fields and outcomes, "Still in
// progress" sends no completion, the cost is read in the reader's locale
// and shows its currency, a date before purchase is said inline, and an
// edit sends only what changed.

const records = vi.hoisted(() => ({
  types: vi.fn(),
  catalogue: vi.fn(),
  vendors: vi.fn(),
  create: vi.fn(),
  update: vi.fn(),
  addFromCatalogue: vi.fn(),
}))

vi.mock('@nosdesk/core/services/assetRecordService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nosdesk/core/services/assetRecordService')>()),
  assetRecordService: records,
}))
vi.mock('@/services/activeWorkspace', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/services/activeWorkspace')>()),
  workspaceReady: () => true,
}))
vi.mock('@/composables/useUsersDirectory', () => ({
  useUsersDirectory: () => ({
    getUserHandle: (uuid: string) => ({
      user: computed(() => ({ uuid, name: 'Sam Okafor' })),
      status: computed(() => 'resolved'),
    }),
  }),
}))

import AssetRecordFormModal from '../AssetRecordFormModal.vue'

const select = defineComponent({
  props: { modelValue: [String, Number], options: Array, label: String, disabled: Boolean },
  emits: ['update:modelValue'],
  setup: (props, { emit }) => () =>
    h(
      'select',
      {
        'data-label': props.label,
        value: props.modelValue,
        onChange: (e: Event) => emit('update:modelValue', (e.target as HTMLSelectElement).value),
      },
      (props.options as { value: string | number; label: string }[]).map((o) =>
        h('option', { value: o.value }, o.label),
      ),
    ),
})
const input = (type: string) =>
  defineComponent({
    props: { modelValue: [String, Number, Boolean], label: String, error: [String, Boolean], disabled: Boolean },
    emits: ['update:modelValue'],
    setup: (props, { emit }) => () =>
      h('label', [
        h('input', {
          type,
          'data-label': props.label,
          value: props.modelValue,
          disabled: props.disabled,
          onInput: (e: Event) => emit('update:modelValue', (e.target as HTMLInputElement).value),
        }),
        typeof props.error === 'string' && props.error ? h('span', { 'data-error': props.label }, props.error) : null,
      ]),
  })

const stubs = {
  Modal: defineComponent({
    props: { show: Boolean, description: String },
    setup: (props, { slots }) => () =>
      h('div', [h('p', { 'data-description': '' }, props.description), slots.default?.(), slots.footer?.()]),
  }),
  BaseDropdown: select,
  SearchableDropdown: select,
  DatePicker: input('date'),
  FormInput: input('text'),
  FormNumber: input('number'),
  TimePicker: input('time'),
  Autocomplete: input('text'),
  FormTextarea: input('text'),
  Checkbox: input('checkbox'),
}

function type(slug: string, id: number, fields: Record<string, unknown>, outcomes: string[]): AssetRecordType {
  return {
    id,
    slug,
    label: null,
    description: null,
    icon: null,
    color: null,
    field_schema: { type: 'object', properties: fields },
    field_order: Object.keys(fields),
    core_fields: {},
    outcomes: outcomes.map((key) => ({ key, category: key === 'failed' ? 'not_done' : 'done' })),
    applies_to_kinds: null,
    opens_with_status: null,
    is_builtin: true,
    sort_order: id,
    archived_at: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    created_by: null,
  }
}

const REPAIR = type(
  'repair',
  1,
  { fault: { type: 'string' }, offsite: { type: 'boolean' }, expected_return: { type: 'string', format: 'date' } },
  ['repaired', 'beyond_repair'],
)
const INSPECTION = type(
  'inspection',
  2,
  { inspector_certificate: { type: 'string' }, next_due: { type: 'string', format: 'date' } },
  ['passed', 'failed'],
)

const ASSET = {
  id: 14,
  name: 'Studio A Camera 2',
  asset_tag: 'CAM-014',
  status: 'in_service',
  kind: 'device',
  purchase_date: '2024-01-01',
  location: 'Studio A',
  primary_user_uuid: null,
}

function savedRecord(overrides: Partial<AssetRecord> = {}): AssetRecord {
  return {
    id: 40,
    asset_id: 14,
    record_type_id: 1,
    record_type: 'repair',
    summary: 'Lens mount replaced',
    notes: null,
    occurred_on: '2025-08-21',
    occurred_at: '2025-08-21T01:07:31.123456Z',
    completed_on: '2025-09-04',
    completed_at: '2025-09-04T02:00:00Z',
    outcome: 'repaired',
    vendor: 'Sony',
    performed_by_uuid: null,
    reference: 'RMA-1',
    cost: { amount: '1250.50', currency: 'AUD', amount_minor: 125050 },
    ticket_id: null,
    replacement_asset_id: null,
    holder_uuid: null,
    location: null,
    snapshot_source: 'none',
    attributes: { fault: 'No SDI' },
    plugin_fields: {},
    lifecycle_event_id: 3,
    schedule_id: null,
    source: 'transition',
    source_ref: null,
    import_job_id: null,
    rule_id: null,
    recorded_at: '2025-08-21T01:07:31Z',
    recorded_by: 'dev',
    updated_at: '2025-09-04T02:00:00Z',
    updated_by: null,
    version: 3,
    files: [],
    ...overrides,
  }
}

let wrapper: VueWrapper | null = null

async function open(props: Record<string, unknown> = {}): Promise<VueWrapper> {
  wrapper = mount(AssetRecordFormModal, {
    props: { show: true, asset: ASSET, loans: [], tickets: [], ...props },
    global: {
      plugins: [
        createFluentVue({ bundles: [new FluentBundle('en-US', { useIsolating: false })] }),
        createPinia(),
        [PiniaColada, {}] as never,
      ],
      stubs,
    },
  })
  await flushPromises()
  return wrapper
}

const field = (w: VueWrapper, label: string) => w.get(`[data-label="${label}"]`)
const has = (w: VueWrapper, label: string) => w.find(`[data-label="${label}"]`).exists()
const options = (w: VueWrapper, label: string) =>
  field(w, label)
    .findAll('option')
    .map((o) => o.attributes('value'))

async function set(w: VueWrapper, label: string, value: string) {
  await field(w, label).setValue(value)
  await flushPromises()
}

async function save(w: VueWrapper) {
  const button = w
    .findAll('button')
    .find((b) => ['asset-record-form-save-new', 'asset-record-form-save'].includes(b.text()))
  await button!.trigger('click')
  await flushPromises()
}

const TYPE = 'asset-record-form-type-short'
const END = 'admin-asset-record-types-core-outcome'
const DAY = 'asset-record-form-when-short'
const COST = 'admin-asset-record-types-core-cost'

const saved = getDateConfig()
beforeEach(() => {
  setDateConfig({ defaultTimezone: 'Australia/Sydney', defaultLocale: 'en-US' })
  records.types.mockResolvedValue({ types: [REPAIR, INSPECTION], currency: 'AUD', suggested_currency: 'AUD' })
  records.catalogue.mockResolvedValue([])
  records.vendors.mockResolvedValue(['Sony'])
  records.create.mockImplementation(async () => ({ record: savedRecord(), warnings: [] }))
  records.update.mockImplementation(async () => ({ record: savedRecord(), warnings: [] }))
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  setDateConfig(saved)
  vi.clearAllMocks()
})

describe('AssetRecordFormModal', () => {
  it('shows the fields and outcomes of the type picked', async () => {
    const w = await open()
    expect(has(w, END)).toBe(false)

    await set(w, TYPE, 'repair')
    expect(has(w, 'asset-record-field-repair-fault')).toBe(true)
    expect(has(w, 'asset-record-field-repair-offsite')).toBe(true)
    expect(has(w, 'asset-record-field-repair-expected_return')).toBe(true)
    expect(options(w, END)).toEqual(['', 'repaired', 'beyond_repair', '__open'])

    await set(w, TYPE, 'inspection')
    expect(has(w, 'asset-record-field-repair-fault')).toBe(false)
    expect(has(w, 'asset-record-field-inspection-inspector_certificate')).toBe(true)
    expect(has(w, 'asset-record-field-inspection-next_due')).toBe(true)
    expect(options(w, END)).toEqual(['', 'passed', 'failed', '__open'])
  })

  it('sends no completion or outcome for work still in progress', async () => {
    const w = await open()
    await set(w, TYPE, 'repair')
    await set(w, DAY, '2025-08-21')
    await set(w, END, '__open')
    expect(field(w, 'asset-record-form-completed').attributes('disabled')).toBeDefined()
    await save(w)
    expect(records.create).toHaveBeenCalledWith(
      14,
      expect.objectContaining({ record_type: 'repair', occurred_on: '2025-08-21', completed_on: null, outcome: null }),
    )
  })

  it('completes on the work date with the outcome chosen', async () => {
    const w = await open()
    await set(w, TYPE, 'inspection')
    await set(w, DAY, '2023-05-03')
    await set(w, END, 'failed')
    await save(w)
    expect(records.create).toHaveBeenCalledWith(
      14,
      expect.objectContaining({ occurred_on: '2023-05-03', completed_on: '2023-05-03', outcome: 'failed' }),
    )
  })

  it('says inline when the work is dated before the purchase', async () => {
    const w = await open()
    await set(w, TYPE, 'repair')
    await set(w, DAY, '2023-05-03')
    expect(w.text()).toContain('asset-record-form-before-purchase')
    await set(w, DAY, '2025-05-03')
    expect(w.text()).not.toContain('asset-record-form-before-purchase')
  })

  it('shows the cost code and reads the amount in the reader locale', async () => {
    const w = await open()
    await set(w, TYPE, 'repair')
    await set(w, DAY, '2025-08-21')
    expect(w.text()).toContain('AUD')
    await set(w, COST, '1,250')
    await save(w)
    // "1,250" is a thousand and more in en-US, never 1.25.
    expect(records.create.mock.calls[0][1].cost).toEqual({ amount: '1250' })
  })

  it('refuses an amount that reads two ways in a comma locale', async () => {
    setDateConfig({ defaultLocale: 'nl-NL' })
    const w = await open()
    await set(w, TYPE, 'repair')
    await set(w, DAY, '2025-08-21')
    await set(w, COST, '1,250')
    await save(w)
    expect(records.create).not.toHaveBeenCalled()
    expect(w.get(`[data-error="${COST}"]`).text()).toBe('asset-lifecycle-record-cost-ambiguous')

    await set(w, COST, '12,50')
    await save(w)
    expect(records.create.mock.calls[0][1].cost).toEqual({ amount: '12.50' })
  })

  it('sends only what an edit changed', async () => {
    setDateConfig({ defaultLocale: 'fr-FR' })
    const w = await open({ record: savedRecord() })
    // The stored cost reads back in the reader's locale.
    expect((field(w, COST).element as HTMLInputElement).value).toBe('1250,50')
    await set(w, 'admin-asset-record-types-core-summary', 'Lens mount and SDI board replaced')
    await save(w)
    expect(records.update).toHaveBeenCalledWith(14, 40, {
      expected_version: 3,
      summary: 'Lens mount and SDI board replaced',
    })
  })

  it('sends the completion when completing open work', async () => {
    const open_ = savedRecord({ completed_on: null, completed_at: null, outcome: null, cost: null })
    const w = await open({ record: open_, completing: true })
    await set(w, END, 'repaired')
    await save(w)
    const sent = records.update.mock.calls[0][2]
    expect(sent).toMatchObject({ expected_version: 3, outcome: 'repaired' })
    expect(sent.completed_on).toMatch(/^\d{4}-\d{2}-\d{2}$/)
    expect(sent).not.toHaveProperty('occurred_at')
    expect(sent).not.toHaveProperty('summary')
  })
})

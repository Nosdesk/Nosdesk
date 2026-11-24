import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import { createPinia } from 'pinia'
import { PiniaColada } from '@pinia/colada'
import type {
  AssetRecord,
  AssetRecordCatalogueEntry,
  AssetRecordType,
} from '@nosdesk/core/types/asset'
import '@/test/mountWithProviders'
import { formatDate, getDateConfig, setDateConfig } from '@nosdesk/core/utils/dateUtils'

// The status dialog opens a record when an asset goes into a status a record
// type opens with (ticked by default, the type added from the catalogue the
// first time), and completes the open record when it comes out, asking
// which one when there are several. Save waits for what it needs to know.

const lifecycle = vi.hoisted(() => ({ transition: vi.fn(), listItadVendors: vi.fn() }))
const records = vi.hoisted(() => ({ types: vi.fn(), catalogue: vi.fn(), list: vi.fn() }))
const statuses = vi.hoisted(() => ({ list: vi.fn() }))

vi.mock('@nosdesk/core/services/assetLifecycleService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nosdesk/core/services/assetLifecycleService')>()),
  assetLifecycleService: lifecycle,
}))
vi.mock('@nosdesk/core/services/assetRecordService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nosdesk/core/services/assetRecordService')>()),
  assetRecordService: records,
}))
vi.mock('@nosdesk/core/services/assetStatusService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nosdesk/core/services/assetStatusService')>()),
  assetStatusService: statuses,
}))
vi.mock('@/services/activeWorkspace', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/services/activeWorkspace')>()),
  workspaceReady: () => true,
}))

import AssetStatusChangeModal from '../AssetStatusChangeModal.vue'

// Plain stand-ins for the controls, so the spec drives them directly.
const stubs = {
  Modal: defineComponent({
    props: { show: Boolean },
    setup: (_, { slots }) => () => h('div', [slots.default?.(), slots.footer?.()]),
  }),
  BaseDropdown: defineComponent({
    props: { modelValue: [String, Number], options: Array, label: String },
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
  }),
  Checkbox: defineComponent({
    props: { modelValue: Boolean, label: String },
    emits: ['update:modelValue'],
    setup: (props, { emit }) => () =>
      h('input', {
        type: 'checkbox',
        'data-label': props.label,
        checked: props.modelValue,
        onChange: (e: Event) => emit('update:modelValue', (e.target as HTMLInputElement).checked),
      }),
  }),
  DatePicker: defineComponent({
    props: { modelValue: String, label: String },
    emits: ['update:modelValue'],
    setup: (props, { emit }) => () =>
      h('input', {
        type: 'date',
        'data-label': props.label,
        value: props.modelValue,
        onInput: (e: Event) => emit('update:modelValue', (e.target as HTMLInputElement).value),
      }),
  }),
  FormInput: defineComponent({
    props: { modelValue: String, label: String, error: String },
    emits: ['update:modelValue'],
    setup: (props, { emit }) => () =>
      h('label', [
        h('input', {
          'data-label': props.label,
          value: props.modelValue,
          onInput: (e: Event) => emit('update:modelValue', (e.target as HTMLInputElement).value),
        }),
        props.error ? h('span', { 'data-error': props.label }, props.error) : null,
      ]),
  }),
  Autocomplete: true,
  FormTextarea: true,
}

const REPAIR_FIELDS = {
  field_schema: {
    type: 'object',
    properties: {
      fault: { type: 'string' },
      offsite: { type: 'boolean' },
      expected_return: { type: 'string', format: 'date' },
    },
  },
  field_order: ['fault', 'offsite', 'expected_return'],
  core_fields: {},
  outcomes: [
    { key: 'repaired', category: 'done' as const },
    { key: 'beyond_repair', category: 'not_done' as const },
  ],
}

const REPAIR_ENTRY: AssetRecordCatalogueEntry = {
  slug: 'repair',
  icon: 'settings',
  sort_order: 10,
  opens_with_status: 'in_repair',
  ...REPAIR_FIELDS,
  added: false,
  type: null,
}

function repairType(overrides: Partial<AssetRecordType> = {}): AssetRecordType {
  return {
    id: 3,
    slug: 'repair',
    label: null,
    description: null,
    icon: 'settings',
    color: null,
    ...REPAIR_FIELDS,
    applies_to_kinds: null,
    opens_with_status: 'in_repair',
    is_builtin: true,
    sort_order: 10,
    archived_at: null,
    created_at: '2026-11-01T00:00:00Z',
    updated_at: '2026-11-01T00:00:00Z',
    created_by: null,
    ...overrides,
  }
}

function openRecord(id: number, reference: string | null, recordTypeId = 3): AssetRecord {
  return {
    id,
    asset_id: 7,
    record_type_id: recordTypeId,
    record_type: 'repair',
    summary: null,
    notes: null,
    occurred_on: '2026-11-0' + id,
    occurred_at: null,
    completed_on: null,
    completed_at: null,
    outcome: null,
    vendor: null,
    performed_by_uuid: null,
    reference,
    cost: null,
    ticket_id: null,
    replacement_asset_id: null,
    holder_uuid: null,
    location: null,
    snapshot_source: 'none',
    attributes: {},
    plugin_fields: {},
    lifecycle_event_id: null,
    schedule_id: null,
    source: 'transition',
    source_ref: null,
    import_job_id: null,
    rule_id: null,
    recorded_at: '2026-11-01T00:00:00Z',
    recorded_by: null,
    updated_at: '2026-11-01T00:00:00Z',
    updated_by: null,
    version: 1,
    files: [],
  }
}

let wrapper: VueWrapper | null = null

/** Mount the dialog; messages resolve to their ids unless `messages` gives
 *  them text. */
async function open(
  props: { currentStatus: string; assetKind?: string },
  messages = '',
): Promise<VueWrapper> {
  const bundle = new FluentBundle('en-US', { useIsolating: false })
  if (messages) bundle.addResource(new FluentResource(messages))
  wrapper = mount(AssetStatusChangeModal, {
    props: { show: true, assetId: 7, hasActiveLoan: false, ...props },
    global: {
      plugins: [
        createFluentVue({ bundles: [bundle] }),
        createPinia(),
        [PiniaColada, {}] as never,
      ],
      stubs,
    },
  })
  await flushPromises()
  return wrapper
}

const byLabel = (w: VueWrapper, label: string) => w.get(`[data-label="${label}"]`)

async function choose(w: VueWrapper, label: string, value: string) {
  await byLabel(w, label).setValue(value)
  await flushPromises()
}

const saveButton = (w: VueWrapper) =>
  w.findAll('button').find((b) => b.text().includes('asset-lifecycle-modal-submit'))!

async function submit(w: VueWrapper) {
  const save = w.findAll('button').find((b) => b.text().includes('asset-lifecycle-modal-submit'))
  await save!.trigger('click')
  await flushPromises()
}

const sent = () => lifecycle.transition.mock.calls[0]?.[1]

beforeEach(() => {
  lifecycle.transition.mockResolvedValue({ id: 1 })
  lifecycle.listItadVendors.mockResolvedValue([])
  records.types.mockResolvedValue({ types: [], currency: 'AUD', suggested_currency: 'AUD' })
  records.catalogue.mockResolvedValue([REPAIR_ENTRY])
  records.list.mockResolvedValue({ records: [], next_cursor: null })
  // A server without the statuses endpoint, unless a case says otherwise.
  statuses.list.mockResolvedValue(null)
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.clearAllMocks()
})

describe('AssetStatusChangeModal', () => {
  it('opens a repair record, ticked, with the repair fields, even before the type is added', async () => {
    const w = await open({ currentStatus: 'in_service' })
    await choose(w, 'asset-lifecycle-modal-to-status', 'in_repair')

    const section = w.get('[data-testid="open-record"]')
    const tick = byLabel(w, 'asset-lifecycle-record-open')
    expect((tick.element as HTMLInputElement).checked).toBe(true)
    // The type's fields, titled from the catalogue, in their order.
    expect(section.text()).toMatch(
      /asset-record-field-repair-fault[\s\S]*asset-record-field-repair-offsite[\s\S]*asset-record-field-repair-expected_return/,
    )
    // The old repair inputs give way to the record's.
    expect(w.find('[data-label="asset-lifecycle-meta-rma"]').exists()).toBe(false)

    await byLabel(w, 'asset-lifecycle-record-vendor').setValue(' Camera Clinic ')
    await byLabel(w, 'asset-lifecycle-record-reference').setValue('RMA-4471')
    await submit(w)

    expect(sent()).toMatchObject({
      to_status: 'in_repair',
      metadata: {},
      record: { vendor: 'Camera Clinic', reference: 'RMA-4471', summary: null, attributes: {} },
    })
    expect(sent().complete_record).toBeUndefined()
  })

  it('opens none when unticked', async () => {
    const w = await open({ currentStatus: 'in_service' })
    await choose(w, 'asset-lifecycle-modal-to-status', 'in_repair')
    await byLabel(w, 'asset-lifecycle-record-open').setValue(false)
    await submit(w)
    expect(sent().record).toBeUndefined()
  })

  it('keeps the old repair details when no type is offered for the asset', async () => {
    records.types.mockResolvedValue({
      types: [repairType({ applies_to_kinds: ['vehicle'] })],
      currency: 'AUD',
      suggested_currency: 'AUD',
    })
    records.catalogue.mockResolvedValue([{ ...REPAIR_ENTRY, added: true }])
    const w = await open({ currentStatus: 'in_service', assetKind: 'device' })
    await choose(w, 'asset-lifecycle-modal-to-status', 'in_repair')
    expect(w.find('[data-testid="open-record"]').exists()).toBe(false)
    await byLabel(w, 'asset-lifecycle-meta-rma').setValue('RMA-9')
    await submit(w)
    expect(sent()).toMatchObject({ metadata: { rma_number: 'RMA-9' } })
    expect(sent().record).toBeUndefined()
  })

  it('completes the open record when the asset leaves repair', async () => {
    records.types.mockResolvedValue({
      types: [repairType()],
      currency: 'AUD',
      suggested_currency: 'AUD',
    })
    records.list.mockResolvedValue({ records: [openRecord(5, 'RMA-1')], next_cursor: null })
    const w = await open({ currentStatus: 'in_repair' })
    expect(records.list).toHaveBeenCalledWith(7, { open: true, leaving: 'in_repair' })

    expect(w.find('[data-testid="complete-record"]').exists()).toBe(true)
    expect(w.find('[data-label="asset-lifecycle-record-which"]').exists()).toBe(false)
    await choose(w, 'asset-lifecycle-record-outcome', 'repaired')
    await byLabel(w, 'asset-lifecycle-record-cost').setValue('1,250.50')
    await byLabel(w, 'asset-lifecycle-record-completed-on').setValue('2026-11-20')
    await submit(w)

    expect(sent().complete_record).toEqual({
      record_id: 5,
      outcome: 'repaired',
      cost: { amount: '1250.50', currency: 'AUD' },
      completed_on: '2026-11-20',
    })
    expect(sent().record).toBeUndefined()
  })

  it('asks which record to complete when several are open', async () => {
    records.types.mockResolvedValue({
      types: [repairType()],
      currency: 'AUD',
      suggested_currency: 'AUD',
    })
    records.list.mockResolvedValue({
      records: [openRecord(5, 'RMA-1'), openRecord(6, null)],
      next_cursor: null,
    })
    const w = await open({ currentStatus: 'in_repair' })
    const which = byLabel(w, 'asset-lifecycle-record-which')
    expect(which.findAll('option')).toHaveLength(2)

    await submit(w)
    expect(lifecycle.transition).not.toHaveBeenCalled()

    await choose(w, 'asset-lifecycle-record-which', '6')
    await submit(w)
    // Left on today, the record completes at the move.
    expect(sent().complete_record).toEqual({
      record_id: 6,
      outcome: null,
      cost: null,
      completed_on: null,
    })
  })
})

describe('AssetStatusChangeModal guards', () => {
  it('reads the cost in the reader\'s locale and refuses what it cannot read', async () => {
    records.types.mockResolvedValue({
      types: [repairType()],
      currency: 'USD',
      suggested_currency: 'USD',
    })
    records.list.mockResolvedValue({ records: [openRecord(5, 'RMA-1')], next_cursor: null })
    const w = await open({ currentStatus: 'in_repair' })

    // "1,250" in en-US is a thousand and more, never 1.25.
    await byLabel(w, 'asset-lifecycle-record-cost').setValue('1,250')
    await submit(w)
    expect(sent().complete_record.cost).toEqual({ amount: '1250', currency: 'USD' })

    lifecycle.transition.mockClear()
    await byLabel(w, 'asset-lifecycle-record-cost').setValue('12,50')
    await flushPromises()
    expect(w.get('[data-error="asset-lifecycle-record-cost"]').text()).toBe(
      'asset-lifecycle-record-cost-invalid',
    )
    expect(saveButton(w).attributes('disabled')).toBeDefined()
    await submit(w)
    expect(lifecycle.transition).not.toHaveBeenCalled()
  })

  it('reads cost decimals with a comma for a French reader', async () => {
    const before = getDateConfig()
    setDateConfig({ defaultLocale: 'fr-FR' })
    try {
      records.types.mockResolvedValue({
        types: [repairType()],
        currency: 'EUR',
        suggested_currency: 'EUR',
      })
      records.list.mockResolvedValue({ records: [openRecord(5, 'RMA-1')], next_cursor: null })
      const w = await open({ currentStatus: 'in_repair' })
      await byLabel(w, 'asset-lifecycle-record-cost').setValue('12,50')
      await submit(w)
      expect(sent().complete_record.cost).toEqual({ amount: '12.50', currency: 'EUR' })
    } finally {
      setDateConfig(before)
    }
  })

  it('shows a record\'s opening day as that day west of UTC', async () => {
    const before = getDateConfig()
    setDateConfig({ defaultTimezone: 'America/Los_Angeles' })
    try {
      records.types.mockResolvedValue({
        types: [repairType()],
        currency: 'AUD',
        suggested_currency: 'AUD',
      })
      records.list.mockResolvedValue({
        records: [openRecord(5, 'RMA-1'), openRecord(6, null)],
        next_cursor: null,
      })
      const w = await open(
        { currentStatus: 'in_repair' },
        'asset-lifecycle-record-choice = Opened { $date } ({ $detail })\n' +
          'asset-lifecycle-record-choice-plain = Opened { $date }\n',
      )
      const labels = byLabel(w, 'asset-lifecycle-record-which')
        .findAll('option')
        .map((o) => o.text())
      const fifth = formatDate('2026-11-05', 'date', 'UTC')
      // Read in Los Angeles, the bare day would be the 4th.
      expect(formatDate('2026-11-05', 'date')).not.toBe(fifth)
      expect(labels).toEqual([
        `Opened ${fifth} (RMA-1)`,
        `Opened ${formatDate('2026-11-06', 'date', 'UTC')}`,
      ])
    } finally {
      setDateConfig(before)
    }
  })

  it('completes a record a move into the status opened, whatever its type', async () => {
    // Calibration now opens with in_repair; the open repair predates it.
    records.types.mockResolvedValue({
      types: [
        repairType({ opens_with_status: null }),
        repairType({ id: 9, slug: 'calibration', label: 'Calibration', is_builtin: false }),
      ],
      currency: 'AUD',
      suggested_currency: 'AUD',
    })
    records.list.mockResolvedValue({ records: [openRecord(5, 'RMA-1', 3)], next_cursor: null })
    const w = await open({ currentStatus: 'in_repair' })
    expect(w.find('[data-testid="complete-record"]').exists()).toBe(true)
    expect(byLabel(w, 'asset-lifecycle-record-complete').exists()).toBe(true)
    await choose(w, 'asset-lifecycle-record-outcome', 'repaired')
    await submit(w)
    expect(sent().complete_record).toMatchObject({ record_id: 5, outcome: 'repaired' })
  })

  it('keeps Save off until the types and open records have loaded', async () => {
    let release: (value: unknown) => void = () => {}
    records.list.mockReturnValue(new Promise((resolve) => (release = resolve)))
    records.types.mockResolvedValue({
      types: [repairType()],
      currency: 'AUD',
      suggested_currency: 'AUD',
    })
    const w = await open({ currentStatus: 'in_repair' })
    await choose(w, 'asset-lifecycle-modal-to-status', 'in_service')
    expect(saveButton(w).attributes('disabled')).toBeDefined()
    await submit(w)
    expect(lifecycle.transition).not.toHaveBeenCalled()

    release({ records: [openRecord(5, 'RMA-1')], next_cursor: null })
    await flushPromises()
    expect(saveButton(w).attributes('disabled')).toBeUndefined()
    await submit(w)
    expect(sent().complete_record).toMatchObject({ record_id: 5 })
  })
})

describe('AssetStatusChangeModal targets', () => {
  const options = (w: VueWrapper) =>
    byLabel(w, 'asset-lifecycle-modal-to-status')
      .findAll('option')
      .map((o) => o.attributes('value'))

  const category = (key: string, targets: { key: string; needs: string[] }[]) => ({
    key,
    default: key === 'in_service',
    start: true,
    loanable: false,
    assignable: false,
    old_asset_target: false,
    terminal: false,
    targets,
  })

  it('offers the moves the server allows, and what each needs', async () => {
    statuses.list.mockResolvedValue({
      categories: [
        category('in_stock', [
          { key: 'in_service', needs: [] },
          { key: 'on_loan', needs: ['active_loan'] },
          { key: 'retired', needs: [] },
          { key: 'disposed', needs: ['disposal'] },
        ]),
      ],
      statuses: [],
    })
    const w = await open({ currentStatus: 'in_stock' })
    expect(options(w)).toEqual(['in_service', 'retired', 'disposed'])
    expect(options(w)).not.toContain('on_order')
  })

  it('offers on loan back only while a loan is open', async () => {
    statuses.list.mockResolvedValue({
      categories: [
        category('in_service', [
          { key: 'in_stock', needs: [] },
          { key: 'on_loan', needs: ['active_loan'] },
        ]),
      ],
      statuses: [],
    })
    wrapper = mount(AssetStatusChangeModal, {
      props: { show: true, assetId: 7, hasActiveLoan: true, currentStatus: 'in_service' },
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
    expect(options(wrapper)).toEqual(['in_stock', 'on_loan'])
  })

  it('offers every other status, as before, when the server answers 404', async () => {
    statuses.list.mockResolvedValue(null)
    const w = await open({ currentStatus: 'in_stock' })
    expect(options(w)).toEqual([
      'in_service',
      'on_order',
      'in_transit',
      'in_repair',
      'retired',
      'lost',
      'disposed',
    ])
  })
})

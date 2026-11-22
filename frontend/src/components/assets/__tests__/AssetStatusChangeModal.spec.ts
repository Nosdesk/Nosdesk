import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { FluentBundle } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import { createPinia } from 'pinia'
import { PiniaColada } from '@pinia/colada'
import type {
  AssetRecord,
  AssetRecordCatalogueEntry,
  AssetRecordType,
} from '@nosdesk/core/types/asset'
import '@/test/mountWithProviders'

// The status dialog opens a record when an asset goes into a status a record
// type opens with (ticked by default, the type added from the catalogue the
// first time), and completes the open record when it comes out, asking
// which one when there are several.

const lifecycle = vi.hoisted(() => ({ transition: vi.fn(), listItadVendors: vi.fn() }))
const records = vi.hoisted(() => ({ types: vi.fn(), catalogue: vi.fn(), list: vi.fn() }))

vi.mock('@nosdesk/core/services/assetLifecycleService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nosdesk/core/services/assetLifecycleService')>()),
  assetLifecycleService: lifecycle,
}))
vi.mock('@nosdesk/core/services/assetRecordService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nosdesk/core/services/assetRecordService')>()),
  assetRecordService: records,
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
    props: { modelValue: String, label: String },
    emits: ['update:modelValue'],
    setup: (props, { emit }) => () =>
      h('input', {
        'data-label': props.label,
        value: props.modelValue,
        onInput: (e: Event) => emit('update:modelValue', (e.target as HTMLInputElement).value),
      }),
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

function openRecord(id: number, reference: string | null): AssetRecord {
  return {
    id,
    asset_id: 7,
    record_type_id: 3,
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

async function open(props: { currentStatus: string; assetKind?: string }): Promise<VueWrapper> {
  wrapper = mount(AssetStatusChangeModal, {
    props: { show: true, assetId: 7, hasActiveLoan: false, ...props },
    global: {
      plugins: [
        createFluentVue({ bundles: [new FluentBundle('en-US')] }),
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
    expect(records.list).toHaveBeenCalledWith(7, { type: 3, open: true })

    expect(w.find('[data-testid="complete-record"]').exists()).toBe(true)
    expect(w.find('[data-label="asset-lifecycle-record-which"]').exists()).toBe(false)
    await choose(w, 'asset-lifecycle-record-outcome', 'repaired')
    await byLabel(w, 'asset-lifecycle-record-cost').setValue('120,50')
    await byLabel(w, 'asset-lifecycle-record-completed-on').setValue('2026-11-20')
    await submit(w)

    expect(sent().complete_record).toEqual({
      record_id: 5,
      outcome: 'repaired',
      cost: { amount: '120.50', currency: 'AUD' },
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

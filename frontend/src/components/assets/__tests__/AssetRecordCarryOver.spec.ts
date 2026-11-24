import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { computed, defineComponent, h } from 'vue'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import { createPinia } from 'pinia'
import { PiniaColada } from '@pinia/colada'
import type { Asset, AssetRecord, AssetRecordType, AssetTimelineTicket } from '@nosdesk/core/types/asset'
import { getDateConfig, setDateConfig } from '@nosdesk/core/utils/dateUtils'
import '@/test/mountWithProviders'

// A save that newly names a replacement asks what follows the equipment:
// open tickets ticked with their count, the holder and the rest ticked only
// when the new asset has none of them, the old asset's status unticked.
// Skip sends nothing, only what is ticked is sent, and Disposed brings the
// disposal fields. A record's received serial offers to become the asset's.

const records = vi.hoisted(() => ({
  types: vi.fn(),
  catalogue: vi.fn(),
  vendors: vi.fn(),
  create: vi.fn(),
  update: vi.fn(),
  addFromCatalogue: vi.fn(),
  carryOver: vi.fn(),
}))
const assets = vi.hoisted(() => ({ getAssetById: vi.fn(), getPaginatedAssetsExcluding: vi.fn() }))

vi.mock('@nosdesk/core/services/assetRecordService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nosdesk/core/services/assetRecordService')>()),
  assetRecordService: records,
}))
vi.mock('@nosdesk/core/services/assetLifecycleService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nosdesk/core/services/assetLifecycleService')>()),
  assetLifecycleService: { listItadVendors: vi.fn(async () => ['Shred Co']) },
}))
vi.mock('@/services/assetService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/services/assetService')>()),
  ...assets,
}))
vi.mock('@/services/activeWorkspace', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/services/activeWorkspace')>()),
  workspaceReady: () => true,
}))
vi.mock('@/composables/useUsersDirectory', () => ({
  useUsersDirectory: () => ({
    getUserHandle: (uuid: string) => ({
      user: computed(() => ({ uuid, name: uuid === 'sam' ? 'Sam Okafor' : 'Kim Lee' })),
      status: computed(() => 'resolved'),
    }),
  }),
}))

import AssetRecordFormModal from '../AssetRecordFormModal.vue'
import AssetRecordDetail from '../AssetRecordDetail.vue'

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
const input = defineComponent({
  props: { modelValue: [String, Number], label: String },
  emits: ['update:modelValue'],
  setup: (props, { emit }) => () =>
    h('input', {
      'data-label': props.label,
      value: props.modelValue,
      onInput: (e: Event) => emit('update:modelValue', (e.target as HTMLInputElement).value),
    }),
})
const checkbox = defineComponent({
  props: { modelValue: Boolean, label: String, ariaLabel: String, disabled: Boolean },
  emits: ['update:modelValue'],
  setup: (props, { emit }) => () =>
    h('input', {
      type: 'checkbox',
      'data-label': props.ariaLabel ?? props.label,
      checked: props.modelValue,
      disabled: props.disabled,
      onChange: (e: Event) => emit('update:modelValue', (e.target as HTMLInputElement).checked),
    }),
})

const stubs = {
  Modal: defineComponent({
    props: { show: Boolean, title: String, description: String },
    setup: (props, { slots }) => () =>
      h('div', [
        h('h2', { 'data-title': '' }, props.title),
        h('p', { 'data-description': '' }, props.description),
        slots.default?.(),
        slots.footer?.(),
      ]),
  }),
  BaseDropdown: select,
  SearchableDropdown: select,
  DatePicker: input,
  FormInput: input,
  TimePicker: input,
  Autocomplete: input,
  FormTextarea: input,
  Checkbox: checkbox,
}

const REPAIR: AssetRecordType = {
  id: 1,
  slug: 'repair',
  label: null,
  description: null,
  icon: null,
  color: null,
  field_schema: { type: 'object', properties: { serial_received: { type: 'string' } } },
  field_order: ['serial_received'],
  core_fields: {},
  outcomes: [
    { key: 'repaired', category: 'done' },
    { key: 'replaced', category: 'replaced' },
  ],
  applies_to_kinds: null,
  opens_with_status: null,
  is_builtin: true,
  sort_order: 1,
  archived_at: null,
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
  created_by: null,
}

const OLD = {
  id: 14,
  name: 'Studio A Camera 2',
  asset_tag: 'CAM-014',
  status: 'in_service',
  kind: 'device',
  purchase_date: null,
  location: 'Studio A',
  primary_user_uuid: 'sam' as string | null,
  managed_by_user_uuid: 'kim' as string | null,
  asset_groups: [{ id: 3, uuid: 'g3', name: 'Studio cameras' }],
}

function newAsset(overrides: Partial<Asset> = {}): Asset {
  return {
    id: 31,
    name: 'Studio A Camera 3',
    asset_tag: 'CAM-031',
    serial_number: 'SN-31',
    status: 'in_service',
    kind: 'device',
    location: null,
    primary_user_uuid: null,
    managed_by_user_uuid: null,
    asset_groups: [],
    ...overrides,
  } as Asset
}

function ticket(id: number, category: 'active' | 'triage' | 'done'): AssetTimelineTicket {
  return { id, number: id, title: `Ticket ${id}`, state: { id: 1, name: category, category } }
}
const TICKETS = [ticket(7, 'active'), ticket(8, 'triage'), ticket(9, 'done')]

function savedRecord(overrides: Partial<AssetRecord> = {}): AssetRecord {
  return {
    id: 40,
    asset_id: 14,
    record_type_id: 1,
    record_type: 'repair',
    summary: null,
    notes: null,
    occurred_on: '2025-08-21',
    occurred_at: null,
    completed_on: '2025-08-21',
    completed_at: null,
    outcome: 'replaced',
    vendor: null,
    performed_by_uuid: null,
    reference: null,
    cost: null,
    ticket_id: null,
    replacement_asset_id: 31,
    holder_uuid: null,
    location: null,
    snapshot_source: 'none',
    attributes: {},
    plugin_fields: {},
    lifecycle_event_id: null,
    schedule_id: null,
    source: 'manual',
    source_ref: null,
    import_job_id: null,
    rule_id: null,
    recorded_at: '2025-08-21T01:07:31Z',
    recorded_by: 'dev',
    updated_at: '2025-08-21T01:07:31Z',
    updated_by: null,
    version: 1,
    files: [],
    ...overrides,
  }
}

let wrapper: VueWrapper | null = null

/** Message ids read back as themselves, but for the ticket count. */
function bundle() {
  const b = new FluentBundle('en-US', { useIsolating: false })
  b.addResource(new FluentResource('asset-record-carry-tickets-detail = { $count } open on { $old }\n'))
  return b
}

async function open(props: Record<string, unknown> = {}): Promise<VueWrapper> {
  wrapper = mount(AssetRecordFormModal, {
    props: { show: true, asset: OLD, loans: [], tickets: TICKETS, ...props },
    global: {
      plugins: [
        createFluentVue({ bundles: [bundle()] }),
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
const checked = (w: VueWrapper, label: string) => (field(w, label).element as HTMLInputElement).checked

async function set(w: VueWrapper, label: string, value: string) {
  await field(w, label).setValue(value)
  await flushPromises()
}
async function tick(w: VueWrapper, label: string, value: boolean) {
  await field(w, label).setValue(value)
  await flushPromises()
}
async function press(w: VueWrapper, text: string) {
  const button = w.findAll('button').find((b) => b.text() === text)
  expect(button, text).toBeDefined()
  await button!.trigger('click')
  await flushPromises()
}

/** Save a new replaced record naming CAM-031: the step follows. */
async function saveReplaced(w: VueWrapper) {
  await set(w, 'asset-record-form-type-short', 'repair')
  await set(w, 'asset-record-form-when-short', '2025-08-21')
  await set(w, 'admin-asset-record-types-core-outcome', 'replaced')
  await set(w, 'asset-record-form-replacement', '31')
  await press(w, 'asset-record-form-save-new')
}

const TICKETS_BOX = 'asset-record-carry-tickets'
const ASSIGNMENT_BOX = 'asset-record-carry-assignment'
const MOVE_BOX = 'asset-record-carry-move'

const saved = getDateConfig()
beforeEach(() => {
  setDateConfig({ defaultTimezone: 'UTC', defaultLocale: 'en-US' })
  records.types.mockResolvedValue({ types: [REPAIR], currency: 'AUD', suggested_currency: 'AUD', timezone: 'UTC' })
  records.catalogue.mockResolvedValue([])
  records.vendors.mockResolvedValue([])
  records.create.mockImplementation(async () => ({ record: savedRecord(), warnings: [] }))
  records.update.mockImplementation(async () => ({ record: savedRecord(), warnings: [] }))
  records.carryOver.mockResolvedValue({})
  assets.getAssetById.mockResolvedValue(newAsset())
  assets.getPaginatedAssetsExcluding.mockResolvedValue({ data: [newAsset()], total: 1, page: 1, pageSize: 100 })
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  setDateConfig(saved)
  vi.clearAllMocks()
})

describe('the carry-over step', () => {
  it('ticks open tickets and an empty new asset, and leaves the status alone', async () => {
    const w = await open()
    await saveReplaced(w)

    expect(w.find('[data-testid="carry-step"]').exists()).toBe(true)
    expect(w.get('[data-title]').text()).toBe('asset-record-carry-title')
    expect(assets.getAssetById).toHaveBeenCalledWith(31)
    expect(w.emitted('saved')).toBeUndefined()

    expect(checked(w, TICKETS_BOX)).toBe(true)
    // Two of the three linked tickets are open.
    expect(w.get('[data-testid="carry-tickets-detail"]').text()).toBe('2 open on CAM-014')

    expect(checked(w, ASSIGNMENT_BOX)).toBe(true)
    const copies = w.get('[data-testid="carry-assignment-detail"]').text()
    expect(copies).toContain('Sam Okafor')
    expect(copies).toContain('asset-record-carry-managed-by')
    expect(copies).toContain('Studio A')
    expect(copies).toContain('Studio cameras')

    expect(checked(w, MOVE_BOX)).toBe(false)
    expect(has(w, 'asset-record-carry-move-to')).toBe(false)
  })

  it('leaves the copy unticked when the new asset has its own', async () => {
    assets.getAssetById.mockResolvedValue(newAsset({ location: 'Studio B' }))
    const w = await open()
    await saveReplaced(w)
    expect(checked(w, ASSIGNMENT_BOX)).toBe(false)
    expect(w.text()).toContain('asset-record-carry-assignment-replaces')
  })

  it('skips without sending anything, and hands the save on', async () => {
    const w = await open()
    await saveReplaced(w)
    await press(w, 'asset-record-carry-skip')

    expect(records.carryOver).not.toHaveBeenCalled()
    expect(w.emitted('saved')).toHaveLength(1)
    expect(w.find('[data-testid="carry-step"]').exists()).toBe(false)
  })

  it('sends only what is ticked', async () => {
    const w = await open()
    await saveReplaced(w)
    await tick(w, ASSIGNMENT_BOX, false)
    await press(w, 'asset-record-carry-apply')

    expect(records.carryOver).toHaveBeenCalledWith(14, 40, { link_open_tickets: true })
    expect(w.emitted('carried')).toHaveLength(1)
    expect(w.emitted('saved')).toHaveLength(1)
  })

  it('reveals the disposal fields for Disposed and sends them', async () => {
    const w = await open()
    await saveReplaced(w)
    await tick(w, TICKETS_BOX, false)
    await tick(w, ASSIGNMENT_BOX, false)
    await tick(w, MOVE_BOX, true)

    const status = field(w, 'asset-record-carry-move-to')
    expect(status.findAll('option').map((o) => o.attributes('value'))).toEqual(['retired', 'in_stock', 'disposed'])
    expect(has(w, 'asset-disposal-method')).toBe(false)

    await set(w, 'asset-record-carry-move-to', 'disposed')
    expect(has(w, 'asset-disposal-method')).toBe(true)
    expect(has(w, 'asset-disposal-data-bearing')).toBe(true)
    expect(has(w, 'asset-disposal-itad-vendor')).toBe(true)
    expect(has(w, 'asset-disposal-notes')).toBe(true)
    await set(w, 'asset-disposal-method', 'destroy')
    await set(w, 'asset-disposal-itad-vendor', 'Shred Co')
    await press(w, 'asset-record-carry-apply')

    expect(records.carryOver).toHaveBeenCalledWith(14, 40, {
      move_old_to: 'disposed',
      disposal: { sanitization_method: 'destroy', data_bearing: true, itad_vendor: 'Shred Co', notes: null },
    })
  })

  it('sends nothing while nothing is ticked', async () => {
    const w = await open()
    await saveReplaced(w)
    await tick(w, TICKETS_BOX, false)
    await tick(w, ASSIGNMENT_BOX, false)
    const apply = w.findAll('button').find((b) => b.text() === 'asset-record-carry-apply')
    expect(apply!.attributes('disabled')).toBeDefined()
  })

  it('has no step for an edit that keeps the replacement it had', async () => {
    const record = savedRecord({ summary: 'Swapped' })
    const w = await open({ record })
    await set(w, 'admin-asset-record-types-core-summary', 'Swapped under warranty')
    await press(w, 'asset-record-form-save')

    expect(records.update).toHaveBeenCalled()
    expect(w.find('[data-testid="carry-step"]').exists()).toBe(false)
    expect(w.emitted('saved')).toHaveLength(1)
    expect(assets.getAssetById).not.toHaveBeenCalled()
  })

  it('has no step for a record without a replacement', async () => {
    records.create.mockImplementation(async () => ({
      record: savedRecord({ outcome: 'repaired', replacement_asset_id: null }),
      warnings: [],
    }))
    const w = await open()
    await set(w, 'asset-record-form-type-short', 'repair')
    await set(w, 'asset-record-form-when-short', '2025-08-21')
    await set(w, 'admin-asset-record-types-core-outcome', 'repaired')
    await press(w, 'asset-record-form-save-new')

    expect(w.find('[data-testid="carry-step"]').exists()).toBe(false)
    expect(w.emitted('saved')).toHaveLength(1)
  })
})

describe('the serial a record received', () => {
  function detail(props: Record<string, unknown>) {
    wrapper = mount(AssetRecordDetail, {
      props: {
        record: savedRecord({ outcome: 'repaired', replacement_asset_id: null, attributes: { serial_received: 'SN-4471' } }),
        type: REPAIR,
        ...props,
      },
      global: {
        plugins: [createFluentVue({ bundles: [new FluentBundle('en-US', { useIsolating: false })] })],
        stubs: { RouterLink: true, DownloadLink: true },
      },
    })
    return wrapper
  }

  it('offers to become the asset serial when it differs', async () => {
    const w = detail({ assetSerial: 'SN-1000' })
    const offer = w.get('[data-testid="serial-offer"]')
    expect(offer.text()).toBe('asset-record-serial-offer')
    await offer.trigger('click')
    expect(w.emitted('update-serial')).toEqual([['SN-4471']])
  })

  it('offers nothing when the serial already matches or no update is allowed', () => {
    expect(detail({ assetSerial: 'SN-4471' }).find('[data-testid="serial-offer"]').exists()).toBe(false)
    wrapper!.unmount()
    expect(detail({}).find('[data-testid="serial-offer"]').exists()).toBe(false)
  })
})

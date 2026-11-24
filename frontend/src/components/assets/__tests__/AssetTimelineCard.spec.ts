import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { computed, defineComponent, h, type PropType } from 'vue'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { FluentBundle } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import { createPinia } from 'pinia'
import { PiniaColada } from '@pinia/colada'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import type {
  Asset,
  AssetLifecycleEvent,
  AssetRecord,
  AssetRecordSummary,
  AssetRecordType,
  AssetTimelineEntry,
} from '@nosdesk/core/types/asset'
import type { AssetTimelineFilter } from '@nosdesk/core/services/assetTimelineService'
import { useToastStore } from '@nosdesk/core/stores/toast'
import '@/test/mountWithProviders'

// The Timeline card: one filter menu, status marks on the entry that tells
// of each move (the status row itself, the loan, the record a transition
// opened, the arrival), the rows that carry none, and a `?record=` link
// that opens a record.

const timeline = vi.hoisted(() => ({ page: vi.fn() }))
const records = vi.hoisted(() => ({
  list: vi.fn(),
  get: vi.fn(),
  summary: vi.fn(),
  types: vi.fn(),
  catalogue: vi.fn(),
  vendors: vi.fn(),
}))
const loans = vi.hoisted(() => ({ list: vi.fn() }))

vi.mock('@nosdesk/core/services/assetTimelineService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nosdesk/core/services/assetTimelineService')>()),
  assetTimelineService: timeline,
}))
vi.mock('@nosdesk/core/services/assetRecordService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nosdesk/core/services/assetRecordService')>()),
  assetRecordService: records,
}))
vi.mock('@nosdesk/core/services/assetLoanService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nosdesk/core/services/assetLoanService')>()),
  assetLoanService: loans,
}))
vi.mock('@/services/activeWorkspace', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/services/activeWorkspace')>()),
  workspaceReady: () => true,
}))
vi.mock('@/composables/useUsersDirectory', () => ({
  useUsersDirectory: () => ({
    getUserHandle: (uuid: string) => ({
      user: computed(() => ({ uuid, name: uuid === 'sam' ? 'Sam Okafor' : 'Dev Malhotra' })),
      status: computed(() => 'resolved'),
    }),
  }),
}))
vi.mock('@/composables/useTicketNumberLookup', () => ({ numberForTicketId: (id: number) => id + 1000 }))

import AssetTimelineCard from '../AssetTimelineCard.vue'

const stubs = {
  SectionCard: defineComponent({
    setup: (_, { slots }) => () =>
      h('section', [h('header', [slots.title?.(), slots.headerActions?.()]), slots.default?.()]),
  }),
  Modal: defineComponent({
    props: { show: Boolean, title: String },
    emits: ['close'],
    setup: (props, { slots, emit }) => () =>
      props.show
        ? h('div', { 'data-modal': props.title }, [
            h('button', { 'data-close': '', onClick: () => emit('close') }),
            slots.default?.(),
            slots.footer?.(),
          ])
        : null,
  }),
  ResponsiveMenu: defineComponent({
    props: { open: Boolean, title: String },
    setup: (props, { slots }) => () => (props.open ? h('div', { 'data-menu': props.title }, slots.default?.()) : null),
  }),
  MenuList: defineComponent({
    props: { items: Array as PropType<{ id: string; label: string; checked?: boolean }[]> },
    emits: ['select'],
    setup: (props, { emit }) => () =>
      h(
        'ul',
        (props.items ?? []).map((item) =>
          h(
            'li',
            { 'data-item': item.id, 'data-checked': item.checked ? 'true' : 'false', onClick: () => emit('select', item.id) },
            item.label,
          ),
        ),
      ),
  }),
  AssetRecordFormModal: true,
  ConfirmModal: true,
}

const ASSET: Asset = {
  id: 14,
  name: 'Studio A Camera 2',
  kind: 'device',
  attributes: {},
  serial_number: 'S01',
  model: 'PXW-Z750',
  status: 'in_service',
  created_at: '2019-03-14T00:00:00Z',
  updated_at: '2026-11-14T00:00:00Z',
  asset_tag: 'CAM-014',
} as Asset

function recordType(slug: string, id: number): AssetRecordType {
  return {
    id,
    slug,
    label: null,
    description: null,
    icon: null,
    color: null,
    field_schema: { type: 'object', properties: {} },
    field_order: [],
    core_fields: {},
    outcomes: [{ key: 'done', category: 'done' }],
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

function record(id: number, slug: string, overrides: Partial<AssetRecord> = {}): AssetRecord {
  return {
    id,
    asset_id: 14,
    record_type_id: slug === 'repair' ? 1 : 2,
    record_type: slug,
    summary: `${slug} ${id}`,
    notes: null,
    occurred_on: '2025-08-21',
    occurred_at: null,
    completed_on: '2025-09-04',
    completed_at: null,
    outcome: 'done',
    vendor: null,
    performed_by_uuid: null,
    reference: null,
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
    source: 'manual',
    source_ref: null,
    import_job_id: null,
    rule_id: null,
    recorded_at: '2025-09-04T00:00:00Z',
    recorded_by: 'dev',
    updated_at: '2025-09-04T00:00:00Z',
    updated_by: null,
    version: 1,
    files: [],
    ...overrides,
  }
}

function status(
  id: number,
  from: string | null,
  to: string,
  at: string,
  extra: { reason?: string; record_ids?: number[] } = {},
): AssetTimelineEntry {
  const event: AssetLifecycleEvent = {
    id,
    asset_id: 14,
    from_status: from,
    to_status: to,
    reason: extra.reason ?? null,
    metadata: {},
    occurred_at: at,
  }
  return {
    key: `status:${id}`,
    source: 'status',
    asset_id: 14,
    day: at.slice(0, 10),
    at,
    recorded_at: at,
    recorded_by: null,
    actor_uuid: null,
    data: { ...event, disposal: null, record_ids: extra.record_ids ?? [] },
  }
}

function recordEntry(r: AssetRecord): AssetTimelineEntry {
  return {
    key: `record:${r.id}`,
    source: 'record',
    asset_id: 14,
    day: r.occurred_on,
    at: r.occurred_at,
    recorded_at: r.recorded_at,
    recorded_by: r.recorded_by,
    actor_uuid: r.recorded_by,
    data: r,
  }
}

const REPAIR = record(10, 'repair', {
  summary: 'No SDI output',
  occurred_on: '2025-08-21',
  occurred_at: '2025-08-21T01:00:00Z',
  lifecycle_event_id: 1,
  source: 'transition',
})
const INSPECTION = record(11, 'inspection', { summary: 'Annual test and tag', occurred_on: '2026-06-09' })

/** The camera: it arrived In stock, went to repair with a record, came
 *  back, was lent to Sam and inspected. */
const ENTRIES: AssetTimelineEntry[] = [
  {
    key: 'loan:5:lent',
    source: 'loan',
    kind: 'lent',
    asset_id: 14,
    day: '2026-11-12',
    at: '2026-11-12T01:00:00Z',
    recorded_at: '2026-11-12T01:00:00Z',
    recorded_by: null,
    actor_uuid: null,
    data: { id: 5, asset_id: 14, borrower_user_uuid: 'sam', loaned_at: '2026-11-12T01:00:00Z' },
  },
  status(3, 'in_service', 'on_loan', '2026-11-12T01:00:30Z'),
  recordEntry(INSPECTION),
  status(2, 'in_repair', 'in_service', '2025-09-04T02:00:00Z', { reason: 'Back from Sony' }),
  recordEntry(REPAIR),
  status(1, 'in_service', 'in_repair', '2025-08-21T01:00:00Z', { record_ids: [10] }),
  status(0, null, 'in_stock', '2019-03-14T00:00:00Z'),
  {
    key: 'marker:added:14',
    source: 'marker',
    kind: 'added',
    asset_id: 14,
    day: '2019-03-14',
    at: '2019-03-14T00:00:00Z',
    recorded_at: '2019-03-14T00:00:00Z',
    recorded_by: null,
    actor_uuid: null,
    data: {},
  },
]

const SUMMARY: AssetRecordSummary = {
  count: 2,
  costs: [],
  last_on: '2026-06-09',
  open: 0,
  by_type: [
    { record_type: 'repair', count: 1 },
    { record_type: 'inspection', count: 1 },
  ],
  by_year: {},
}

let wrapper: VueWrapper | null = null
let router: Router

async function mountCard(path = '/assets/14'): Promise<VueWrapper> {
  router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/assets/:id', component: { render: () => null } }, { path: '/:p(.*)*', component: { render: () => null } }],
  })
  await router.push(path)
  await router.isReady()
  wrapper = mount(AssetTimelineCard, {
    props: { asset: ASSET, canEdit: true, me: 'dev', isAdmin: false },
    global: {
      plugins: [
        createFluentVue({ bundles: [new FluentBundle('en-US', { useIsolating: false })] }),
        createPinia(),
        [PiniaColada, {}] as never,
        router,
      ],
      stubs,
    },
  })
  await flushPromises()
  return wrapper
}

const row = (w: VueWrapper, key: string) => w.find(`[data-entry="${key}"]`)
const markOf = (w: VueWrapper, key: string) => row(w, key).find('[role="img"]')

beforeEach(() => {
  timeline.page.mockImplementation(async (_id: number, filter: AssetTimelineFilter) => {
    const wanted = filter.sources
    return {
      entries: wanted ? ENTRIES.filter((e) => wanted.includes(e.source)) : ENTRIES,
      next_cursor: null,
    }
  })
  records.list.mockResolvedValue({ records: [], next_cursor: null })
  records.summary.mockResolvedValue(SUMMARY)
  records.types.mockResolvedValue({
    types: [recordType('repair', 1), recordType('inspection', 2)],
    currency: 'AUD',
    suggested_currency: 'AUD',
  })
  records.get.mockImplementation(async (_asset: number, id: number) =>
    record(id, 'repair', { summary: 'Lens mount replaced' }),
  )
  loans.list.mockResolvedValue([])
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.clearAllMocks()
})

describe('AssetTimelineCard', () => {
  it('marks the entry that tells of each move with the state it moved to', async () => {
    const w = await mountCard()

    // A plain status change is its own row, marked with its state.
    expect(markOf(w, 'status:2').attributes('aria-label')).toBe('asset-status-in-service')
    // The loan carries On loan; its status change is folded into it.
    expect(markOf(w, 'loan:5:lent').attributes('aria-label')).toBe('asset-status-on-loan')
    expect(row(w, 'status:3').exists()).toBe(false)
    // The repair the transition opened carries In repair and still says so.
    expect(markOf(w, 'record:10').attributes('aria-label')).toBe('asset-status-in-repair')
    expect(row(w, 'record:10').text()).toContain('asset-timeline-moved-to')
    expect(row(w, 'status:1').exists()).toBe(false)
    // The arrival marker carries the state the asset arrived in.
    expect(markOf(w, 'marker:added:14').attributes('aria-label')).toBe('asset-status-in-stock')
    // A plain record has no mark.
    expect(row(w, 'record:11').exists()).toBe(true)
    expect(markOf(w, 'record:11').exists()).toBe(false)
  })

  it('filters through one menu, with the types this asset has records of', async () => {
    const w = await mountCard()
    const trigger = w.get('[aria-label="asset-timeline-filter-aria"]')
    await trigger.trigger('click')
    const items = w.findAll('[data-menu="asset-timeline-filter-label"] [data-item]').map((i) => i.attributes('data-item'))
    expect(items).toEqual(['all', 'record', 'type:repair', 'type:inspection', 'status', 'loan', 'ticket'])

    await w.get('[data-item="loan"]').trigger('click')
    await flushPromises()
    expect(timeline.page).toHaveBeenCalledWith(14, expect.objectContaining({ sources: ['loan'] }))
    expect(row(w, 'loan:5:lent').exists()).toBe(true)
    expect(row(w, 'record:11').exists()).toBe(false)

    await trigger.trigger('click')
    await w.get('[data-item="type:repair"]').trigger('click')
    await flushPromises()
    expect(timeline.page).toHaveBeenCalledWith(
      14,
      expect.objectContaining({ sources: ['record', 'replacement'], recordType: 'repair' }),
    )
    expect(records.summary).toHaveBeenCalledWith(14, 'repair')
  })

  it('opens the record a ?record= link names', async () => {
    const w = await mountCard('/assets/14?record=77')
    expect(records.get).toHaveBeenCalledWith(14, 77)
    const modal = w.get('[data-modal="asset-record-detail-title"]')
    expect(modal.text()).toContain('Lens mount replaced')

    // Closing it drops the link, so a reload doesn't open it again.
    await modal.get('[data-close]').trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.query.record).toBeUndefined()
    expect(w.find('[data-modal="asset-record-detail-title"]').exists()).toBe(false)
  })

  it('says on save that the status stayed and what to look at again', async () => {
    const w = await mountCard()
    const saved = record(12, 'repair', { occurred_on: '2018-05-03', completed_on: '2018-05-03' })
    w.findComponent({ name: 'AssetRecordFormModal' }).vm.$emit(
      'saved',
      { record: saved, warnings: [{ code: 'before_purchase', purchase_date: '2019-03-14' }] },
      [],
      false,
    )
    await flushPromises()
    const toast = useToastStore().toasts.at(-1)
    expect(toast?.title).toBe('asset-record-saved-new')
    expect(toast?.message).toContain('asset-record-saved-status')
    expect(toast?.message).toContain('asset-record-warning-before-purchase')
    // The timeline is read again with the new record in it.
    expect(timeline.page.mock.calls.length).toBeGreaterThan(1)
  })

  it('offers Complete only on this asset\'s own open work', async () => {
    const predecessor = recordEntry(
      record(30, 'repair', { asset_id: 31, summary: 'Old camera repair', completed_on: null, outcome: null }),
    )
    const predecessorEntry = { ...predecessor, asset_id: 31 }
    timeline.page.mockImplementation(async () => ({ entries: [...ENTRIES, predecessorEntry], next_cursor: null }))
    const w = await mountCard()
    expect(row(w, 'record:30').exists()).toBe(true)
    expect(row(w, 'record:30').findAll('button').map((b) => b.text())).not.toContain('asset-timeline-complete')
  })
})

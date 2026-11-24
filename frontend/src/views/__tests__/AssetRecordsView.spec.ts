import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { AssetRecordList, AssetRecordListRow } from '@nosdesk/core/types/asset'

// The Records tab reads its filters from the address bar, asks the server
// for that set, and says what the whole set adds up to with each total
// naming its currency. Grouped, it shows the server's groups in order.

const records = vi.hoisted(() => ({
  listAll: vi.fn(),
  facets: vi.fn(),
  types: vi.fn(),
}))
vi.mock('@nosdesk/core/services/assetRecordService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@nosdesk/core/services/assetRecordService')>()),
  assetRecordService: records,
}))
vi.mock('@/services/activeWorkspace', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/services/activeWorkspace')>()),
  workspaceReady: () => true,
  activeWorkspaceSlug: () => null,
}))

import AssetRecordsView from '@/views/AssetRecordsView.vue'

function row(id: number, vendor: string | null, group?: string): AssetRecordListRow {
  return {
    id,
    asset_id: 7,
    record_type_id: 1,
    record_type: 'repair',
    summary: `Work ${id}`,
    notes: null,
    occurred_on: '2025-06-10',
    occurred_at: null,
    completed_on: id === 1 ? null : '2025-06-10',
    completed_at: null,
    outcome: null,
    vendor,
    performed_by_uuid: null,
    reference: `RMA-${id}`,
    cost: { amount: '100.00', currency: 'AUD', amount_minor: 10000 },
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
    recorded_at: '2025-06-10T00:00:00Z',
    recorded_by: null,
    updated_at: '2025-06-10T00:00:00Z',
    updated_by: null,
    version: 1,
    files: [],
    asset: { id: 7, name: 'Studio camera', asset_tag: 'CAM-014' },
    group,
  }
}

const TOTALS = [
  { currency: 'USD', amount: '320.00', amount_minor: 32000 },
  { currency: 'AUD', amount: '1500.00', amount_minor: 150000 },
]

function list(over: Partial<AssetRecordList> = {}): AssetRecordList {
  return {
    records: [row(1, 'Dell'), row(2, 'Camera Clinic')],
    total: 240,
    open: 1,
    totals: TOTALS,
    currency: 'AUD',
    groups: null,
    offset: 0,
    next_offset: 100,
    ...over,
  }
}

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
  vi.clearAllMocks()
})

async function mountAt(path: string) {
  records.facets.mockResolvedValue({ total: 240, vendors: [], assets: [] })
  records.types.mockResolvedValue({ types: [], currency: 'AUD', suggested_currency: 'AUD' })
  const page = { template: '<div />' }
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/assets/records', name: 'asset-records', component: page },
      { path: '/assets/:id', name: 'asset-view', component: page },
    ],
  })
  await router.push(path)
  await router.isReady()
  wrapper = mountWithProviders(AssetRecordsView, {}, {}, [[PiniaColada, {}] as never, router])
  await flushPromises()
  return router
}

describe('AssetRecordsView', () => {
  it('asks for the set the address bar names and says what all of it adds up to', async () => {
    records.listAll.mockResolvedValue(list())
    await mountAt('/assets/records?type=repair&status=open&vendor=Dell&period=last-year')

    expect(records.listAll).toHaveBeenCalledWith(
      expect.objectContaining({
        type: ['repair'],
        status: ['open'],
        vendor: ['Dell'],
        offset: 0,
        limit: 100,
      }),
    )
    const summary = wrapper!.get('[data-records-summary]').text()
    expect(summary).toContain('asset-records-count')
    expect(summary).toContain('asset-records-open-count')
    // Every total names its currency, the workspace's too, workspace last.
    expect(summary).toMatch(/USD.*320\.00.*AUD.*1,500\.00/s)
    expect(summary).not.toContain('$')
    expect(wrapper!.text()).toContain('asset-records-show-more')
  })

  it('shows the server groups in its order, each with its own totals', async () => {
    records.listAll.mockResolvedValue(
      list({
        records: [row(2, 'Camera Clinic', 'camera clinic'), row(1, 'Dell', 'dell')],
        groups: [
          { key: 'camera clinic', label: 'Camera Clinic', count: 3, open: 0, totals: [TOTALS[1]] },
          { key: 'dell', label: 'Dell', count: 2, open: 1, totals: [TOTALS[0]] },
        ],
      }),
    )
    await mountAt('/assets/records?group=vendor')

    expect(records.listAll).toHaveBeenCalledWith(expect.objectContaining({ group: 'vendor' }))
    const text = wrapper!.text()
    expect(text.indexOf('Camera Clinic')).toBeLessThan(text.indexOf('Dell'))
    expect(text).toMatch(/Camera Clinic\s*3/)
  })

  it('says the workspace has no records before the first one', async () => {
    records.listAll.mockResolvedValue(list({ records: [], total: 0, open: 0, totals: [], next_offset: null }))
    records.facets.mockResolvedValue({ total: 0, vendors: [], assets: [] })
    const page = { template: '<div />' }
    const router = createRouter({
      history: createMemoryHistory(),
      routes: [{ path: '/assets/records', name: 'asset-records', component: page }],
    })
    await router.push('/assets/records')
    records.types.mockResolvedValue({ types: [], currency: 'AUD', suggested_currency: 'AUD' })
    wrapper = mountWithProviders(AssetRecordsView, {}, {}, [[PiniaColada, {}] as never, router])
    await flushPromises()

    expect(wrapper.text()).toContain('asset-records-empty-title')
    expect(wrapper.find('[data-records-toolbar]').exists()).toBe(false)
  })
})

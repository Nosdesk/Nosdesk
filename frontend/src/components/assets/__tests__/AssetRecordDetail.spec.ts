import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { computed } from 'vue'
import { mount, RouterLinkStub } from '@vue/test-utils'
import { FluentBundle } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import type { AssetRecord, AssetRecordType } from '@nosdesk/core/types/asset'
import { getDateConfig, setDateConfig } from '@nosdesk/core/utils/dateUtils'
import '@/test/mountWithProviders'

// A record in full: its linked ticket reads as a reference does elsewhere,
// number and title, and a money field reads as money.

const tickets = vi.hoisted(() => ({
  numbers: new Map<number, number>(),
  titles: new Map<number, string>(),
}))
vi.mock('@/composables/useTicketNumberLookup', () => ({
  numberForTicketId: (id: number) => tickets.numbers.get(id),
  titleForTicketId: (id: number) => tickets.titles.get(id),
}))
vi.mock('@/composables/useUsersDirectory', () => ({
  useUsersDirectory: () => ({
    getUserHandle: (uuid: string) => ({
      user: computed(() => ({ uuid, name: 'Kim Lee' })),
      status: computed(() => 'resolved'),
    }),
  }),
}))

import AssetRecordDetail from '../AssetRecordDetail.vue'

const REPAIR: AssetRecordType = {
  id: 1,
  slug: 'repair',
  label: null,
  description: null,
  icon: null,
  color: null,
  field_schema: {
    type: 'object',
    properties: { parts: { type: 'object', format: 'money', title: 'Parts' } },
  },
  field_order: ['parts'],
  core_fields: {},
  outcomes: [{ key: 'repaired', category: 'done' }],
  applies_to_kinds: null,
  opens_with_status: null,
  is_builtin: false,
  sort_order: 1,
  archived_at: null,
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
  created_by: null,
}

function record(overrides: Partial<AssetRecord> = {}): AssetRecord {
  return {
    id: 11,
    asset_id: 14,
    record_type_id: 1,
    record_type: 'repair',
    summary: 'Replaced the fuser',
    notes: null,
    occurred_on: '2026-06-09',
    occurred_at: null,
    completed_on: '2026-06-09',
    completed_at: null,
    outcome: 'repaired',
    vendor: null,
    performed_by_uuid: null,
    reference: null,
    cost: null,
    ticket_id: 7,
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
    recorded_at: '2026-06-09T05:00:00Z',
    recorded_by: null,
    updated_at: '2026-06-09T05:00:00Z',
    updated_by: null,
    version: 1,
    files: [],
    ...overrides,
  } as AssetRecord
}

function render(r: AssetRecord) {
  return mount(AssetRecordDetail, {
    props: { record: r, type: REPAIR },
    global: {
      plugins: [createFluentVue({ bundles: [new FluentBundle('en-US', { useIsolating: false })] })],
      stubs: { RouterLink: RouterLinkStub, DownloadLink: true },
    },
  })
}

const saved = getDateConfig()
beforeEach(() => {
  setDateConfig({ defaultTimezone: 'UTC', defaultLocale: 'en-US' })
  tickets.numbers.clear()
  tickets.titles.clear()
})
afterEach(() => setDateConfig(saved))

describe('AssetRecordDetail', () => {
  it('names the linked ticket by number and title, linking to it', () => {
    tickets.numbers.set(7, 123)
    tickets.titles.set(7, 'Printer jams on tray 2')
    const link = render(record()).getComponent(RouterLinkStub)
    expect(link.props('to')).toBe('/tickets/123')
    expect(link.text()).toBe('#123 Printer jams on tray 2')
    // Cut to one line, with the whole reference on hover.
    expect(link.attributes('title')).toBe('#123 Printer jams on tray 2')
    expect(link.classes()).toContain('truncate')
  })

  it('falls back to the number while the title is unknown', () => {
    tickets.numbers.set(7, 123)
    const link = render(record()).getComponent(RouterLinkStub)
    expect(link.text()).toBe('#123')
  })

  it('shows a money field in its currency', () => {
    const w = render(record({ ticket_id: null, attributes: { parts: { amount: '84.20', currency: 'AUD' } } }))
    expect(w.text()).toContain('A$84.20')
    expect(w.text()).not.toContain('[object Object]')
  })
})

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { computed } from 'vue'
import { mount, RouterLinkStub } from '@vue/test-utils'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import type { AssetRecord, AssetRecordType, AssetTimelineEntry } from '@nosdesk/core/types/asset'
import { getDateConfig, setDateConfig } from '@nosdesk/core/utils/dateUtils'
import '@/test/mountWithProviders'

// One timeline entry: a record's facts and cost, the line that says when
// it was written down long after the work, the red of a failed outcome,
// the gutter mark for the state an entry moved the asset into, and a
// disposal said on its status change.

vi.mock('@/composables/useUsersDirectory', () => ({
  useUsersDirectory: () => ({
    getUserHandle: (uuid: string) => ({
      user: computed(() => ({ uuid, name: 'Dev Malhotra' })),
      status: computed(() => 'resolved'),
    }),
  }),
}))

import AssetTimelineEntryRow from '../AssetTimelineEntry.vue'

const INSPECTION: AssetRecordType = {
  id: 2,
  slug: 'inspection',
  label: null,
  description: null,
  icon: null,
  color: null,
  field_schema: { type: 'object', properties: {} },
  field_order: [],
  core_fields: {},
  outcomes: [
    { key: 'passed', category: 'done' },
    { key: 'failed', category: 'not_done' },
  ],
  applies_to_kinds: null,
  opens_with_status: null,
  is_builtin: true,
  sort_order: 20,
  archived_at: null,
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
  created_by: null,
}

function recordEntry(overrides: Partial<AssetRecord> = {}): AssetTimelineEntry {
  const data = {
    id: 11,
    asset_id: 14,
    record_type_id: 2,
    record_type: 'inspection',
    summary: 'Annual test and tag',
    notes: null,
    occurred_on: '2026-06-09',
    occurred_at: null,
    completed_on: '2026-06-09',
    completed_at: null,
    outcome: 'passed',
    vendor: 'Metro Test and Tag',
    performed_by_uuid: null,
    reference: 'TT-66120',
    cost: { amount: '38.50', currency: 'AUD', amount_minor: 3850 },
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
    recorded_at: '2026-06-09T05:00:00Z',
    recorded_by: 'dev',
    updated_at: '2026-06-09T05:00:00Z',
    updated_by: null,
    version: 1,
    files: [],
    ...overrides,
  } as AssetRecord
  return {
    key: `record:${data.id}`,
    source: 'record',
    asset_id: 14,
    day: data.occurred_on,
    at: data.occurred_at,
    recorded_at: data.recorded_at,
    recorded_by: data.recorded_by,
    actor_uuid: data.recorded_by,
    data,
  }
}

const MESSAGES = `
asset-timeline-recorded-fact = recorded { $date }
asset-timeline-imported-fact = imported { $date }
asset-timeline-moved-to = moved to { $status }
asset-timeline-status-moved = Moved to { $status }
asset-timeline-fact = { $label }: { $value }
asset-status-in-repair = In repair
asset-status-disposed = Disposed
asset-disposal-method = Sanitization method
asset-disposal-method-destroy = Destroy
asset-disposal-data-bearing = Data-bearing
`

function render(props: Record<string, unknown>) {
  const bundle = new FluentBundle('en-US', { useIsolating: false })
  bundle.addResource(new FluentResource(MESSAGES))
  return mount(AssetTimelineEntryRow, {
    props,
    global: { plugins: [createFluentVue({ bundles: [bundle] })], stubs: { RouterLink: RouterLinkStub } },
  })
}

const saved = getDateConfig()
beforeEach(() => setDateConfig({ defaultTimezone: 'UTC', defaultLocale: 'en-US' }))
afterEach(() => setDateConfig(saved))

describe('AssetTimelineEntry', () => {
  it('shows a record with its facts and its cost in its own currency', () => {
    const w = render({ entry: recordEntry(), type: INSPECTION })
    expect(w.text()).toContain('Annual test and tag')
    expect(w.text()).toContain('Metro Test and Tag')
    expect(w.text()).toContain('TT-66120')
    // The code is never dropped: AUD reads A$ in en-US.
    expect(w.text()).toContain('A$38.50')
    expect(w.text()).not.toContain('recorded')
  })

  it('says when a record was written down more than a day after the work', () => {
    const later = render({ entry: recordEntry({ recorded_at: '2026-11-02T03:14:00Z' }), type: INSPECTION })
    expect(later.text()).toContain('recorded Nov 2, 2026')

    const imported = render({
      entry: recordEntry({ recorded_at: '2026-11-02T03:14:00Z', source: 'import' }),
      type: INSPECTION,
    })
    expect(imported.text()).toContain('imported Nov 2, 2026')

    // The next morning is still the same piece of work.
    const nextDay = render({ entry: recordEntry({ recorded_at: '2026-06-10T08:00:00Z' }), type: INSPECTION })
    expect(nextDay.text()).not.toContain('recorded')
  })

  it('paints a failed outcome red and nothing else', () => {
    const w = render({ entry: recordEntry({ outcome: 'failed' }), type: INSPECTION })
    const failed = w.findAll('span').find((s) => s.text() === 'asset-record-outcome-inspection-failed')
    expect(failed?.classes()).toContain('text-status-error')
    const passed = render({ entry: recordEntry(), type: INSPECTION })
    expect(passed.find('.text-status-error').exists()).toBe(false)
  })

  it('puts the state an entry moved the asset into in the gutter', () => {
    const marked = render({ entry: recordEntry({ cost: null }), type: INSPECTION, movedTo: 'in_repair' })
    const mark = marked.get('[role="img"]')
    expect(mark.attributes('aria-label')).toBe('In repair')
    expect(mark.classes()).toContain('text-status-warning')
    // The words stay on the record too.
    expect(marked.text()).toContain('moved to In repair')

    const plain = render({ entry: recordEntry(), type: INSPECTION })
    expect(plain.find('[role="img"]').exists()).toBe(false)
  })

  it('offers Complete on an open record instead of a cost', async () => {
    const w = render({
      entry: recordEntry({ completed_on: null, outcome: null, cost: null }),
      type: INSPECTION,
      canEdit: true,
    })
    expect(w.text()).toContain('asset-timeline-in-progress')
    const complete = w.findAll('button').find((b) => b.text() === 'asset-timeline-complete')
    await complete!.trigger('click')
    expect(w.emitted('complete')).toEqual([[11]])
  })

  it('says a disposal on the status change that made it', () => {
    const entry: AssetTimelineEntry = {
      key: 'status:9',
      source: 'status',
      asset_id: 14,
      day: '2026-09-01',
      at: '2026-09-01T03:30:00Z',
      recorded_at: '2026-09-01T03:30:00Z',
      recorded_by: null,
      actor_uuid: null,
      data: {
        id: 9,
        asset_id: 14,
        from_status: 'retired',
        to_status: 'disposed',
        reason: 'E-waste collection',
        metadata: {},
        occurred_at: '2026-09-01T03:30:00Z',
        disposal: {
          id: 1,
          asset_id: 14,
          lifecycle_event_id: 9,
          sanitization_method: 'destroy',
          data_bearing: true,
          certificate_file_id: null,
          itad_vendor: 'Green Recyclers',
          notes: null,
          actor_uuid: null,
          occurred_at: '2026-09-01T03:30:00Z',
        },
      },
    }
    const w = render({ entry, movedTo: 'disposed' })
    expect(w.text()).toContain('Moved to Disposed')
    expect(w.text()).toContain('Sanitization method: Destroy')
    expect(w.text()).toContain('Data-bearing')
    expect(w.text()).toContain('Green Recyclers')
  })
})

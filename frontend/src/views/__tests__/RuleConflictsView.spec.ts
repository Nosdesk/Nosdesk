import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { RuleConflicts } from '@nosdesk/core/types/rule'

const rules = vi.hoisted(() => ({ conflicts: vi.fn() }))
vi.mock('@nosdesk/core/services/rulesService', () => ({ default: rules }))
vi.mock('@/composables/useTicketNumberLookup', () => ({ numberForTicketId: () => 1042 }))
vi.mock('@/utils/ticketNumbers', () => ({ ticketPathForId: () => '/tickets/1042' }))

import RuleConflictsView from '@/views/RuleConflictsView.vue'

const CONFLICTS: RuleConflicts = {
  since: new Date().toISOString(),
  rules: [
    { id: 2, name: 'Escalate VIP tickets', trigger_kind: 'ticket_updated', state: 'live', priority: 100 },
    { id: 5, name: 'Route urgent tickets', trigger_kind: 'ticket_updated', state: 'dry_run', priority: 100 },
  ],
  overlaps: [{ writer_rule_id: 2, reader_rule_id: 5, fields: ['ticket.priority'] }],
  co_fires: [
    {
      first_rule_id: 2,
      second_rule_id: 5,
      ticket_count: 1,
      last_co_fired_at: new Date().toISOString(),
      sample_ticket_ids: [99],
    },
  ],
}

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
  vi.clearAllMocks()
})

async function mount(): Promise<VueWrapper> {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', component: { template: '<div />' } },
      { path: '/rules', name: 'admin-rules', component: { template: '<div />' } },
      { path: '/rules/:id', name: 'admin-rules-edit', component: { template: '<div />' } },
      { path: '/tickets/:number', component: { template: '<div />' } },
    ],
  })
  const w = mountWithProviders(RuleConflictsView, {}, {}, [[PiniaColada, {}] as never, router])
  await flushPromises()
  return w
}

describe('RuleConflictsView', () => {
  it('links the rule that changes a field to the rule that checks it, and their shared tickets', async () => {
    rules.conflicts.mockResolvedValue(CONFLICTS)
    wrapper = await mount()

    expect(rules.conflicts).toHaveBeenCalledWith(7)
    const overlap = wrapper.get('[data-testid="rule-overlaps"] li')
    const links = overlap.findAll('a').map((a) => [a.text(), a.attributes('href')])
    expect(links).toEqual([
      ['Escalate VIP tickets', '/rules/2'],
      ['Route urgent tickets', '/rules/5'],
    ])
    expect(overlap.text()).toContain('admin-rules-state-dry-run')
    expect(overlap.text()).toContain('admin-rules-conflicts-overlap-detail')

    const coFire = wrapper.get('[data-testid="rule-co-fires"] li')
    expect(coFire.text()).toContain('admin-rules-conflicts-co-fire-detail')
    expect(coFire.find('a[href="/tickets/1042"]').text()).toBe('#1042')
  })

  it('says so when no automated rule is running', async () => {
    rules.conflicts.mockResolvedValue({ ...CONFLICTS, rules: [], overlaps: [], co_fires: [] })
    wrapper = await mount()

    expect(wrapper.text()).toContain('admin-rules-conflicts-none-checked-title')
    expect(wrapper.find('[data-testid="rule-overlaps"]').exists()).toBe(false)
  })

  it('says each section is clear when running rules leave each other alone', async () => {
    rules.conflicts.mockResolvedValue({ ...CONFLICTS, overlaps: [], co_fires: [] })
    wrapper = await mount()

    expect(wrapper.get('[data-testid="rule-overlaps"]').text()).toContain('admin-rules-conflicts-overlaps-empty')
    expect(wrapper.get('[data-testid="rule-co-fires"]').text()).toContain('admin-rules-conflicts-co-fires-empty')
  })
})

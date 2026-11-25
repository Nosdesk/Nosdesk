import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { Rule, RuleApplication } from '@nosdesk/core/types/rule'

const rules = vi.hoisted(() => ({ listApplications: vi.fn(), list: vi.fn() }))
vi.mock('@nosdesk/core/services/rulesService', () => ({ default: rules }))
vi.mock('@/services/userService', () => ({
  default: {
    getUsersBatch: async () => [
      { uuid: 'u-priya', name: 'Priya Shah' },
      { uuid: 'u-marcus', name: 'Marcus Chen' },
    ],
  },
}))
vi.mock('@nosdesk/core/stores/workflowStates', () => ({
  useWorkflowStatesStore: () => ({ load: async () => [], findById: () => undefined }),
}))
vi.mock('@nosdesk/core/stores/tags', () => ({
  useTagsStore: () => ({ findById: (id: number) => (id === 5 ? { id: 5, name: 'network' } : null) }),
}))
vi.mock('@/composables/useTicketNumberLookup', () => ({ numberForTicketId: () => 1042 }))
vi.mock('@/utils/ticketNumbers', () => ({ ticketPathForId: () => '/tickets/1042' }))

import RuleActivityView from '@/views/RuleActivityView.vue'

const RULE = {
  id: 2,
  name: 'Escalate to network team',
  actions: [
    { kind: 'set_priority', config: { priority: 'high' } },
    { kind: 'assign', config: { method: 'group', group_id: 3 } },
    { kind: 'add_tags', config: { tag_ids: [5] } },
    { kind: 'reply', config: { visibility: 'internal', body: 'Escalated.' } },
  ],
} as unknown as Rule

const RUN = {
  id: 7,
  rule_id: 2,
  rule_version: 1,
  ticket_id: 99,
  status: 'succeeded',
  actor_uuid: 'u-priya',
  actor_kind: 'user',
  actions_taken: [
    { index: 1, kind: 'set_priority', priority: 'high' },
    { index: 2, kind: 'assign', assigned_to_uuid: 'u-marcus' },
    { index: 4, kind: 'reply', comment_id: 31 },
  ],
  actions_skipped: [{ index: 3, reason: 'suppressed_by_override' }],
  failure_reason: null,
  applied_at: new Date().toISOString(),
} as unknown as RuleApplication

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
  vi.clearAllMocks()
})

describe('RuleActivityView', () => {
  it('names the rule and the agent, and says what the run did', async () => {
    rules.listApplications.mockResolvedValue([RUN])
    rules.list.mockResolvedValue([RULE])
    const router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/', component: { template: '<div />' } },
        { path: '/rules', name: 'admin-rules', component: { template: '<div />' } },
        { path: '/rules/:id', name: 'admin-rules-edit', component: { template: '<div />' } },
        { path: '/tickets/:number', component: { template: '<div />' } },
      ],
    })
    wrapper = mountWithProviders(RuleActivityView, {}, {}, [[PiniaColada, {}] as never, router])
    await flushPromises()

    const row = wrapper.get('li > button[aria-expanded]')
    expect(row.text()).toContain('Escalate to network team')
    expect(row.text()).toContain('admin-rules-activity-row-by-person')

    await row.trigger('click')
    await flushPromises()
    const lines = wrapper.findAll('li li').map((li) => li.text())
    expect(lines).toEqual([
      'admin-rules-activity-did-priority',
      'admin-rules-activity-did-assign',
      'admin-rules-activity-did-note',
      'admin-rules-activity-skipped',
    ])
    expect(wrapper.find('a[href="/tickets/1042"]').exists()).toBe(true)
    expect(wrapper.find('a[href="/rules/2"]').exists()).toBe(true)
  })

  it("filters to one rule's dry runs from the link, and says what each would have done", async () => {
    rules.listApplications.mockResolvedValue([
      { ...RUN, id: 8, status: 'dry_run', actor_uuid: null, actor_kind: 'system', actions_skipped: null },
    ])
    rules.list.mockResolvedValue([RULE])
    const router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/activity', component: { template: '<div />' } },
        { path: '/rules/:id', name: 'admin-rules-edit', component: { template: '<div />' } },
        { path: '/tickets/:number', component: { template: '<div />' } },
      ],
    })
    await router.push('/activity?rule=2&status=dry_run')
    wrapper = mountWithProviders(RuleActivityView, {}, {}, [[PiniaColada, {}] as never, router])
    await flushPromises()

    expect(rules.listApplications).toHaveBeenCalledWith({ status: 'dry_run', rule_id: 2, limit: 50 })
    const row = wrapper.get('li > button[aria-expanded]')
    expect(row.text()).toContain('admin-rules-activity-row-dry-run')
    await row.trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('admin-rules-activity-what-it-would-do')
    expect(wrapper.text()).not.toContain('admin-rules-activity-what-it-did')
  })

  it('says which email steps an automated run held on a ticket flagged as spam', async () => {
    rules.listApplications.mockResolvedValue([
      {
        ...RUN,
        id: 9,
        status: 'suppressed_loop_guard',
        actor_uuid: null,
        actor_kind: 'system',
        actions_taken: [{ index: 3, kind: 'add_tags', tag_ids_added: [5] }],
        actions_skipped: [{ index: 4, reason: 'spam_suspected' }],
      },
    ])
    rules.list.mockResolvedValue([RULE])
    const router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/', component: { template: '<div />' } },
        { path: '/rules/:id', name: 'admin-rules-edit', component: { template: '<div />' } },
        { path: '/tickets/:number', component: { template: '<div />' } },
      ],
    })
    wrapper = mountWithProviders(RuleActivityView, {}, {}, [[PiniaColada, {}] as never, router])
    await flushPromises()
    await wrapper.get('li > button[aria-expanded]').trigger('click')
    await flushPromises()
    const lines = wrapper.findAll('li li').map((li) => li.text())
    expect(lines).toEqual(['admin-rules-activity-did-add-tags', 'admin-rules-activity-held-spam'])
  })

  it("says a time rule's tags weren't added when its warning wasn't sent", async () => {
    rules.listApplications.mockResolvedValue([
      {
        ...RUN,
        id: 10,
        actor_uuid: null,
        actor_kind: 'system',
        actions_taken: [{ index: 4, kind: 'reply', comment_id: 31 }],
        actions_skipped: [{ index: 3, reason: 'warning_not_sent' }],
      },
    ])
    rules.list.mockResolvedValue([RULE])
    const router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/', component: { template: '<div />' } },
        { path: '/rules/:id', name: 'admin-rules-edit', component: { template: '<div />' } },
        { path: '/tickets/:number', component: { template: '<div />' } },
      ],
    })
    await router.push('/')
    wrapper = mountWithProviders(RuleActivityView, {}, {}, [[PiniaColada, {}] as never, router])
    await flushPromises()
    await wrapper.get('li > button[aria-expanded]').trigger('click')
    await flushPromises()
    const lines = wrapper.findAll('li li').map((li) => li.text())
    expect(lines).toEqual(['admin-rules-activity-did-note', 'admin-rules-activity-held-warning-unsent'])
  })
})

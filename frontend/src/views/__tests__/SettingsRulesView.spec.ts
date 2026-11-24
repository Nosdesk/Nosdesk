import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { Rule } from '@nosdesk/core/types/rule'

const rules = vi.hoisted(() => ({ list: vi.fn(), restore: vi.fn(), archive: vi.fn(), transitionState: vi.fn() }))
const toastSuccess = vi.hoisted(() => vi.fn())
vi.mock('vue-router', () => ({ useRouter: () => ({ push: vi.fn() }) }))
vi.mock('@nosdesk/core/services/rulesService', () => ({ default: rules }))
vi.mock('@nosdesk/core/stores/toast', () => ({
  useToastStore: () => ({ success: toastSuccess, error: vi.fn() }),
}))
vi.mock('@/components/admin/rules/StarterRulesDialog.vue', () => ({ default: { template: '<div />' } }))

import SettingsRulesView from '@/views/SettingsRulesView.vue'

function rule(id: number, name: string, overrides: Partial<Rule> = {}): Rule {
  return {
    id,
    workspace_id: 1,
    name,
    description: null,
    trigger_kind: 'manual',
    trigger_config: {},
    conditions: [],
    actions: [{ kind: 'set_priority', config: { priority: 'high' } }],
    reads_set: [],
    writes_set: [],
    state: 'draft',
    priority: 100,
    last_fired_at: null,
    fire_count: 0,
    created_by: null,
    created_at: '2026-10-01T00:00:00Z',
    updated_at: '2026-10-01T00:00:00Z',
    archived_at: null,
    ...overrides,
  }
}

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
  vi.clearAllMocks()
})

const button = (label: string) =>
  Array.from(document.body.querySelectorAll<HTMLButtonElement>('button')).find((b) => b.textContent?.includes(label))

async function showArchived(w: VueWrapper) {
  const vm = w.findComponent(SettingsRulesView).vm as unknown as { stateFilter: string }
  vm.stateFilter = 'archived'
  await flushPromises()
}

describe('SettingsRulesView', () => {
  it('lists archived rules only under the Archived filter, and restores one', async () => {
    rules.list.mockResolvedValue([
      rule(1, 'Escalate outages', { state: 'live' }),
      rule(2, 'Old triage', { state: 'live', archived_at: '2026-10-02T00:00:00Z' }),
      rule(3, 'Retired by state', { state: 'archived', archived_at: '2026-10-02T00:00:00Z' }),
    ])
    rules.restore.mockResolvedValue(rule(2, 'Old triage'))
    wrapper = mountWithProviders(SettingsRulesView, {}, {}, [[PiniaColada, {}] as never])
    await flushPromises()

    expect(rules.list).toHaveBeenCalledWith({ include_archived: true })
    expect(wrapper.text()).toContain('Escalate outages')
    expect(wrapper.text()).not.toContain('Old triage')
    expect(wrapper.text()).not.toContain('Retired by state')

    await showArchived(wrapper)
    expect(wrapper.text()).not.toContain('Escalate outages')
    expect(wrapper.text()).toContain('Old triage')
    expect(wrapper.text()).toContain('Retired by state')
    expect(button('admin-rules-pause')).toBeUndefined()

    button('admin-rules-restore')!.click()
    await flushPromises()
    expect(rules.restore).toHaveBeenCalledWith(2)
    expect(toastSuccess).toHaveBeenCalledWith('admin-rules-toast-restored')
  })

  it('offers Go live on manual drafts but not on event drafts while event rules do not run', async () => {
    rules.list.mockResolvedValue([
      rule(1, 'Manual draft'),
      rule(2, 'Event draft', { trigger_kind: 'ticket_created' }),
      rule(3, 'Event live', { trigger_kind: 'ticket_updated', state: 'live' }),
    ])
    wrapper = mountWithProviders(SettingsRulesView, {}, {}, [[PiniaColada, {}] as never])
    await flushPromises()

    const row = (name: string) =>
      Array.from(document.body.querySelectorAll('tbody tr')).find((r) => r.textContent?.includes(name))!
    const offers = (name: string, label: string) =>
      Array.from(row(name).querySelectorAll('button')).some((b) => b.textContent?.includes(label))
    expect(offers('Manual draft', 'admin-rules-go-live')).toBe(true)
    expect(offers('Event draft', 'admin-rules-go-live')).toBe(false)
    expect(offers('Event live', 'admin-rules-pause')).toBe(true)
  })

  it('says when a time-elapsed rule runs, under its trigger', async () => {
    rules.list.mockResolvedValue([
      rule(1, 'Nudge', {
        trigger_kind: 'time_elapsed',
        trigger_config: { clock_name: 'awaiting_requester', minutes: 4320, repeat_every_minutes: null },
      }),
      rule(2, 'Unfinished', { trigger_kind: 'time_elapsed', trigger_config: {} }),
      rule(3, 'Escalate', { trigger_kind: 'ticket_created' }),
    ])
    wrapper = mountWithProviders(SettingsRulesView, {}, {}, [[PiniaColada, {}] as never])
    await flushPromises()

    const detail = (name: string) =>
      Array.from(document.body.querySelectorAll('tbody tr'))
        .find((r) => r.textContent?.includes(name))!
        .querySelector('[data-test="trigger-detail"]')?.textContent?.trim()
    expect(detail('Nudge')).toBe('admin-rule-elapsed-summary-awaiting-requester')
    expect(detail('Unfinished')).toBeUndefined()
    expect(detail('Escalate')).toBeUndefined()
  })
})

/**
 * The editor building a time-elapsed rule: picking the trigger, its clock,
 * its time and its repeat, and saving them as the server takes them.
 */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { Rule } from '@nosdesk/core/types/rule'

const route = vi.hoisted(() => ({ name: 'admin-rules-new' as string, params: {} as Record<string, string> }))
const rules = vi.hoisted(() => ({
  get: vi.fn(),
  create: vi.fn(),
  update: vi.fn(),
  transitionState: vi.fn(),
  preview: vi.fn(),
}))
vi.mock('vue-router', () => ({ useRoute: () => route, useRouter: () => ({ push: vi.fn() }) }))
vi.mock('@nosdesk/core/services/rulesService', () => ({ default: rules }))
vi.mock('@/stores/auth', () => ({ useAuthStore: () => ({ user: { uuid: 'u-me', name: 'Me' } }) }))
vi.mock('@nosdesk/core/stores/toast', () => ({ useToastStore: () => ({ success: vi.fn(), error: vi.fn() }) }))
vi.mock('@nosdesk/core/stores/workflowStates', () => ({
  useWorkflowStatesStore: () => ({ load: async () => [], states: [] }),
}))
vi.mock('@nosdesk/core/stores/tags', () => ({ useTagsStore: () => ({ tags: [] }) }))
vi.mock('@nosdesk/core/services/groupService', () => ({ groupService: { getGroups: async () => [] } }))
vi.mock('@nosdesk/core/services/categoryService', () => ({
  categoryService: { getAllCategoriesAdmin: async () => [] },
}))
vi.mock('@/services/activeWorkspace', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/services/activeWorkspace')>()),
  workspaceReady: () => true,
}))
vi.mock('@/components/ticketComponents/UserPicker.vue', () => ({
  default: { props: ['modelValue'], template: '<input data-test="user-picker" />' },
}))

import RuleEditView from '@/views/RuleEditView.vue'
import BaseDropdown from '@/components/common/BaseDropdown.vue'
import TimeElapsedTriggerForm from '@/components/admin/rules/TimeElapsedTriggerForm.vue'

function rule(overrides: Partial<Rule> = {}): Rule {
  return {
    id: 4,
    workspace_id: 1,
    name: 'Nudge the requester',
    description: null,
    trigger_kind: 'time_elapsed',
    trigger_config: { clock_name: 'awaiting_requester', minutes: 4320, repeat_every_minutes: 120 },
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

async function mountAt(name: string, params: Record<string, string> = {}) {
  route.name = name
  route.params = params
  rules.preview.mockResolvedValue({ matched: 3, scanned: 200, sample_ticket_ids: [] })
  wrapper = mountWithProviders(RuleEditView, {}, {}, [[PiniaColada, {}] as never])
  await flushPromises()
  return wrapper
}

const button = (label: string) =>
  Array.from(document.body.querySelectorAll<HTMLButtonElement>('button')).find((b) => b.textContent?.includes(label))
const trigger = (w: VueWrapper) =>
  w.findAllComponents(BaseDropdown).find((d) => d.props('label') === 'admin-rule-editor-trigger-label')!

describe('RuleEditView with a time-elapsed trigger', () => {
  it('offers the trigger, and saves a new rule with its clock and time in minutes', async () => {
    rules.create.mockImplementation(async (req: Record<string, unknown>) => ({ ...rule(), ...req }))
    const w = await mountAt('admin-rules-new')
    const offered = (trigger(w).props('options') as { value: string }[]).map((o) => o.value)
    expect(offered).toContain('time_elapsed')

    await w.find('input').setValue('Nudge the requester')
    trigger(w).vm.$emit('update:modelValue', 'time_elapsed')
    await flushPromises()
    expect(w.find('[data-test="time-elapsed"]').exists()).toBe(true)
    expect(w.text()).toContain('admin-rule-editor-trigger-time-elapsed-summary')
    await w.find('textarea[placeholder="admin-rule-editor-reply-placeholder"]').setValue('Any update?')

    // No clock or time yet: the save points at the clock first.
    button('admin-rule-editor-save')!.click()
    await flushPromises()
    expect(rules.create).not.toHaveBeenCalled()
    expect(w.text()).toContain('admin-rule-editor-elapsed-clock-missing')

    const form = w.findComponent(TimeElapsedTriggerForm)
    form.vm.$emit('update:clockName', 'awaiting_requester')
    form.vm.$emit('update:after', { amount: 3, unit: 'days' })
    await flushPromises()
    // Complete, the trigger reads as its settings, and the preview says
    // what it doesn't check.
    expect(w.text()).toContain('admin-rule-elapsed-summary-awaiting-requester')
    expect(w.text()).toContain('admin-rule-preview-elapsed-note')
    expect(w.text()).not.toContain('admin-rule-preview-event-note')

    button('admin-rule-editor-save')!.click()
    await flushPromises()
    expect(rules.create).toHaveBeenCalledWith(
      expect.objectContaining({
        trigger_kind: 'time_elapsed',
        trigger_config: { clock_name: 'awaiting_requester', minutes: 4320, repeat_every_minutes: null },
      }),
    )
  })

  it("reads a saved rule's times in their largest unit, and turns its repeat off", async () => {
    rules.get.mockResolvedValue(rule())
    rules.update.mockImplementation(async (_id: number, req: Record<string, unknown>) => ({ ...rule(), ...req }))
    const w = await mountAt('admin-rules-edit', { id: '4' })

    const form = w.findComponent(TimeElapsedTriggerForm)
    expect(form.props('clockName')).toBe('awaiting_requester')
    expect(form.props('after')).toEqual({ amount: 3, unit: 'days' })
    expect(form.props('repeat')).toBe(true)
    expect(form.props('every')).toEqual({ amount: 2, unit: 'hours' })
    expect(w.text()).toContain('admin-rule-elapsed-summary-repeat')

    form.vm.$emit('update:repeat', false)
    await flushPromises()
    expect(w.find('[data-test="elapsed-every"]').exists()).toBe(false)
    button('admin-rule-editor-save')!.click()
    await flushPromises()
    expect(rules.update).toHaveBeenCalledWith(
      4,
      expect.objectContaining({
        trigger_config: { clock_name: 'awaiting_requester', minutes: 4320, repeat_every_minutes: null },
      }),
    )
  })

  it('refuses a repeat more often than hourly before asking the server', async () => {
    rules.get.mockResolvedValue(rule())
    const w = await mountAt('admin-rules-edit', { id: '4' })
    w.findComponent(TimeElapsedTriggerForm).vm.$emit('update:every', { amount: 30, unit: 'minutes' })
    await flushPromises()
    // Out of range shows at once, before any save.
    expect(w.text()).toContain('admin-rule-editor-elapsed-repeat-range')
    button('admin-rule-editor-save')!.click()
    await flushPromises()
    expect(rules.update).not.toHaveBeenCalled()
  })

  it("won't put a rule live on a clock the editor doesn't offer", async () => {
    rules.get.mockResolvedValue(
      rule({ trigger_config: { clock_name: 'until_sla_due_at', minutes: 60, repeat_every_minutes: null } }),
    )
    const w = await mountAt('admin-rules-edit', { id: '4' })
    button('admin-rules-dry-run')!.click()
    await flushPromises()
    expect(rules.transitionState).not.toHaveBeenCalled()
    expect(w.text()).toContain('admin-rule-editor-elapsed-clock-missing')
  })
})

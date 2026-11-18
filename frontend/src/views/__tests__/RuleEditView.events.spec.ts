/**
 * The editor with event rules running (EVENT_RULES_RUN): an event
 * rule goes to a dry run, or live straight from draft once the admin says
 * they checked it by hand.
 */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { Rule } from '@nosdesk/core/types/rule'

const route = vi.hoisted(() => ({ name: 'admin-rules-edit' as string, params: { id: '4' } as Record<string, string> }))
const rules = vi.hoisted(() => ({ get: vi.fn(), update: vi.fn(), transitionState: vi.fn(), preview: vi.fn() }))
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
const PLUGIN = '0190a5b2-0000-7000-8000-0000000000a1'
vi.mock('@nosdesk/core/services/pluginService', () => ({
  default: {
    listPlugins: async () => [
      { uuid: PLUGIN, name: 'github', display_name: 'GitHub', state: 'installed', manifest: { events: ['github:issue_linked'] } },
    ],
  },
}))
vi.mock('@/components/ticketComponents/UserPicker.vue', () => ({
  default: { props: ['modelValue'], template: '<input data-test="user-picker" />' },
}))

import RuleEditView from '@/views/RuleEditView.vue'

function rule(overrides: Partial<Rule> = {}): Rule {
  return {
    id: 4,
    workspace_id: 1,
    name: 'Escalate urgent',
    description: null,
    trigger_kind: 'ticket_created',
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

async function mount() {
  rules.preview.mockResolvedValue({ matched: 3, scanned: 200, sample_ticket_ids: [] })
  wrapper = mountWithProviders(RuleEditView, {}, {}, [[PiniaColada, {}] as never])
  await flushPromises()
  return wrapper
}

const button = (label: string) =>
  Array.from(document.body.querySelectorAll<HTMLButtonElement>('button')).find((b) => b.textContent?.includes(label))

describe('RuleEditView with event rules running', () => {
  it('starts a dry run from draft, then goes live from it', async () => {
    rules.get.mockResolvedValue(rule())
    rules.transitionState
      .mockResolvedValueOnce(rule({ state: 'dry_run' }))
      .mockResolvedValueOnce(rule({ state: 'live' }))
    const w = await mount()

    button('admin-rules-dry-run')!.click()
    await flushPromises()
    expect(rules.transitionState).toHaveBeenCalledWith(4, { state: 'dry_run' })
    expect(w.text()).toContain('admin-rule-editor-state-dry-run')
    expect(w.text()).toContain('admin-rule-editor-dry-run-results')
    // From a dry run, going live needs no extra check.
    expect(w.text()).not.toContain('admin-rule-editor-verified-manually')
    button('admin-rules-go-live')!.click()
    await flushPromises()
    expect(rules.transitionState).toHaveBeenLastCalledWith(4, { state: 'live' })
    expect(w.text()).toContain('admin-rule-editor-state-live-event')
  })

  it('goes live from draft only once the admin says they checked it by hand', async () => {
    rules.get.mockResolvedValue(rule())
    rules.transitionState.mockResolvedValue(rule({ state: 'live' }))
    await mount()
    expect(button('admin-rules-go-live')!.disabled).toBe(true)

    const check = document.body.querySelector<HTMLElement>('[role="checkbox"]')!
    check.click()
    await flushPromises()
    expect(button('admin-rules-go-live')!.disabled).toBe(false)
    button('admin-rules-go-live')!.click()
    await flushPromises()
    expect(rules.transitionState).toHaveBeenCalledWith(4, { state: 'live' })
  })

  it('saves a plugin event rule with its plugin and event, and not without them', async () => {
    rules.get.mockResolvedValue(rule({ trigger_kind: 'plugin_event', trigger_config: {} }))
    const w = await mount()
    expect(w.find('[data-test="plugin-event"]').exists()).toBe(true)
    expect(w.text()).toContain('admin-rule-editor-trigger-plugin-event-summary')

    button('admin-rule-editor-save')!.click()
    await flushPromises()
    expect(rules.update).not.toHaveBeenCalled()
    expect(w.text()).toContain('admin-rule-editor-plugin-incomplete')

    const form = w.findComponent({ name: 'PluginEventTriggerForm' })
    form.vm.$emit('update:pluginUuid', PLUGIN)
    form.vm.$emit('update:eventType', 'github:issue_linked')
    await flushPromises()
    rules.update.mockResolvedValue(
      rule({ trigger_kind: 'plugin_event', trigger_config: { plugin_uuid: PLUGIN, event_type: 'github:issue_linked' } }),
    )
    button('admin-rule-editor-save')!.click()
    await flushPromises()
    expect(rules.update).toHaveBeenCalledWith(
      4,
      expect.objectContaining({
        trigger_kind: 'plugin_event',
        trigger_config: { plugin_uuid: PLUGIN, event_type: 'github:issue_linked' },
      }),
    )
  })
})

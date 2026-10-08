import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { Rule } from '@nosdesk/core/types/rule'

const route = vi.hoisted(() => ({ name: 'admin-rules-new' as string, params: {} as Record<string, string> }))
const push = vi.hoisted(() => vi.fn())
const rules = vi.hoisted(() => ({
  get: vi.fn(),
  create: vi.fn(),
  update: vi.fn(),
  transitionState: vi.fn(),
  restore: vi.fn(),
}))
const toastSuccess = vi.hoisted(() => vi.fn())
vi.mock('vue-router', () => ({ useRoute: () => route, useRouter: () => ({ push }) }))
vi.mock('@nosdesk/core/services/rulesService', () => ({ default: rules }))
vi.mock('@nosdesk/core/stores/toast', () => ({
  useToastStore: () => ({ success: toastSuccess, error: vi.fn() }),
}))
vi.mock('@nosdesk/core/stores/workflowStates', () => ({
  useWorkflowStatesStore: () => ({ load: async () => [], states: [] }),
}))
vi.mock('@nosdesk/core/stores/tags', () => ({ useTagsStore: () => ({ tags: [] }) }))
vi.mock('@nosdesk/core/services/groupService', () => ({
  groupService: { getGroups: async () => [{ id: 3, name: 'Network' }] },
}))
vi.mock('@/components/ticketComponents/UserPicker.vue', () => ({
  default: { props: ['modelValue'], template: '<input data-test="user-picker" :value="modelValue" />' },
}))

import RuleEditView from '@/views/RuleEditView.vue'

function rule(overrides: Partial<Rule> = {}): Rule {
  return {
    id: 4,
    workspace_id: 1,
    name: 'Bump priority',
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

async function mountAt(name: string, params: Record<string, string> = {}) {
  route.name = name
  route.params = params
  wrapper = mountWithProviders(RuleEditView, {}, {}, [[PiniaColada, {}] as never])
  await flushPromises()
  return wrapper
}

const button = (label: string) =>
  Array.from(document.body.querySelectorAll<HTMLButtonElement>('button')).find((b) => b.textContent?.includes(label))

describe('RuleEditView', () => {
  it('creates a manual rule, says so, and offers Go live', async () => {
    rules.create.mockResolvedValue(rule())
    const w = await mountAt('admin-rules-new')
    // A new rule has no trigger to choose: it's always manual.
    expect(w.text()).toContain('admin-rule-editor-trigger-manual-summary')
    expect(w.text()).toContain('admin-rule-editor-state-new')
    expect(button('admin-rules-go-live')).toBeUndefined()

    await w.find('input').setValue('Bump priority')
    // The new rule's reply step is empty, so saving points at it first.
    button('admin-rule-editor-save')!.click()
    await flushPromises()
    expect(rules.create).not.toHaveBeenCalled()
    expect(w.text()).toContain('admin-rule-editor-steps-incomplete')
    expect(w.text()).toContain('admin-rule-editor-step-needs-reply')

    await w.find('textarea[placeholder="admin-rule-editor-reply-placeholder"]').setValue('Thanks, we are on it.')
    button('admin-rule-editor-save')!.click()
    await flushPromises()

    expect(toastSuccess).toHaveBeenCalledWith('admin-rules-toast-created')
    expect(push).toHaveBeenCalledWith({ name: 'admin-rules-edit', params: { id: 4 } })
    expect(w.text()).toContain('admin-rule-editor-state-draft')
    expect(button('admin-rules-go-live')).toBeDefined()
  })

  it('saves pending edits before going live, then confirms once', async () => {
    rules.get.mockResolvedValue(rule())
    rules.update.mockResolvedValue(rule({ name: 'Bump to high' }))
    rules.transitionState.mockResolvedValue(rule({ name: 'Bump to high', state: 'live' }))
    const w = await mountAt('admin-rules-edit', { id: '4' })

    await w.find('input').setValue('Bump to high')
    button('admin-rules-go-live')!.click()
    await flushPromises()

    expect(rules.update).toHaveBeenCalledWith(4, expect.objectContaining({ name: 'Bump to high' }))
    expect(rules.transitionState).toHaveBeenCalledWith(4, { state: 'live' })
    expect(toastSuccess.mock.calls.map((c) => c[0])).toEqual(['admin-rules-toast-live'])
    expect(w.text()).toContain('admin-rule-editor-state-live')
    expect(button('admin-rules-pause')).toBeDefined()
  })

  it("warns that an older event rule won't run and doesn't offer Go live", async () => {
    rules.get.mockResolvedValue(rule({ trigger_kind: 'ticket_created' }))
    const w = await mountAt('admin-rules-edit', { id: '4' })
    expect(w.text()).toContain('admin-rule-editor-trigger-other-phase')
    expect(button('admin-rules-go-live')).toBeUndefined()
  })

  it('shows an archived rule as archived, with no Go live and no saving', async () => {
    // Archived from the list: only archived_at is set; the state is as it was.
    for (const archived of [rule({ archived_at: '2026-10-02T00:00:00Z' }), rule({ state: 'live', archived_at: '2026-10-02T00:00:00Z' })]) {
      rules.get.mockResolvedValue(archived)
      const w = await mountAt('admin-rules-edit', { id: '4' })
      expect(w.text()).toContain('admin-rule-editor-state-archived')
      expect(w.text()).not.toContain('admin-rule-editor-state-draft')
      expect(w.text()).not.toContain('admin-rule-editor-state-live')
      expect(button('admin-rules-go-live')).toBeUndefined()
      expect(button('admin-rules-pause')).toBeUndefined()
      expect(button('admin-rule-editor-save')!.disabled).toBe(true)
      // The form itself is read-only too.
      expect(w.find('input').element.matches(':disabled')).toBe(true)
      expect(w.find('textarea').element.matches(':disabled')).toBe(true)
      w.unmount()
      wrapper = null
      document.body.innerHTML = ''
    }
  })

  it('restores an archived rule as an editable draft', async () => {
    rules.get.mockResolvedValue(rule({ state: 'live', archived_at: '2026-10-02T00:00:00Z' }))
    rules.restore.mockResolvedValue(rule())
    const w = await mountAt('admin-rules-edit', { id: '4' })

    button('admin-rules-restore')!.click()
    await flushPromises()

    expect(rules.restore).toHaveBeenCalledWith(4)
    expect(toastSuccess.mock.calls.map((c) => c[0])).toEqual(['admin-rules-toast-restored'])
    expect(w.text()).toContain('admin-rule-editor-state-draft')
    expect(button('admin-rules-restore')).toBeUndefined()
    expect(button('admin-rules-go-live')).toBeDefined()
    expect(w.find('input').element.matches(':disabled')).toBe(false)
  })

  it('shows the rule as archived when a save finds it was archived elsewhere', async () => {
    rules.get
      .mockResolvedValueOnce(rule())
      .mockResolvedValueOnce(rule({ archived_at: '2026-10-02T00:00:00Z' }))
    rules.update.mockRejectedValue({
      response: { status: 409, data: { code: 'RULE_ARCHIVED', message: 'rule 4 is archived' } },
    })
    const w = await mountAt('admin-rules-edit', { id: '4' })

    await w.find('input').setValue('Bump to high')
    button('admin-rule-editor-save')!.click()
    await flushPromises()

    expect(rules.get).toHaveBeenCalledTimes(2)
    expect(w.text()).toContain('admin-rule-editor-state-archived')
    expect(button('admin-rules-go-live')).toBeUndefined()
  })

  it('assigns a step to a team', async () => {
    rules.get.mockResolvedValue(rule({ actions: [{ kind: 'assign', config: { method: 'direct', user_uuid: '' } }] }))
    rules.update.mockResolvedValue(rule())
    const w = await mountAt('admin-rules-edit', { id: '4' })
    expect(w.find('[data-test="user-picker"]').exists()).toBe(true)

    const team = Array.from(document.body.querySelectorAll<HTMLElement>('[role="radio"], button'))
      .find((b) => b.textContent?.trim() === 'admin-rule-editor-assign-team')
    team!.click()
    await flushPromises()
    expect(w.find('[data-test="user-picker"]').exists()).toBe(false)
    expect(w.text()).toContain('admin-rule-editor-team-hint')

    button('admin-rule-editor-save')!.click()
    await flushPromises()
    expect(w.text()).toContain('admin-rule-editor-step-needs-team')
    expect(rules.update).not.toHaveBeenCalled()
  })
})

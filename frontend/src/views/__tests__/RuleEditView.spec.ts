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
  preview: vi.fn(),
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
vi.mock('@/services/activeWorkspace', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/services/activeWorkspace')>()),
  workspaceReady: () => true,
}))
vi.mock('@nosdesk/core/services/categoryService', () => ({
  categoryService: { getAllCategoriesAdmin: async () => [] },
}))
vi.mock('@/composables/useTicketNumberLookup', () => ({ numberForTicketId: (id: number) => id + 1000 }))
vi.mock('@/utils/ticketNumbers', () => ({ ticketPathForId: (id: number) => `/tickets/${id + 1000}` }))

import RuleEditView from '@/views/RuleEditView.vue'
import BaseDropdown from '@/components/common/BaseDropdown.vue'
import type { RulePreview } from '@nosdesk/core/types/rule'

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
    // A new rule starts manual, with no conditions to set.
    expect(w.text()).toContain('admin-rule-editor-trigger-manual-summary')
    expect(w.find('[data-test="conditions"]').exists()).toBe(false)
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

  it('pauses a live rule as paused and says so', async () => {
    rules.get.mockResolvedValue(rule({ state: 'live' }))
    rules.transitionState.mockResolvedValue(rule({ state: 'paused' }))
    const w = await mountAt('admin-rules-edit', { id: '4' })

    button('admin-rules-pause')!.click()
    await flushPromises()

    expect(rules.transitionState).toHaveBeenCalledWith(4, { state: 'paused' })
    expect(toastSuccess.mock.calls.map((c) => c[0])).toEqual(['admin-rules-toast-paused'])
    expect(w.text()).toContain('admin-rule-editor-state-paused')
    expect(button('admin-rules-go-live')).toBeDefined()
  })

  it('offers an event draft rule a dry run, or going live once checked by hand', async () => {
    rules.get.mockResolvedValue(rule({ trigger_kind: 'ticket_created' }))
    rules.preview.mockResolvedValue(PREVIEW)
    const w = await mountAt('admin-rules-edit', { id: '4' })
    expect(w.text()).toContain('admin-rule-editor-state-draft-event')
    expect(button('admin-rules-dry-run')).toBeDefined()
    expect(button('admin-rules-go-live')).toBeDefined()
    expect(w.text()).toContain('admin-rule-editor-verified-manually')
  })

  it('keeps a manual draft rule able to go live, with no dry run', async () => {
    rules.get.mockResolvedValue(rule())
    await mountAt('admin-rules-edit', { id: '4' })
    expect(button('admin-rules-go-live')).toBeDefined()
    expect(button('admin-rules-dry-run')).toBeUndefined()
  })

  it('lets a running event rule be paused, and holds its trigger until then', async () => {
    rules.get.mockResolvedValue(rule({ trigger_kind: 'ticket_created', state: 'live' }))
    rules.preview.mockResolvedValue(PREVIEW)
    rules.transitionState.mockResolvedValue(rule({ trigger_kind: 'ticket_created', state: 'paused' }))
    const w = await mountAt('admin-rules-edit', { id: '4' })
    expect(w.text()).toContain('admin-rule-editor-trigger-locked')
    expect(button('admin-rules-go-live')).toBeUndefined()
    button('admin-rules-pause')!.click()
    await flushPromises()
    expect(rules.transitionState).toHaveBeenCalledWith(4, { state: 'paused' })
    // Paused, it can start again.
    expect(button('admin-rules-go-live')).toBeDefined()
    expect(button('admin-rules-dry-run')).toBeDefined()
    expect(w.text()).not.toContain('admin-rule-editor-trigger-locked')
  })

  it('saves the changed fields of an updated trigger, and the conditions, as loaded plus the edit', async () => {
    const conditions = {
      kind: 'and' as const,
      children: [{ kind: 'leaf' as const, field: 'ticket.priority', op: 'eq' as const, value: 'high' }],
    }
    const loaded = rule({
      trigger_kind: 'ticket_updated',
      trigger_config: { changed_fields_include: ['ticket.priority'] },
      conditions,
    })
    rules.get.mockResolvedValue(loaded)
    rules.preview.mockResolvedValue(PREVIEW)
    rules.update.mockImplementation(async (_id: number, req: Record<string, unknown>) => ({ ...loaded, ...req, preview: PREVIEW }))
    await mountAt('admin-rules-edit', { id: '4' })

    const chip = (field: string) =>
      Array.from(document.body.querySelectorAll<HTMLButtonElement>('[data-test="changed-fields"] button')).find(
        (b) => b.textContent?.includes(`admin-rule-conditions-field-${field}`),
      )!
    expect(chip('ticket-priority').getAttribute('aria-pressed')).toBe('true')
    expect(chip('ticket-assignee-uuid').getAttribute('aria-pressed')).toBe('false')
    chip('ticket-assignee-uuid').click()
    await flushPromises()

    button('admin-rule-editor-save')!.click()
    await flushPromises()
    expect(rules.update).toHaveBeenCalledWith(
      4,
      expect.objectContaining({
        trigger_kind: 'ticket_updated',
        trigger_config: { changed_fields_include: ['ticket.priority', 'ticket.assignee_uuid'] },
        conditions,
      }),
    )
    // Saved as shown, so nothing is left to save.
    expect(chip('ticket-assignee-uuid').getAttribute('aria-pressed')).toBe('true')
  })

  it('saves whose replies run a replied rule', async () => {
    const loaded = rule({ trigger_kind: 'ticket_replied', trigger_config: { reply_kind: 'agent' } })
    rules.get.mockResolvedValue(loaded)
    rules.preview.mockResolvedValue(PREVIEW)
    rules.update.mockImplementation(async (_id: number, req: Record<string, unknown>) => ({ ...loaded, ...req, preview: PREVIEW }))
    await mountAt('admin-rules-edit', { id: '4' })

    const radio = (label: string) =>
      Array.from(document.body.querySelectorAll<HTMLElement>('[data-test="reply-kind"] [role="radio"]')).find(
        (b) => b.textContent?.trim() === label,
      )!
    expect(radio('admin-rule-editor-reply-kind-agent').getAttribute('aria-checked')).toBe('true')
    radio('admin-rule-editor-reply-kind-any').click()
    await flushPromises()
    button('admin-rule-editor-save')!.click()
    await flushPromises()
    expect(rules.update).toHaveBeenCalledWith(4, expect.objectContaining({ trigger_config: { reply_kind: 'any' }, conditions: [] }))
  })

  it('creates an event rule with its trigger settings, swapping the reply step for one it can run', async () => {
    rules.preview.mockResolvedValue(PREVIEW)
    rules.create.mockImplementation(async (req: Record<string, unknown>) => ({ ...rule(), ...req, preview: PREVIEW }))
    const w = await mountAt('admin-rules-new')
    await w.find('input').setValue('Escalate replies')
    trigger(w).vm.$emit('update:modelValue', 'ticket_replied')
    await flushPromises()
    expect(w.find('[data-test="conditions"]').exists()).toBe(true)
    // The reply step became a status step, which needs a status.
    button('admin-rule-editor-save')!.click()
    await flushPromises()
    expect(w.text()).toContain('admin-rule-editor-step-needs-status')
    expect(rules.create).not.toHaveBeenCalled()
  })

  it('switching an event rule to manual saves no conditions and no trigger settings', async () => {
    const loaded = rule({
      trigger_kind: 'ticket_updated',
      trigger_config: { changed_fields_include: ['ticket.priority'] },
      conditions: { kind: 'and', children: [{ kind: 'leaf', field: 'ticket.priority', op: 'eq', value: 'high' }] },
    })
    rules.get.mockResolvedValue(loaded)
    rules.preview.mockResolvedValue(PREVIEW)
    rules.update.mockImplementation(async (_id: number, req: Record<string, unknown>) => ({ ...loaded, ...req, preview: PREVIEW }))
    const w = await mountAt('admin-rules-edit', { id: '4' })
    trigger(w).vm.$emit('update:modelValue', 'manual')
    await flushPromises()
    expect(w.find('[data-test="conditions"]').exists()).toBe(false)
    button('admin-rule-editor-save')!.click()
    await flushPromises()
    expect(rules.update).toHaveBeenCalledWith(
      4,
      expect.objectContaining({ trigger_kind: 'manual', trigger_config: {}, conditions: [] }),
    )
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

  it('saves a lend step with how many days the loan runs', async () => {
    rules.get.mockResolvedValue(rule({ actions: [{ kind: 'lend_assets', config: {} }] }))
    rules.update.mockResolvedValue(rule())
    const w = await mountAt('admin-rules-edit', { id: '4' })
    expect(w.text()).toContain('admin-rule-editor-assets-lend-hint')

    const input = w.find<HTMLInputElement>('input[role="spinbutton"]').element
    input.value = '14'
    input.dispatchEvent(new Event('input', { bubbles: true }))
    input.dispatchEvent(new Event('blur', { bubbles: true }))
    await flushPromises()
    button('admin-rule-editor-save')!.click()
    await flushPromises()
    expect(rules.update).toHaveBeenCalledWith(
      4,
      expect.objectContaining({ actions: [{ kind: 'lend_assets', config: { due_in_days: 14 } }] }),
    )
  })

  it('offers the asset steps, which need no settings to save', async () => {
    rules.get.mockResolvedValue(rule({ actions: [{ kind: 'return_requester_assets', config: {} }] }))
    rules.update.mockResolvedValue(rule())
    const w = await mountAt('admin-rules-edit', { id: '4' })
    expect(w.text()).toContain('admin-rule-editor-assets-return-hint')
    button('admin-rule-editor-save')!.click()
    await flushPromises()
    expect(rules.update).toHaveBeenCalledWith(
      4,
      expect.objectContaining({ actions: [{ kind: 'return_requester_assets', config: {} }] }),
    )
  })
})

const PREVIEW: RulePreview = { matched: 12, scanned: 200, sample_ticket_ids: [41, 42] }

const trigger = (w: VueWrapper) =>
  w.findAllComponents(BaseDropdown).find((d) => d.props('label') === 'admin-rule-editor-trigger-label')!

describe('RuleEditView preview', () => {
  const eventRule = () =>
    rule({
      trigger_kind: 'ticket_created',
      conditions: { kind: 'and', children: [{ kind: 'leaf', field: 'ticket.priority', op: 'eq', value: 'high' }] },
    })
  const previewText = () => document.body.querySelector('[data-test="rule-preview"]')?.textContent ?? ''

  it('counts the matches and links the samples', async () => {
    rules.get.mockResolvedValue(eventRule())
    rules.preview.mockResolvedValue(PREVIEW)
    const w = await mountAt('admin-rules-edit', { id: '4' })
    expect(rules.preview).toHaveBeenCalledWith({
      trigger_kind: 'ticket_created',
      conditions: { kind: 'and', children: [{ kind: 'leaf', field: 'ticket.priority', op: 'eq', value: 'high' }] },
    })
    expect(previewText()).toContain('admin-rule-preview-count')
    // The samples read as ticket numbers.
    expect(previewText()).toContain('#1041')
    expect(previewText()).toContain('#1042')
    expect(w.text()).not.toContain('admin-rule-preview-broad')
  })

  it('warns when nothing recent matches', async () => {
    rules.get.mockResolvedValue(eventRule())
    rules.preview.mockResolvedValue({ matched: 0, scanned: 200, sample_ticket_ids: [] })
    await mountAt('admin-rules-edit', { id: '4' })
    expect(previewText()).toContain('admin-rule-preview-none')
  })

  it('warns when more than half of recent tickets match', async () => {
    rules.get.mockResolvedValue(eventRule())
    rules.preview.mockResolvedValue({ matched: 101, scanned: 200, sample_ticket_ids: [1, 2, 3, 4, 5] })
    await mountAt('admin-rules-edit', { id: '4' })
    expect(previewText()).toContain('admin-rule-preview-broad')
  })

  it('reads exactly half as a plain count', async () => {
    rules.get.mockResolvedValue(eventRule())
    rules.preview.mockResolvedValue({ matched: 100, scanned: 200, sample_ticket_ids: [1] })
    await mountAt('admin-rules-edit', { id: '4' })
    expect(previewText()).toContain('admin-rule-preview-count')
  })

  it('says when there are no tickets to check yet', async () => {
    rules.get.mockResolvedValue(eventRule())
    rules.preview.mockResolvedValue({ matched: 0, scanned: 0, sample_ticket_ids: [] })
    await mountAt('admin-rules-edit', { id: '4' })
    expect(previewText()).toContain('admin-rule-preview-no-tickets')
  })

  it("shows the server's reason when it refuses the conditions", async () => {
    rules.get.mockResolvedValue(eventRule())
    rules.preview.mockRejectedValue({
      response: {
        status: 400,
        data: { code: 'RULE_CONDITION_INVALID', message: 'conditions.children[0]: give some text' },
      },
    })
    await mountAt('admin-rules-edit', { id: '4' })
    expect(previewText()).toContain('conditions.children[0]: give some text')
  })

  it('waits for unfinished conditions, then checks again after a pause in typing', async () => {
    rules.get.mockResolvedValue(eventRule())
    rules.preview.mockResolvedValue(PREVIEW)
    await mountAt('admin-rules-edit', { id: '4' })
    expect(rules.preview).toHaveBeenCalledTimes(1)

    Array.from(document.body.querySelectorAll<HTMLButtonElement>('button'))
      .find((b) => b.textContent?.includes('admin-rule-conditions-add') && !b.textContent.includes('group'))!
      .click()
    await flushPromises()
    // The new condition has no value yet.
    expect(previewText()).toContain('admin-rule-preview-incomplete')
    await new Promise((resolve) => setTimeout(resolve, 450))
    await flushPromises()
    expect(rules.preview).toHaveBeenCalledTimes(1)

    button('admin-rule-editor-save')!.click()
    await flushPromises()
    expect(rules.update).not.toHaveBeenCalled()
    expect(document.body.textContent).toContain('admin-rule-editor-conditions-incomplete')
    expect(document.body.textContent).toContain('admin-rule-conditions-problem-value')
  })

  it('does not preview a manual rule', async () => {
    rules.get.mockResolvedValue(rule())
    await mountAt('admin-rules-edit', { id: '4' })
    expect(rules.preview).not.toHaveBeenCalled()
    expect(document.body.querySelector('[data-test="rule-preview"]')).toBeNull()
  })
})

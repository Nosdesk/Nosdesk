import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import type { App } from 'vue'
import { mountWithProviders } from '@/test/mountWithProviders'
import { vSafeHtml } from '@/directives/vSafeHtml'
import type { Rule } from '@nosdesk/core/types/rule'

const apply = vi.hoisted(() => vi.fn())
const toastSuccess = vi.hoisted(() => vi.fn())
const onClose = vi.hoisted(() => vi.fn())
vi.mock('@nosdesk/core/services/rulesService', () => ({ default: { apply } }))
vi.mock('@/services/userService', () => ({
  default: { getUsersBatch: async () => [{ uuid: 'u-priya', name: 'Priya Shah' }] },
}))
vi.mock('@nosdesk/core/stores/workflowStates', () => ({
  useWorkflowStatesStore: () => ({
    load: async () => [],
    findById: (id: number) => (id === 7 ? { id: 7, name: 'Resolved' } : undefined),
  }),
}))
vi.mock('@nosdesk/core/stores/tags', () => ({
  useTagsStore: () => ({ findById: (id: number) => (id === 3 ? { id: 3, name: 'printer' } : null) }),
}))
vi.mock('@nosdesk/core/services/groupService', () => ({
  groupService: { getGroups: async () => [{ id: 3, name: 'Network' }] },
}))
vi.mock('@nosdesk/core/stores/toast', () => ({
  useToastStore: () => ({ success: toastSuccess, error: vi.fn() }),
}))

import ActionsDialog from '@/components/ticketComponents/ActionsDialog.vue'

const safeHtml = { install: (app: App) => app.directive('safe-html', vSafeHtml) }

function rule(id: number, name: string, fireCount: number, actions: Rule['actions']): Rule {
  return {
    id,
    workspace_id: 1,
    name,
    description: null,
    trigger_kind: 'manual',
    trigger_config: {},
    conditions: [],
    actions,
    reads_set: [],
    writes_set: [],
    state: 'live',
    priority: 100,
    last_fired_at: null,
    fire_count: fireCount,
    created_by: null,
    created_at: '2026-10-01T00:00:00Z',
    updated_at: '2026-10-01T00:00:00Z',
    archived_at: null,
  }
}

const RESOLVE = rule(1, 'Resolve with summary', 9, [
  { kind: 'reply', config: { visibility: 'public', body: 'Hi {{customer_name}},\n\nAll sorted.' } },
  { kind: 'set_status', config: { workflow_state_id: 7 } },
  { kind: 'stop_processing' },
  { kind: 'assign', config: { method: 'direct', user_uuid: 'u-priya' } },
  { kind: 'add_tags', config: { tag_ids: [3] } },
])
const BUMP = rule(2, 'Bump priority', 2, [{ kind: 'set_priority', config: { priority: 'high' } }])

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
  vi.clearAllMocks()
})

async function open(rules: Rule[], vars = { customer_name: 'Jane Doe', ticket_id: 42 }) {
  wrapper = mountWithProviders(
    ActionsDialog,
    { show: true, ticketId: 5, rules, vars, onClose },
    {},
    [[PiniaColada, {}] as never, safeHtml],
  )
  await flushPromises()
  return wrapper
}

const body = () => document.body
const stepLabels = () =>
  Array.from(body().querySelectorAll('section li')).map((li) => li.querySelector('label')?.textContent?.trim())

describe('ActionsDialog', () => {
  it('selects the most used rule and describes its steps by name', async () => {
    await open([BUMP, RESOLVE])
    const radios = Array.from(body().querySelectorAll<HTMLInputElement>('input[type="radio"]'))
    expect(radios.map((r) => r.closest('label')?.textContent?.trim())).toEqual([
      'Resolve with summary',
      'Bump priority',
    ])
    expect(radios[0].checked).toBe(true)
    // stop_processing does nothing on a manual apply, so it isn't listed.
    expect(stepLabels()).toEqual([
      'ticket-actions-step-reply',
      'ticket-actions-step-status',
      'ticket-actions-step-assign',
      'ticket-actions-step-add-tags',
    ])
  })

  it('previews a plain reply with its line breaks and this ticket’s values', async () => {
    await open([RESOLVE])
    const preview = body().querySelector('p.whitespace-pre-wrap')
    expect(preview?.textContent).toBe('Hi Jane Doe,\n\nAll sorted.')
  })

  it('escapes values in an HTML reply preview', async () => {
    const html = rule(3, 'Html', 1, [
      { kind: 'reply', config: { visibility: 'public', body: 'Hi <strong>{{customer_name}}</strong>' } },
    ])
    await open([html], { customer_name: '<img src=x onerror=alert(1)>', ticket_id: 1 })
    const preview = body().querySelector('.reply-preview')
    expect(preview?.querySelector('strong')?.textContent).toBe('<img src=x onerror=alert(1)>')
    expect(preview?.querySelector('img')).toBeNull()
  })

  it('sends skipped steps and the edited reply template', async () => {
    apply.mockResolvedValue({ rule: RESOLVE, application_id: 1, correlation_id: null })
    await open([RESOLVE])
    // Untick the status step (position 2 in the rule's action list).
    const checkboxes = Array.from(body().querySelectorAll<HTMLButtonElement>('section [role="checkbox"]'))
    checkboxes[1].click()
    await flushPromises()
    const edit = Array.from(body().querySelectorAll('button')).find((b) => b.textContent?.includes('ticket-actions-reply-edit'))
    edit?.click()
    await flushPromises()
    const textarea = body().querySelector<HTMLTextAreaElement>('textarea')
    expect(textarea?.value).toBe('Hi {{customer_name}},\n\nAll sorted.')
    textarea!.value = 'Hi {{customer_name}}, done.'
    textarea!.dispatchEvent(new Event('input'))
    await flushPromises()
    body().querySelector('form')!.dispatchEvent(new Event('submit'))
    await flushPromises()
    expect(apply).toHaveBeenCalledWith(1, {
      ticket_id: 5,
      overrides: { body: 'Hi {{customer_name}}, done.', suppress_actions: [2] },
    })
    expect(toastSuccess).toHaveBeenCalledWith('ticket-actions-success-toast')
    expect(onClose).toHaveBeenCalled()
  })

  it("leaves the reply alone when it wasn't edited and can't apply with nothing to run", async () => {
    apply.mockResolvedValue({ rule: BUMP, application_id: 2, correlation_id: null })
    await open([BUMP])
    const submit = () => Array.from(body().querySelectorAll<HTMLButtonElement>('button[type="submit"]'))[0]
    expect(submit().disabled).toBe(false)
    body().querySelector<HTMLButtonElement>('section [role="checkbox"]')!.click()
    await flushPromises()
    expect(submit().disabled).toBe(true)
    body().querySelector<HTMLButtonElement>('section [role="checkbox"]')!.click()
    await flushPromises()
    body().querySelector('form')!.dispatchEvent(new Event('submit'))
    await flushPromises()
    expect(apply).toHaveBeenCalledWith(2, { ticket_id: 5, overrides: { body: undefined, suppress_actions: [] } })
  })

  it('names the team a team step assigns to', async () => {
    const team = rule(4, 'Hand to network', 1, [{ kind: 'assign', config: { method: 'group', group_id: 3 } }])
    await open([team])
    expect(stepLabels()).toEqual(['ticket-actions-step-assign-team'])
  })
})

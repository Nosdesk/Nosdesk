import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { Rule, StarterRule } from '@nosdesk/core/types/rule'

const rules = vi.hoisted(() => ({ starterCatalog: vi.fn(), create: vi.fn() }))
const push = vi.hoisted(() => vi.fn())
const toastSuccess = vi.hoisted(() => vi.fn())
vi.mock('vue-router', () => ({ useRouter: () => ({ push }) }))
vi.mock('@nosdesk/core/services/rulesService', () => ({ default: rules }))
vi.mock('@nosdesk/core/stores/toast', () => ({
  useToastStore: () => ({ success: toastSuccess, error: vi.fn() }),
}))
vi.mock('@nosdesk/core/stores/dateStore', () => ({ useDateStore: () => ({ locale: 'fr-FR' }) }))
vi.mock('@nosdesk/core/stores/workflowStates', () => ({
  useWorkflowStatesStore: () => ({ load: async () => [], findById: () => undefined }),
}))
vi.mock('@nosdesk/core/stores/tags', () => ({ useTagsStore: () => ({ findById: () => null }) }))

import StarterRulesDialog from '@/components/admin/rules/StarterRulesDialog.vue'

const STARTERS: StarterRule[] = [
  {
    id: 'set-high-priority',
    name: 'Bump to high priority',
    description: 'For anything blocking work.',
    trigger_kind: 'manual',
    conditions: [],
    actions: [{ kind: 'set_priority', config: { priority: 'high' } }],
  },
  {
    id: 'request-more-info',
    name: 'Request more information',
    description: 'Ask for the details needed to reproduce.',
    trigger_kind: 'manual',
    conditions: [],
    actions: [{ kind: 'reply', config: { visibility: 'public', body: 'Hi {{customer_name}}, could you share more?' } }],
  },
]

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
  vi.clearAllMocks()
})

async function open(existing: Partial<Rule>[] = []) {
  rules.starterCatalog.mockResolvedValue(STARTERS)
  wrapper = mountWithProviders(
    StarterRulesDialog,
    { show: true, rules: existing, onClose: () => {} },
    {},
    [[PiniaColada, {}] as never],
  )
  await flushPromises()
  return wrapper
}

const items = () => Array.from(document.body.querySelectorAll('ul > li.rounded-lg'))

describe('StarterRulesDialog', () => {
  it("lists the starters in the app's language with their steps", async () => {
    await open()
    expect(rules.starterCatalog).toHaveBeenCalledWith('fr-FR')
    const first = items()[0]
    expect(first.textContent).toContain('Bump to high priority')
    expect(first.textContent).toContain('ticket-actions-step-priority')
    expect(items()[1].textContent).toContain('ticket-actions-step-reply')
  })

  it('marks a starter whose name is taken as added', async () => {
    await open([{ id: 9, name: 'request more information ' }])
    expect(items()[1].textContent).toContain('admin-rules-starters-added')
    expect(items()[1].querySelector('button')).toBeNull()
    expect(items()[0].querySelector('button')?.textContent).toContain('admin-rules-starters-add')
  })

  it('adds a starter as a manual draft and opens it', async () => {
    rules.create.mockResolvedValue({ id: 12, name: 'Bump to high priority' })
    await open()
    items()[0].querySelector('button')!.click()
    await flushPromises()
    expect(rules.create).toHaveBeenCalledWith({
      name: 'Bump to high priority',
      description: 'For anything blocking work.',
      trigger_kind: 'manual',
      conditions: [],
      actions: STARTERS[0].actions,
    })
    expect(toastSuccess).toHaveBeenCalledWith('admin-rules-starters-toast')
    expect(push).toHaveBeenCalledWith({ name: 'admin-rules-edit', params: { id: 12 } })
  })
})

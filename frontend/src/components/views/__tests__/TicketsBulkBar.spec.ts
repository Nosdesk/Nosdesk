import { afterEach, describe, expect, it, vi } from 'vitest'
import { computed, nextTick } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

const STATES = [
  { id: 1, name: 'Open', color: 'blue', category: 'active' },
  { id: 2, name: 'Done', color: 'green', category: 'done' },
]
const TICKETS: Record<number, { id: number; title: string; workflow_state_id: number; priority: string }> = {
  10: { id: 10, title: 'A', workflow_state_id: 1, priority: 'high' },
  11: { id: 11, title: 'B', workflow_state_id: 1, priority: 'low' },
}

vi.mock('@nosdesk/core/stores/workflowStates', () => ({
  useWorkflowStatesStore: () => ({
    byCategory: { active: [STATES[0]], done: [STATES[1]] },
    findById: (id: number) => STATES.find((s) => s.id === id),
  }),
}))
vi.mock('@/sync/stores/tickets', () => ({
  useSyncTicketsStore: () => ({ byId: (id: number) => computed(() => TICKETS[id] ?? null) }),
}))
vi.mock('@/plugins/loader', () => ({ getSlotRegistrations: () => [] }))
vi.mock('@/plugins/usePluginModal', () => ({ openPluginModal: () => {} }))
vi.mock('@/components/UserSelectionModal.vue', () => ({ default: { template: '<div />' } }))
vi.mock('@/components/ticketComponents/MergeTicketsDialog.vue', () => ({ default: { template: '<div />' } }))

import TicketsBulkBar from '@/components/views/TicketsBulkBar.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

const settle = () => new Promise((r) => setTimeout(r, 0))
async function key(el: Element, key: string) {
  el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
  await nextTick()
}
const options = () => Array.from(document.body.querySelectorAll<HTMLElement>('[role="option"]'))

describe('TicketsBulkBar', () => {
  it('is a toolbar with the count, select all, clear and the ticket actions', async () => {
    const events: string[] = []
    wrapper = mountWithProviders(TicketsBulkBar, {
      selectedIds: ['10', '11'],
      totalCount: 5,
      onClear: () => events.push('clear'),
      onSelectAll: () => events.push('select-all'),
    })
    await nextTick()
    const bar = wrapper.get('[role="toolbar"]')
    expect(bar.attributes('aria-label')).toBe('common-bulk-actions-aria')
    expect(bar.text()).toContain('bulk-bar-tickets-selected')
    const names = bar.findAll('button').map((b) => b.text())
    // The status button carries the shared state's glyph (named "Open").
    expect(names).toEqual([
      'bulk-bar-select-all-matching',
      'bulk-bar-clear',
      'Openticket-list-bulk-status',
      'ticket-list-bulk-priority',
      'ticket-list-bulk-assign',
      'ticket-list-bulk-merge',
    ])
    await bar.findAll('button')[0].trigger('click')
    await bar.findAll('button')[1].trigger('click')
    expect(events).toEqual(['select-all', 'clear'])
  })

  it('hides merge for a single ticket', async () => {
    wrapper = mountWithProviders(TicketsBulkBar, { selectedIds: ['10'] })
    await nextTick()
    expect(wrapper.text()).not.toContain('ticket-list-bulk-merge')
  })

  it('opens status as a grouped listbox focused on the shared state, and picks with Enter', async () => {
    const picks: unknown[] = []
    wrapper = mountWithProviders(TicketsBulkBar, {
      selectedIds: ['10', '11'],
      onSetStatus: (id: number, ids: number[]) => picks.push([id, ids]),
    })
    await nextTick()
    const trigger = wrapper.get('button[aria-haspopup="dialog"]')
    expect(trigger.attributes('aria-expanded')).toBe('false')
    await trigger.trigger('click')
    await nextTick()
    await nextTick()
    await nextTick()
    expect(trigger.attributes('aria-expanded')).toBe('true')
    const groups = Array.from(document.body.querySelectorAll('[role="listbox"] [role="group"]'))
    expect(groups.map((g) => document.getElementById(g.getAttribute('aria-labelledby')!)?.textContent?.trim())).toEqual([
      'Active',
      'Done',
    ])
    expect(options().map((o) => o.textContent?.replace(/\s+/g, ' ').trim())).toEqual(['OpenOpen', 'DoneDone'])
    // Both tickets are Open, so it is selected and focused.
    expect(options()[0].getAttribute('aria-selected')).toBe('true')
    expect(document.activeElement).toBe(options()[0])
    await key(options()[0], 'ArrowDown')
    expect(document.activeElement).toBe(options()[1])
    await key(options()[1], 'Enter')
    expect(picks).toEqual([[2, [10, 11]]])
    await settle()
    expect(document.body.querySelector('[role="listbox"]')).toBeNull()
  })

  it('opens priority with nothing selected when the tickets differ, and picks with a click', async () => {
    const picks: unknown[] = []
    wrapper = mountWithProviders(TicketsBulkBar, {
      selectedIds: ['10', '11'],
      onSetPriority: (p: string, ids: number[]) => picks.push([p, ids]),
    })
    await nextTick()
    await wrapper.findAll('button[aria-haspopup="dialog"]')[1].trigger('click')
    await nextTick()
    await nextTick()
    await nextTick()
    expect(options()).toHaveLength(3)
    expect(options().every((o) => o.getAttribute('aria-selected') === 'false')).toBe(true)
    expect(document.activeElement).toBe(options()[0])
    options()[2].click()
    await nextTick()
    expect(picks).toEqual([['high', [10, 11]]])
  })
})

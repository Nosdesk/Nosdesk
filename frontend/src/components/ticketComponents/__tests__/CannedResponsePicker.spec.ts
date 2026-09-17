import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

const RESPONSES = [
  { id: 1, title: 'Greeting', body: 'Hi {{customer_name}}, thanks for writing in.', workspace_id: 1, inserts_30d: 0 },
  { id: 2, title: 'Password reset', body: 'Use the reset link, {{tech_name}} has sent it.', workspace_id: 1, inserts_30d: 0 },
  { id: 3, title: 'Closing', body: 'Glad that is sorted.', workspace_id: 1, inserts_30d: 0 },
]
const recorded: number[] = []
vi.mock('@nosdesk/core/services/cannedResponsesService', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@nosdesk/core/services/cannedResponsesService')>()
  return {
    ...actual,
    cannedResponsesService: {
      list: async () => RESPONSES,
      recordInsertion: async (id: number) => {
        recorded.push(id)
      },
    },
  }
})

import CannedResponsePicker from '@/components/ticketComponents/CannedResponsePicker.vue'

let wrapper: VueWrapper | null = null
// Desktop: the anchored popover.
beforeEach(() => {
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches: query.includes('min-width'),
    media: query,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    onchange: null,
    dispatchEvent: () => false,
  }))
  recorded.length = 0
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

const settle = (ms = 5) => new Promise((r) => setTimeout(r, ms))
const options = () => Array.from(document.body.querySelectorAll<HTMLElement>('[role="option"]'))
async function key(el: Element, key: string) {
  el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
  await nextTick()
}

async function openPicker(inserts: string[] = []) {
  wrapper = mountWithProviders(
    CannedResponsePicker,
    { vars: { customer_name: 'Ada' }, onInsert: (t: string) => inserts.push(t) },
    {},
    [[PiniaColada, {}] as never],
  )
  await flushPromises()
  await wrapper.get('button[aria-haspopup="dialog"]').trigger('click')
  await flushPromises()
  await settle()
  await nextTick()
  return document.body.querySelector<HTMLInputElement>('[role="dialog"] input')!
}

describe('CannedResponsePicker', () => {
  it('opens a dialog with a focused filter over a listbox of the templates', async () => {
    const input = await openPicker()
    expect(document.body.querySelector('[role="dialog"]')?.getAttribute('aria-label')).toBe('ticket-picker-canned-trigger-aria')
    expect(document.activeElement).toBe(input)
    expect(options().map((o) => o.querySelector('span')?.textContent)).toEqual(['Greeting', 'Password reset', 'Closing'])
    // The first row is highlighted; the filter names it.
    expect(options()[0].hasAttribute('data-highlighted')).toBe(true)
    expect(input.getAttribute('aria-activedescendant')).toBe(options()[0].id)
    // The preview shows the substituted body.
    expect(options()[0].textContent).toContain('Hi Ada, thanks')
  })

  it('filters, warns about unbound variables on the highlighted row, and inserts on Enter', async () => {
    const inserts: string[] = []
    const input = await openPicker(inserts)
    input.value = 'reset'
    input.dispatchEvent(new Event('input', { bubbles: true }))
    await nextTick()
    await nextTick()
    expect(options()).toHaveLength(1)
    expect(options()[0].hasAttribute('data-highlighted')).toBe(true)
    expect(document.body.querySelector('[role="status"]')?.textContent).toContain('ticket-picker-canned-missing-vars')
    await key(input, 'Enter')
    expect(inserts).toEqual(['Use the reset link,  has sent it.'])
    expect(recorded).toEqual([2])
    await settle()
    expect(document.body.querySelector('[role="dialog"]')).toBeNull()
  })

  it('walks with the arrows and inserts on click', async () => {
    const inserts: string[] = []
    const input = await openPicker(inserts)
    await key(input, 'ArrowDown')
    await key(input, 'ArrowDown')
    expect(options()[2].hasAttribute('data-highlighted')).toBe(true)
    expect(input.getAttribute('aria-activedescendant')).toBe(options()[2].id)
    options()[2].click()
    await nextTick()
    expect(inserts).toEqual(['Glad that is sorted.'])
  })
})

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import CustomDropdown from '@/components/ticketComponents/CustomDropdown.vue'

let wrapper: VueWrapper | null = null

// Desktop: the anchored popover. The phone path is the bottom sheet,
// covered by the BottomSheet spec.
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
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

const OPTIONS = [
  { value: 'low', label: 'Low' },
  { value: 'medium', label: 'Medium' },
  { value: 'high', label: 'High' },
]

async function open() {
  await wrapper!.get('button[aria-haspopup="dialog"]').trigger('click')
  await nextTick()
  await nextTick()
  await nextTick()
}

const options = () => Array.from(document.body.querySelectorAll<HTMLElement>('[role="option"]'))

const settle = () => new Promise((r) => setTimeout(r, 0))

async function key(el: Element, key: string) {
  el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
  await nextTick()
}

describe('CustomDropdown', () => {
  it('opens a named dialog holding a listbox of options with the value selected', async () => {
    wrapper = mountWithProviders(CustomDropdown, { value: 'medium', options: OPTIONS, type: 'priority' })
    await nextTick()
    const trigger = wrapper.get('button[aria-haspopup="dialog"]')
    expect(trigger.attributes('aria-expanded')).toBe('false')
    await open()
    expect(trigger.attributes('aria-expanded')).toBe('true')
    const dialog = document.body.querySelector('[role="dialog"]')
    expect(dialog?.getAttribute('aria-label')).toBe('ticket-chip-dropdown-priority')
    const listbox = document.body.querySelector('[role="listbox"]')
    expect(listbox?.getAttribute('aria-label')).toBe('ticket-chip-dropdown-priority')
    expect(options().map((o) => o.textContent?.trim())).toEqual(['Low', 'Medium', 'High'])
    expect(options().map((o) => o.getAttribute('aria-selected'))).toEqual(['false', 'true', 'false'])
    // No stray buttons outside the option role.
    expect(dialog?.querySelectorAll('button:not([role="option"])')).toHaveLength(0)
    // Focus opens on the current option.
    expect(document.activeElement).toBe(options()[1])
    expect(options()[1].hasAttribute('data-highlighted')).toBe(true)
  })

  it('turns disabled entries into labelled groups of the options after them', async () => {
    wrapper = mountWithProviders(CustomDropdown, {
      value: 'open',
      options: [
        { value: 'new', label: 'New' },
        { value: '__active', label: 'Active', disabled: true },
        { value: 'open', label: 'Open' },
        { value: '__done', label: 'Done', disabled: true },
        { value: 'closed', label: 'Closed' },
      ],
      type: 'status',
    })
    await nextTick()
    await open()
    expect(options().map((o) => o.textContent?.trim())).toEqual(['New', 'Open', 'Closed'])
    const groups = Array.from(document.body.querySelectorAll('[role="listbox"] [role="group"]'))
    const label = (g: Element) => document.getElementById(g.getAttribute('aria-labelledby')!)?.textContent?.trim()
    expect(groups.map(label)).toEqual([undefined, 'Active', 'Done'])
    expect(groups.map((g) => g.querySelectorAll('[role="option"]').length)).toEqual([1, 1, 1])
  })

  it('picks with the keyboard and with a click, then closes', async () => {
    const updates: string[] = []
    wrapper = mountWithProviders(CustomDropdown, {
      value: 'medium',
      options: OPTIONS,
      type: 'priority',
      'onUpdate:value': (v: string) => updates.push(v),
    })
    await nextTick()
    await open()
    await key(document.activeElement!, 'End')
    expect(document.activeElement).toBe(options()[2])
    await key(document.activeElement!, 'Enter')
    expect(updates).toEqual(['high'])
    await nextTick()
    expect(document.body.querySelector('[role="listbox"]')).toBeNull()

    await open()
    options()[0].click()
    await nextTick()
    expect(updates).toEqual(['high', 'low'])
    await settle()
    expect(document.body.querySelector('[role="listbox"]')).toBeNull()

    // Picking the current value again closes rather than deselects.
    await open()
    options()[1].click()
    await nextTick()
    expect(updates).toEqual(['high', 'low', 'medium'])
    await settle()
    expect(document.body.querySelector('[role="listbox"]')).toBeNull()
  })
})

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import SearchableDropdown from '@/components/common/SearchableDropdown.vue'
import Autocomplete from '@/components/common/Autocomplete.vue'

const settle = (ms = 30) => new Promise((r) => setTimeout(r, ms))

let wrapper: VueWrapper | null = null

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
  Element.prototype.scrollIntoView = () => {}
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

const key = (el: Element | null, k: string) =>
  el?.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true }))

async function type(input: HTMLInputElement, text: string) {
  input.value = text
  input.dispatchEvent(new Event('input', { bubbles: true }))
  await settle()
}

describe('SearchableDropdown', () => {
  const options = [
    { value: 'Australia/Sydney', label: 'Sydney', description: 'UTC+10' },
    { value: 'Europe/Paris', label: 'Paris', description: 'UTC+1' },
    { value: 'America/New_York', label: 'New York', description: 'UTC-5' },
  ]

  it('opens a filter over a listbox, filters locale-aware, and picks with the keyboard', async () => {
    const updates: string[] = []
    wrapper = mountWithProviders(SearchableDropdown, {
      modelValue: 'Europe/Paris',
      options,
      label: 'Timezone',
      'onUpdate:modelValue': (v: string) => updates.push(v),
    })
    await settle()
    await wrapper.get('button').trigger('click')
    await settle()
    const input = document.body.querySelector<HTMLInputElement>('[role="dialog"] input')
    expect(input).not.toBeNull()
    expect(document.activeElement).toBe(input)
    expect(document.body.querySelectorAll('[role="option"]')).toHaveLength(3)
    const listbox = document.body.querySelector<HTMLElement>('[role="listbox"]')
    expect(listbox?.getAttribute('aria-label')).toBe('Timezone')
    expect(listbox?.getAttribute('tabindex')).toBe('0')

    // Accent and case insensitive, across label and description.
    await type(input!, 'sÝd')
    const rows = Array.from(document.body.querySelectorAll<HTMLElement>('[role="option"]'))
    expect(rows.map((r) => r.textContent?.trim())).toEqual(['SydneyUTC+10'])

    // Enter picks the highlighted row (the filter highlighted the first
    // match) and the surface closes.
    key(input, 'Enter')
    await settle()
    expect(updates).toEqual(['Australia/Sydney'])
    expect(document.body.querySelector('[role="dialog"]')).toBeNull()
  })

  it('shows the empty message when nothing matches', async () => {
    wrapper = mountWithProviders(SearchableDropdown, {
      modelValue: '',
      options,
      emptyMessage: 'Nothing here',
    })
    await settle()
    await wrapper.get('button').trigger('click')
    await settle()
    const input = document.body.querySelector<HTMLInputElement>('[role="dialog"] input')!
    await type(input, 'zzz')
    expect(document.body.querySelectorAll('[role="option"]')).toHaveLength(0)
    expect(document.body.querySelector('[role="dialog"]')?.textContent).toContain('Nothing here')
  })
})

describe('Autocomplete', () => {
  it('is a combobox whose text is the value, with filtered suggestions', async () => {
    const updates: string[] = []
    wrapper = mountWithProviders(Autocomplete, {
      modelValue: '',
      options: ['Dell', 'Lenovo', 'Apple'],
      label: 'Vendor',
      'onUpdate:modelValue': (v: string) => updates.push(v),
    })
    await settle()
    const input = wrapper.get('input').element as HTMLInputElement
    expect(input.getAttribute('role')).toBe('combobox')
    input.focus()
    input.dispatchEvent(new Event('focus'))
    await settle()
    await type(input, 'le')
    expect(updates).toEqual(['le'])
    const rows = Array.from(document.body.querySelectorAll<HTMLElement>('[role="option"]'))
    expect(rows.map((r) => r.textContent?.trim())).toEqual(['Lenovo', 'Apple'])
    // Typing highlights the first match; Enter takes it, ArrowDown walks.
    key(input, 'ArrowDown')
    key(input, 'Enter')
    await settle()
    expect(updates[updates.length - 1]).toBe('Apple')
  })
})

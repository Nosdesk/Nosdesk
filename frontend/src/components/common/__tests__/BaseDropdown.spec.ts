import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import BaseDropdown from '@/components/common/BaseDropdown.vue'

const settle = () => new Promise((r) => setTimeout(r, 20))

let wrapper: VueWrapper | null = null
let mobile = false

beforeEach(() => {
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches: query.includes('min-width') ? !mobile : mobile,
    media: query,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    onchange: null,
    dispatchEvent: () => false,
  }))
  // jsdom lacks the pointer-capture and scroll APIs Reka's Select uses.
  Element.prototype.scrollIntoView = () => {}
  Element.prototype.hasPointerCapture = () => false
  Element.prototype.setPointerCapture = () => {}
  Element.prototype.releasePointerCapture = () => {}
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

const OPTIONS = [
  { value: 'open', label: 'Open', tones: ['bg-status-open'] },
  { value: 'closed', label: 'Closed' },
  { value: 'merged', label: 'Merged', disabled: true },
]

// Reka ignores the first pointerup after opening unless the pointer moved
// more than 10px since the trigger press (so click-hold-release on the
// trigger selects nothing); `moveAway` supplies that movement.
function pointer(type: string, target: Element, at = 0) {
  const ev = new Event(type, { bubbles: true, cancelable: true }) as PointerEvent
  Object.defineProperty(ev, 'pointerType', { value: 'mouse' })
  Object.defineProperty(ev, 'button', { value: 0 })
  Object.defineProperty(ev, 'ctrlKey', { value: false })
  Object.defineProperty(ev, 'clientX', { value: at })
  Object.defineProperty(ev, 'clientY', { value: at })
  Object.defineProperty(ev, 'pageX', { value: at })
  Object.defineProperty(ev, 'pageY', { value: at })
  target.dispatchEvent(ev)
}

function moveAway() {
  pointer('pointermove', document.body, 300)
}

function mountDropdown(props: Record<string, unknown> = {}) {
  const updates: unknown[] = []
  wrapper = mountWithProviders(BaseDropdown, {
    modelValue: 'open',
    options: OPTIONS,
    label: 'Status',
    'onUpdate:modelValue': (v: unknown) => updates.push(v),
    ...props,
  })
  return { updates }
}

describe('BaseDropdown on desktop', () => {
  beforeEach(() => {
    mobile = false
  })

  it('renders a combobox trigger named by its label, showing the selection', async () => {
    mountDropdown()
    await settle()
    const trigger = wrapper!.get('[role="combobox"]')
    const label = wrapper!.get('label')
    expect(label.attributes('for')).toBe(trigger.attributes('id'))
    expect(trigger.text()).toContain('Open')
    expect(trigger.attributes('aria-expanded')).toBe('false')
  })

  it('opens a listbox with option rows and selects with the pointer', async () => {
    const { updates } = mountDropdown()
    await settle()
    const trigger = wrapper!.get('[role="combobox"]').element
    pointer('pointerdown', trigger)
    await settle()
    const listbox = document.body.querySelector('[role="listbox"]')
    expect(listbox).not.toBeNull()
    const options = Array.from(document.body.querySelectorAll<HTMLElement>('[role="option"]'))
    expect(options.map((o) => o.textContent?.trim())).toEqual(['Open', 'Closed', 'Merged'])
    expect(options[0].getAttribute('aria-selected')).toBe('true')
    expect(options[2].getAttribute('data-disabled')).not.toBeNull()
    moveAway()
    pointer('pointerup', options[1], 300)
    await settle()
    expect(updates).toEqual(['closed'])
  })

  it('keeps number values as numbers', async () => {
    const { updates } = mountDropdown({
      modelValue: 2,
      options: [
        { value: 1, label: 'One' },
        { value: 2, label: 'Two' },
      ],
    })
    await settle()
    pointer('pointerdown', wrapper!.get('[role="combobox"]').element)
    await settle()
    const options = Array.from(document.body.querySelectorAll<HTMLElement>('[role="option"]'))
    moveAway()
    pointer('pointerup', options[0], 300)
    await settle()
    expect(updates).toEqual([1])
  })

  it('supports an empty-string "none" option, which Reka alone refuses', async () => {
    const { updates } = mountDropdown({
      modelValue: '',
      options: [
        { value: '', label: 'None' },
        { value: 'a', label: 'A' },
      ],
    })
    await settle()
    expect(wrapper!.get('[role="combobox"]').text()).toContain('None')
    pointer('pointerdown', wrapper!.get('[role="combobox"]').element)
    await settle()
    const options = Array.from(document.body.querySelectorAll<HTMLElement>('[role="option"]'))
    expect(options.map((o) => o.textContent?.trim())).toEqual(['None', 'A'])
    expect(options[0].getAttribute('aria-selected')).toBe('true')
    moveAway()
    pointer('pointerup', options[1], 300)
    await settle()
    expect(updates).toEqual(['a'])

    await wrapper!.setProps({ modelValue: 'a' })
    pointer('pointerdown', wrapper!.get('[role="combobox"]').element)
    await settle()
    const reopened = Array.from(document.body.querySelectorAll<HTMLElement>('[role="option"]'))
    moveAway()
    pointer('pointerup', reopened[0], 300)
    await settle()
    expect(updates).toEqual(['a', ''])
  })

  it('translates the all meta option for multi-select', async () => {
    const { updates } = mountDropdown({
      multiple: true,
      modelValue: [],
      options: [{ value: 'all', label: 'All' }, ...OPTIONS.slice(0, 2)],
    })
    await settle()
    pointer('pointerdown', wrapper!.get('[role="combobox"]').element)
    await settle()
    const options = Array.from(document.body.querySelectorAll<HTMLElement>('[role="option"]'))
    moveAway()
    pointer('pointerup', options[0], 300)
    await settle()
    expect(updates).toEqual([['open', 'closed']])
  })
})

describe('BaseDropdown on a phone', () => {
  beforeEach(() => {
    mobile = true
  })

  it('opens a sheet listbox and selects from it', async () => {
    const { updates } = mountDropdown()
    await settle()
    expect(wrapper!.get('button').attributes('aria-haspopup')).toBe('dialog')
    await wrapper!.get('button').trigger('click')
    await settle()
    const dialog = document.body.querySelector<HTMLElement>('[role="dialog"]')
    expect(dialog?.querySelector('[role="listbox"]')).not.toBeNull()
    const options = Array.from(dialog!.querySelectorAll<HTMLButtonElement>('[role="option"]'))
    expect(options.map((o) => o.getAttribute('aria-selected'))).toEqual(['true', 'false', 'false'])
    options[1].click()
    await nextTick()
    expect(updates).toEqual(['closed'])
  })
})

import { afterEach, describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import FormNumber from '@/components/common/FormNumber.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

// The steppers are press-and-hold controls listening to pointer events,
// which jsdom lacks; a plain Event with the fields Reka reads stands in.
function press(target: Element) {
  const down = new Event('pointerdown', { bubbles: true, cancelable: true })
  Object.defineProperty(down, 'button', { value: 0 })
  target.dispatchEvent(down)
  window.dispatchEvent(new Event('pointerup', { bubbles: true }))
}

async function typeAndBlur(input: HTMLInputElement, text: string) {
  input.value = text
  input.dispatchEvent(new Event('input', { bubbles: true }))
  input.dispatchEvent(new Event('blur', { bubbles: true }))
  await nextTick()
}

describe('FormNumber', () => {
  it('is a labelled spinbutton with named steppers and bounds', async () => {
    wrapper = mountWithProviders(FormNumber, { modelValue: 8080, label: 'Port', min: 1, max: 65535 })
    await nextTick()
    const input = wrapper.get('[role="spinbutton"]')
    expect(wrapper.get('label').attributes('for')).toBe(input.attributes('id'))
    expect(input.attributes('aria-valuenow')).toBe('8080')
    expect(input.attributes('aria-valuemin')).toBe('1')
    expect(input.attributes('aria-valuemax')).toBe('65535')
    expect(input.attributes('aria-roledescription')).toBeUndefined()
    expect(wrapper.get('[aria-label="form-number-decrement"]').exists()).toBe(true)
    expect(wrapper.get('[aria-label="form-number-increment"]').exists()).toBe(true)
  })

  it('renders large values without grouping and commits on blur, clamped', async () => {
    const updates: Array<number | null> = []
    wrapper = mountWithProviders(FormNumber, {
      modelValue: 60000,
      min: 1,
      max: 65535,
      integer: true,
      'onUpdate:modelValue': (v: number | null) => updates.push(v),
    })
    await nextTick()
    const input = wrapper.get('[role="spinbutton"]').element as HTMLInputElement
    expect(input.value).toBe('60000')
    expect(input.getAttribute('inputmode')).toBe('numeric')
    await typeAndBlur(input, '70000')
    expect(updates.at(-1)).toBe(65535)
    await typeAndBlur(input, '42')
    expect(updates.at(-1)).toBe(42)
  })

  it('reports an emptied field as null', async () => {
    const updates: Array<number | null> = []
    wrapper = mountWithProviders(FormNumber, {
      modelValue: 5,
      'onUpdate:modelValue': (v: number | null) => updates.push(v),
    })
    await nextTick()
    const input = wrapper.get('[role="spinbutton"]').element as HTMLInputElement
    await typeAndBlur(input, '')
    expect(updates.at(-1)).toBeNull()
  })

  it('refuses a decimal separator when integer', async () => {
    wrapper = mountWithProviders(FormNumber, { modelValue: 1, integer: true })
    await nextTick()
    const input = wrapper.get('[role="spinbutton"]').element as HTMLInputElement
    const ev = new InputEvent('beforeinput', { data: '.', inputType: 'insertText', cancelable: true, bubbles: true })
    input.dispatchEvent(ev)
    expect(ev.defaultPrevented).toBe(true)
    const ok = new InputEvent('beforeinput', { data: '7', inputType: 'insertText', cancelable: true, bubbles: true })
    input.dispatchEvent(ok)
    expect(ok.defaultPrevented).toBe(false)
  })

  it('steps with the buttons and the arrow keys within bounds', async () => {
    const updates: Array<number | null> = []
    wrapper = mountWithProviders(FormNumber, {
      modelValue: 3,
      min: 0,
      max: 4,
      'onUpdate:modelValue': (v: number | null) => updates.push(v),
    })
    await nextTick()
    press(wrapper.get('[aria-label="form-number-increment"]').element)
    await nextTick()
    expect(updates.at(-1)).toBe(4)
    const input = wrapper.get('[role="spinbutton"]').element as HTMLInputElement
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }))
    await nextTick()
    expect(updates.at(-1)).toBe(2)
  })

  it('disables the whole group', async () => {
    wrapper = mountWithProviders(FormNumber, { modelValue: 3, disabled: true })
    await nextTick()
    expect(wrapper.get('[role="spinbutton"]').attributes('disabled')).toBeDefined()
    expect(wrapper.get('[aria-label="form-number-increment"]').attributes('disabled')).toBeDefined()
  })
})

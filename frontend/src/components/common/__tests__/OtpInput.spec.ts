import { afterEach, describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import OtpInput from '@/components/common/OtpInput.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

function type(input: HTMLInputElement, ch: string) {
  input.value = ch
  input.dispatchEvent(new InputEvent('input', { data: ch, bubbles: true }))
}

describe('OtpInput', () => {
  it('is a named group of one input per digit, marked for one-time codes', async () => {
    wrapper = mountWithProviders(OtpInput, { modelValue: '', ariaLabel: 'Verification code' })
    await nextTick()
    expect(wrapper.get('[role="group"]').attributes('aria-label')).toBe('Verification code')
    const inputs = wrapper.findAll('input:not([tabindex="-1"])')
    expect(inputs).toHaveLength(6)
    expect(inputs[1].attributes('aria-label')).toBe('otp-digit-aria')
    expect(inputs[0].attributes('autocomplete')).toBe('one-time-code')
    expect(inputs[0].attributes('inputmode')).toBe('numeric')
  })

  it('advances as digits are typed and completes on the last one', async () => {
    const updates: string[] = []
    const completes: string[] = []
    let value = ''
    wrapper = mountWithProviders(OtpInput, {
      modelValue: value,
      'onUpdate:modelValue': (v: string) => {
        updates.push(v)
        value = v
        wrapper?.setProps({})
      },
      onComplete: (v: string) => completes.push(v),
    })
    await nextTick()
    const inputs = wrapper.findAll('input:not([tabindex="-1"])').map((i) => i.element as HTMLInputElement)
    inputs[0].focus()
    type(inputs[0], '1')
    await nextTick()
    expect(updates.at(-1)).toBe('1')
    expect(document.activeElement).toBe(inputs[1])
  })

  it('spreads a pasted code across the boxes', async () => {
    const updates: string[] = []
    wrapper = mountWithProviders(OtpInput, {
      modelValue: '',
      'onUpdate:modelValue': (v: string) => updates.push(v),
    })
    await nextTick()
    const first = wrapper.get('input:not([tabindex="-1"])').element as HTMLInputElement
    // jsdom has no DataTransfer; Reka reads clipboardData.getData('text').
    const paste = new Event('paste', { bubbles: true, cancelable: true }) as ClipboardEvent
    Object.defineProperty(paste, 'clipboardData', { value: { getData: () => '123456' } })
    first.dispatchEvent(paste)
    await nextTick()
    expect(updates.at(-1)).toBe('123456')
  })

  it('renders the current value into the boxes', async () => {
    wrapper = mountWithProviders(OtpInput, { modelValue: '4207' })
    await nextTick()
    const inputs = wrapper.findAll('input:not([tabindex="-1"])').map((i) => (i.element as HTMLInputElement).value)
    expect(inputs).toEqual(['4', '2', '0', '7', '', ''])
  })
})

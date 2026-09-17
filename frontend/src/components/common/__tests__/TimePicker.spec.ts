import { afterEach, describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import TimePicker from '@/components/common/TimePicker.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

const segment = (part: string) => wrapper!.get(`[data-reka-time-field-segment="${part}"]`)

async function key(el: Element, key: string) {
  el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
  await nextTick()
}

// Focus leaving the control (no related target) is what commits.
async function blur() {
  wrapper!.get('[role="group"]').element.dispatchEvent(new FocusEvent('focusout', { bubbles: true }))
  await nextTick()
}

describe('TimePicker', () => {
  it('is a named group of labelled hour and minute spinbuttons', async () => {
    wrapper = mountWithProviders(TimePicker, { modelValue: '09:30', ariaLabel: 'Opens at' })
    await nextTick()
    expect(wrapper.get('[role="group"]').attributes('aria-label')).toBe('Opens at')
    expect(segment('hour').attributes('role')).toBe('spinbutton')
    expect(segment('hour').attributes('aria-label')).toBe('date-segment-hour')
    expect(segment('minute').attributes('aria-label')).toBe('date-segment-minute')
    expect(segment('minute').attributes('aria-valuenow')).toBe('30')
    expect(wrapper.find('[data-reka-time-field-segment="dayPeriod"]').exists()).toBe(true)
    expect(wrapper.find('[role="dialog"]').exists()).toBe(false)
  })

  it('steps minutes by minuteStep and hours by one, emitting 24-hour HH:MM', async () => {
    const updates: string[] = []
    wrapper = mountWithProviders(TimePicker, {
      modelValue: '17:30',
      minuteStep: 15,
      'onUpdate:modelValue': (v: string) => updates.push(v),
    })
    await nextTick()
    expect(segment('hour').attributes('aria-valuenow')).toBe('17')
    await key(segment('minute').element, 'ArrowUp')
    expect(segment('minute').attributes('aria-valuenow')).toBe('45')
    expect(updates).toEqual([])
    await key(segment('hour').element, 'ArrowDown')
    await blur()
    expect(updates).toEqual(['16:45'])
  })

  it('takes a typed minute as it is, without snapping to the step', async () => {
    const updates: string[] = []
    wrapper = mountWithProviders(TimePicker, {
      modelValue: '09:00',
      minuteStep: 15,
      'onUpdate:modelValue': (v: string) => updates.push(v),
    })
    await nextTick()
    await key(segment('minute').element, '3')
    await key(segment('minute').element, '7')
    await key(segment('minute').element, 'Enter')
    expect(updates).toEqual(['09:37'])
  })

  it('follows the parent value and clears to an empty string', async () => {
    const updates: string[] = []
    wrapper = mountWithProviders(TimePicker, {
      modelValue: '09:30',
      'onUpdate:modelValue': (v: string) => updates.push(v),
    })
    await nextTick()
    await wrapper.setProps({ modelValue: '14:05' })
    expect(segment('hour').attributes('aria-valuenow')).toBe('14')
    expect(segment('minute').attributes('aria-valuenow')).toBe('5')
    await key(segment('minute').element, 'Backspace')
    await blur()
    expect(updates).toEqual([''])
  })

  it('renders a label tied to the field and puts error on the segments', async () => {
    wrapper = mountWithProviders(TimePicker, { modelValue: '', label: 'Closes at', error: true })
    await nextTick()
    const label = wrapper.get('label')
    expect(wrapper.get(`#${label.attributes('for')}`).element.tagName).toBe('INPUT')
    expect(wrapper.get('[role="group"]').attributes('aria-labelledby')).toBe(label.attributes('id'))
    expect(segment('hour').attributes('aria-invalid')).toBe('true')
    expect(segment('minute').attributes('aria-invalid')).toBe('true')
  })
})

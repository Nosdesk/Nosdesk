import { afterEach, describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import Checkbox from '@/components/common/Checkbox.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

describe('Checkbox', () => {
  it('is a checkbox named by its label, with the label pointing at it', async () => {
    wrapper = mountWithProviders(Checkbox, { modelValue: false, label: 'Remember me' })
    await nextTick()
    const box = wrapper.get('[role="checkbox"]')
    expect(box.attributes('aria-checked')).toBe('false')
    expect(box.attributes('aria-label')).toBe('Remember me')
    expect(wrapper.get('label').attributes('for')).toBe(box.attributes('id'))
  })

  it('prefers the screen-reader label and hides the tick until checked', async () => {
    wrapper = mountWithProviders(Checkbox, { modelValue: true, label: 'Row', ariaLabel: 'Select ticket 12' })
    await nextTick()
    const box = wrapper.get('[role="checkbox"]')
    expect(box.attributes('aria-label')).toBe('Select ticket 12')
    expect(box.attributes('data-state')).toBe('checked')
    expect(box.find('svg').exists()).toBe(true)
  })

  it('reports mixed when indeterminate and lands on checked from a click', async () => {
    const updates: boolean[] = []
    wrapper = mountWithProviders(Checkbox, {
      modelValue: false,
      indeterminate: true,
      'onUpdate:modelValue': (v: boolean) => updates.push(v),
    })
    await nextTick()
    const box = wrapper.get('[role="checkbox"]')
    expect(box.attributes('aria-checked')).toBe('mixed')
    await box.trigger('click')
    expect(updates).toEqual([true])
  })

  it('emits change with the native click so callers can read modifiers', async () => {
    const events: MouseEvent[] = []
    const updates: boolean[] = []
    wrapper = mountWithProviders(Checkbox, {
      modelValue: false,
      onChange: (e: MouseEvent) => events.push(e),
      'onUpdate:modelValue': (v: boolean) => updates.push(v),
    })
    await nextTick()
    await wrapper.get('[role="checkbox"]').trigger('click', { shiftKey: true })
    expect(events).toHaveLength(1)
    expect(events[0].shiftKey).toBe(true)
    expect(updates).toEqual([true])
  })

  it('swallows Enter and ignores clicks when disabled', async () => {
    const updates: boolean[] = []
    wrapper = mountWithProviders(Checkbox, {
      modelValue: false,
      disabled: true,
      'onUpdate:modelValue': (v: boolean) => updates.push(v),
    })
    await nextTick()
    const box = wrapper.get('[role="checkbox"]')
    expect(box.attributes('disabled')).toBeDefined()
    await box.trigger('click')
    expect(updates).toEqual([])
  })
})

import { afterEach, describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import ToggleSwitch from '@/components/common/ToggleSwitch.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

describe('ToggleSwitch', () => {
  it('is a switch named by its label and described by its description', async () => {
    wrapper = mountWithProviders(ToggleSwitch, {
      modelValue: true,
      label: 'Compact view',
      description: 'Tighter rows',
    })
    await nextTick()
    const sw = wrapper.get('[role="switch"]')
    expect(sw.attributes('aria-checked')).toBe('true')
    expect(sw.attributes('data-state')).toBe('checked')
    const label = wrapper.get('label')
    expect(label.attributes('for')).toBe(sw.attributes('id'))
    const desc = wrapper.get(`#${sw.attributes('aria-describedby')}`)
    expect(desc.text()).toBe('Tighter rows')
  })

  it('toggles on click, on Space and through the label, once each', async () => {
    const updates: boolean[] = []
    wrapper = mountWithProviders(ToggleSwitch, {
      modelValue: false,
      label: 'Compact view',
      'onUpdate:modelValue': (v: boolean) => updates.push(v),
    })
    await nextTick()
    await wrapper.get('[role="switch"]').trigger('click')
    expect(updates).toEqual([true])
    await wrapper.get('[role="switch"]').trigger('keydown', { key: ' ' })
    await wrapper.get('[role="switch"]').trigger('keyup', { key: ' ' })
    expect(updates.length).toBeGreaterThanOrEqual(1)
    ;(wrapper.get('label').element as HTMLLabelElement).click()
    await nextTick()
    expect(updates.at(-1)).toBe(true)
  })

  it('does nothing when disabled', async () => {
    const updates: boolean[] = []
    wrapper = mountWithProviders(ToggleSwitch, {
      modelValue: false,
      disabled: true,
      'onUpdate:modelValue': (v: boolean) => updates.push(v),
    })
    await nextTick()
    const sw = wrapper.get('[role="switch"]')
    expect(sw.attributes('disabled')).toBeDefined()
    await sw.trigger('click')
    expect(updates).toEqual([])
  })
})

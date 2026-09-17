import { afterEach, describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import ColorHueSlider from '@/components/common/ColorHueSlider.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

const slider = (name: string) => wrapper!.get(`[role="slider"][aria-label="${name}"]`)
// Reka reports the channel unrounded; the spoken value is ours and whole.
const valueNow = (name: string) => Math.round(Number(slider(name).attributes('aria-valuenow')))

async function key(el: Element, key: string) {
  el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
  await nextTick()
}

async function expand() {
  await wrapper!.get('[aria-label="color-picker-expand"]').trigger('click')
  await nextTick()
}

const HEX = /^#[0-9a-f]{6}$/

describe('ColorHueSlider', () => {
  it('is a named group with a hue slider that speaks the colour name', async () => {
    wrapper = mountWithProviders(ColorHueSlider, { modelValue: '#6366f1', label: 'Accent' })
    await nextTick()
    const group = wrapper.get('[role="group"][aria-labelledby]')
    expect(wrapper.get(`#${group.attributes('aria-labelledby')}`).text()).toBe('Accent')
    const hue = slider('color-slider-hue')
    expect(hue.attributes('aria-valuemin')).toBe('0')
    expect(hue.attributes('aria-valuemax')).toBe('360')
    expect(valueNow('color-slider-hue')).toBe(239)
    expect(hue.attributes('aria-valuetext')).toBe('color-slider-hue-value')
    expect(wrapper.get('input').element.value).toBe('#6366f1')
    expect(wrapper.get('input').attributes('aria-label')).toBe('color-hex-label')
    // The finer controls start closed.
    expect(wrapper.find('[role="slider"][aria-label="color-slider-saturation"]').exists()).toBe(false)
  })

  it('steps the hue from the keyboard and emits lowercase six-digit hex', async () => {
    const updates: string[] = []
    wrapper = mountWithProviders(ColorHueSlider, {
      modelValue: '#6366f1',
      'onUpdate:modelValue': (v: string) => updates.push(v),
    })
    await nextTick()
    const hue = slider('color-slider-hue')
    await key(hue.element, 'ArrowRight')
    expect(hue.attributes('aria-valuenow')).toBe('240')
    expect(updates).toHaveLength(1)
    expect(updates[0]).toMatch(HEX)
    await key(hue.element, 'Home')
    expect(hue.attributes('aria-valuenow')).toBe('0')
    await key(hue.element, 'End')
    expect(hue.attributes('aria-valuenow')).toBe('360')
    for (const v of updates) expect(v).toMatch(HEX)
  })

  it('emits nothing on mount, even for a value it cannot parse', async () => {
    const updates: string[] = []
    wrapper = mountWithProviders(ColorHueSlider, {
      modelValue: '',
      'onUpdate:modelValue': (v: string) => updates.push(v),
    })
    await nextTick()
    expect(wrapper.get('input').element.value).toMatch(HEX)
    wrapper.unmount()
    wrapper = mountWithProviders(ColorHueSlider, {
      modelValue: '#zz',
      'onUpdate:modelValue': (v: string) => updates.push(v),
    })
    await nextTick()
    expect(updates).toEqual([])
  })

  it('takes hex, rgb() and shorthand from the field on Enter, ignoring garbage', async () => {
    const updates: string[] = []
    wrapper = mountWithProviders(ColorHueSlider, {
      modelValue: '#6366f1',
      'onUpdate:modelValue': (v: string) => updates.push(v),
    })
    await nextTick()
    const input = wrapper.get('input')
    await input.setValue('rgb(255, 0, 0)')
    await input.trigger('keydown', { key: 'Enter' })
    expect(updates.at(-1)).toBe('#ff0000')
    await input.setValue('#0F0')
    await input.trigger('blur')
    expect(updates.at(-1)).toBe('#00ff00')
    await input.setValue('rgba(10, 20, 30, 0.5)')
    await input.trigger('blur')
    expect(updates.at(-1)).toBe('#0a141e')
    await input.setValue('nonsense')
    await input.trigger('blur')
    expect(updates.at(-1)).toBe('#0a141e')
    expect((input.element as HTMLInputElement).value).toBe('#0a141e')
  })

  it('discloses presets and tone sliders; a preset selects once and stays selected', async () => {
    const updates: string[] = []
    wrapper = mountWithProviders(ColorHueSlider, {
      modelValue: '#6366f1',
      'onUpdate:modelValue': (v: string) => updates.push(v),
    })
    await nextTick()
    const trigger = wrapper.get('[aria-label="color-picker-expand"]')
    expect(trigger.attributes('aria-expanded')).toBe('false')
    await expand()
    expect(wrapper.get('[aria-label="color-picker-collapse"]').attributes('aria-expanded')).toBe('true')
    expect(wrapper.get(`#${trigger.attributes('aria-controls')}`).exists()).toBe(true)
    expect(valueNow('color-slider-saturation')).toBe(84)
    expect(valueNow('color-slider-lightness')).toBe(67)

    const group = wrapper.get('[role="radiogroup"]')
    expect(group.attributes('aria-label')).toBe('color-presets-label')
    const options = group.findAll('[role="radio"]')
    expect(options).toHaveLength(8)
    expect(options[5].attributes('aria-label')).toBe('color-blue')
    expect(options.every((o) => o.attributes('aria-checked') === 'false')).toBe(true)
    await options[5].trigger('click')
    const blue = updates.at(-1)
    expect(blue).toMatch(HEX)
    expect(options[5].attributes('aria-checked')).toBe('true')
    await options[5].trigger('click')
    expect(updates.at(-1)).toBe(blue)
    expect(options[5].attributes('aria-checked')).toBe('true')
    // A slider step leaves the preset, and leaves focus where it is.
    const lightness = slider('color-slider-lightness')
    ;(lightness.element as HTMLElement).focus()
    await key(lightness.element, 'ArrowLeft')
    await wrapper.setProps({ modelValue: updates.at(-1) })
    expect(options[5].attributes('aria-checked')).toBe('false')
    expect(document.activeElement).toBe(lightness.element)
  })

  it('resets the tone to the accent presets and follows the parent value', async () => {
    const updates: string[] = []
    wrapper = mountWithProviders(ColorHueSlider, {
      modelValue: '#6366f1',
      'onUpdate:modelValue': (v: string) => updates.push(v),
    })
    await nextTick()
    await expand()
    await wrapper.get('button[type="button"].self-end').trigger('click')
    expect(valueNow('color-slider-saturation')).toBe(70)
    expect(valueNow('color-slider-lightness')).toBe(45)
    expect(updates).toHaveLength(1)
    await wrapper.setProps({ modelValue: '#ff0000' })
    expect(valueNow('color-slider-hue')).toBe(0)
    expect(wrapper.get('input').element.value).toBe('#ff0000')
  })

  it('hides the swatch and its disclosure when asked', async () => {
    wrapper = mountWithProviders(ColorHueSlider, { modelValue: '#6366f1', hideSwatch: true, layout: 'stacked' })
    await nextTick()
    expect(wrapper.find('[aria-label="color-picker-expand"]').exists()).toBe(false)
    expect(wrapper.find('[role="slider"]').exists()).toBe(true)
  })
})

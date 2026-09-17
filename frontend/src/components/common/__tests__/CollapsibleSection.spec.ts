import { afterEach, describe, expect, it } from 'vitest'
import { h, nextTick } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import CollapsibleSection from '@/components/common/CollapsibleSection.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

describe('CollapsibleSection', () => {
  it('is one heading button that names, expands and controls its panel', async () => {
    const toggles: number[] = []
    wrapper = mountWithProviders(
      CollapsibleSection,
      { title: 'Recent tickets', isCollapsed: false, icon: 'clock', onToggle: () => toggles.push(1) },
      { default: () => h('p', 'body') },
    )
    await nextTick()
    const buttons = wrapper.findAll('button')
    expect(buttons).toHaveLength(1)
    const trigger = buttons[0]
    expect(wrapper.get('h3').element.contains(trigger.element)).toBe(true)
    expect(trigger.text()).toBe('Recent tickets')
    expect(trigger.attributes('aria-expanded')).toBe('true')
    const panel = wrapper.get(`#${trigger.attributes('aria-controls')}`)
    expect(panel.text()).toBe('body')
    await trigger.trigger('click')
    expect(toggles).toHaveLength(1)
  })

  it('unmounts the content while collapsed and reports it', async () => {
    wrapper = mountWithProviders(
      CollapsibleSection,
      { title: 'Docs', isCollapsed: true },
      { default: () => h('p', 'body') },
    )
    await nextTick()
    const trigger = wrapper.get('button')
    expect(trigger.attributes('aria-expanded')).toBe('false')
    expect(wrapper.text()).not.toContain('body')
  })
})

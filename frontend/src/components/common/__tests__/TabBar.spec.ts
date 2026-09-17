import { afterEach, describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import TabBar from '@/components/common/TabBar.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

const key = (el: Element, k: string) =>
  el.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true }))

describe('TabBar', () => {
  it('renders a named tablist with tabs, selection state and badges', async () => {
    wrapper = mountWithProviders(TabBar, {
      modelValue: 'unread',
      label: 'Filter',
      items: [
        { value: 'all', label: 'All' },
        { value: 'unread', label: 'Unread', badge: 3 },
        { value: 'mentions', label: 'Mentions', disabled: true },
      ],
    })
    await nextTick()
    const list = wrapper.get('[role="tablist"]')
    expect(list.attributes('aria-label')).toBe('Filter')
    const tabs = wrapper.findAll('[role="tab"]')
    expect(tabs.map((t) => t.attributes('aria-selected'))).toEqual(['false', 'true', 'false'])
    expect(tabs[1].text()).toContain('3')
    expect(tabs[2].attributes('data-disabled')).toBeDefined()
    // The roving group is the single tab stop until entered; it then
    // forwards focus to the active tab and the arrows take over.
    expect(list.attributes('tabindex')).toBe('0')
    expect(tabs.map((t) => t.attributes('tabindex'))).toEqual(['-1', '-1', '-1'])
  })

  it('moves and activates with the arrow keys, Home and End', async () => {
    const updates: string[] = []
    wrapper = mountWithProviders(TabBar, {
      modelValue: 'all',
      label: 'Filter',
      items: [
        { value: 'all', label: 'All' },
        { value: 'unread', label: 'Unread' },
        { value: 'mentions', label: 'Mentions' },
      ],
      'onUpdate:modelValue': (v: string) => updates.push(v),
    })
    await nextTick()
    const tabs = wrapper.findAll('[role="tab"]').map((t) => t.element as HTMLElement)
    tabs[0].focus()
    key(tabs[0], 'ArrowRight')
    await nextTick()
    expect(document.activeElement).toBe(tabs[1])
    // Automatic activation: focus selects.
    expect(updates).toEqual(['unread'])
    key(tabs[1], 'End')
    await nextTick()
    expect(document.activeElement).toBe(tabs[2])
    expect(updates).toEqual(['unread', 'mentions'])
    key(tabs[2], 'Home')
    await nextTick()
    expect(document.activeElement).toBe(tabs[0])
  })
})

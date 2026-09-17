import { afterEach, describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import FilterValueList from '@/components/views/FilterValueList.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

const option = (value: string) => ({ value, label: value[0].toUpperCase() + value.slice(1) })
const FEW = ['open', 'closed', 'pending'].map(option)
const MANY = ['alpha', 'beta', 'gamma', 'delta', 'epsilon', 'zeta', 'eta', 'theta', 'iota'].map(option)

const options = () => wrapper!.findAll('[role="option"]')
async function key(el: Element, key: string) {
  el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
  await nextTick()
}

describe('FilterValueList', () => {
  it('is a multi-select listbox that opens focused on the selected row and toggles from the keyboard', async () => {
    const toggles: string[] = []
    wrapper = mountWithProviders(FilterValueList, {
      options: FEW,
      selected: new Set(['closed']),
      label: 'Status',
      onToggle: (v: string) => toggles.push(v),
    })
    await nextTick()
    await nextTick()
    const listbox = wrapper.get('[role="listbox"]')
    expect(listbox.attributes('aria-multiselectable')).toBe('true')
    expect(listbox.attributes('aria-label')).toBe('Status')
    expect(wrapper.find('input').exists()).toBe(false)
    expect(options().map((o) => o.attributes('aria-selected'))).toEqual(['false', 'true', 'false'])
    expect(document.activeElement).toBe(options()[1].element)
    await key(options()[1].element, 'ArrowDown')
    expect(document.activeElement).toBe(options()[2].element)
    await key(options()[2].element, 'Enter')
    expect(toggles).toEqual(['pending'])
    await options()[1].trigger('click')
    expect(toggles).toEqual(['pending', 'closed'])
  })

  it('adds a filter for long lists that narrows the rows and toggles the first match on Enter', async () => {
    const toggles: string[] = []
    wrapper = mountWithProviders(FilterValueList, {
      options: MANY,
      selected: new Set(),
      onToggle: (v: string) => toggles.push(v),
    })
    await nextTick()
    // Reka focuses the filter a macrotask after mount.
    await new Promise((r) => setTimeout(r, 5))
    const input = wrapper.get('input').element as HTMLInputElement
    expect(document.activeElement).toBe(input)
    expect(options()).toHaveLength(9)
    input.value = 'eta'
    input.dispatchEvent(new Event('input', { bubbles: true }))
    await nextTick()
    await nextTick()
    expect(options().map((o) => o.text())).toEqual(['Beta', 'Zeta', 'Eta', 'Theta'])
    expect(input.getAttribute('aria-activedescendant')).toBe(options()[0].attributes('id'))
    await key(input, 'ArrowDown')
    await key(input, 'Enter')
    expect(toggles).toEqual(['zeta'])
    // Focus stays in the filter; the list is its own tab stop.
    expect(document.activeElement).toBe(input)
    expect(wrapper.get('[role="listbox"]').attributes('tabindex')).toBe('0')
  })

  it('shows the clear affordance only with a selection', async () => {
    wrapper = mountWithProviders(FilterValueList, { options: FEW, selected: new Set(), autoFocus: false })
    await nextTick()
    expect(wrapper.text()).not.toContain('views-filter-value-clear')
    await wrapper.setProps({ selected: new Set(['open']) })
    expect(wrapper.text()).toContain('views-filter-value-clear')
  })
})

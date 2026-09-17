import { afterEach, describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import SegmentedControl from '@/components/common/SegmentedControl.vue'
import ListDensityToggle from '@/components/common/ListDensityToggle.vue'
import FilterToggle from '@/components/common/FilterToggle.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

const key = (el: Element, k: string) =>
  el.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true }))

const options = [
  { value: 'status', label: 'Status' },
  { value: 'assignee', label: 'Assignee' },
  { value: 'priority', label: 'Priority' },
]

describe('SegmentedControl', () => {
  it('is a named radio group with exactly one checked radio', async () => {
    wrapper = mountWithProviders(SegmentedControl, {
      modelValue: 'assignee',
      options,
      ariaLabel: 'Group by',
    })
    await nextTick()
    const group = wrapper.get('[role="radiogroup"]')
    expect(group.attributes('aria-label')).toBe('Group by')
    const radios = wrapper.findAll('[role="radio"]')
    expect(radios.map((r) => r.attributes('aria-checked'))).toEqual(['false', 'true', 'false'])
    expect(radios[1].attributes('data-state')).toBe('checked')
    // The roving group is the single tab stop until entered; it then
    // forwards focus to the checked radio and the arrows take over.
    expect(group.attributes('tabindex')).toBe('0')
    expect(radios.map((r) => r.attributes('tabindex'))).toEqual(['-1', '-1', '-1'])
  })

  it('selects with a click and with the arrows, looping at the ends', async () => {
    const updates: string[] = []
    wrapper = mountWithProviders(SegmentedControl, {
      modelValue: 'status',
      options,
      'onUpdate:modelValue': (v: string) => updates.push(v),
    })
    await nextTick()
    const radios = wrapper.findAll('[role="radio"]').map((r) => r.element as HTMLElement)
    radios[2].click()
    expect(updates).toEqual(['priority'])
    // Arrow moves focus; Reka then checks the focused radio on the next
    // macrotask while the key is down (keydown only here, no keyup).
    radios[0].focus()
    key(radios[0], 'ArrowRight')
    await nextTick()
    expect(document.activeElement).toBe(radios[1])
    await new Promise((r) => setTimeout(r, 1))
    expect(updates.at(-1)).toBe('assignee')
    key(radios[1], 'ArrowLeft')
    key(radios[0], 'ArrowLeft')
    await nextTick()
    expect(document.activeElement).toBe(radios[2])
  })

  it('does not re-emit the current value', async () => {
    const updates: string[] = []
    wrapper = mountWithProviders(SegmentedControl, {
      modelValue: 'status',
      options,
      'onUpdate:modelValue': (v: string) => updates.push(v),
    })
    await nextTick()
    ;(wrapper.findAll('[role="radio"]')[0].element as HTMLElement).click()
    expect(updates).toEqual([])
  })
})

describe('ListDensityToggle', () => {
  it('is a radio group of named icon buttons', async () => {
    const set: string[] = []
    wrapper = mountWithProviders(ListDensityToggle, {
      density: 'cosy',
      onSetDensity: (v: string) => set.push(v),
    })
    await nextTick()
    const radios = wrapper.findAll('[role="radio"]')
    expect(radios.map((r) => r.attributes('aria-label'))).toEqual([
      'views-display-menu-density-compact',
      'views-display-menu-density-cosy',
      'views-display-menu-density-comfortable',
    ])
    expect(radios[1].attributes('aria-checked')).toBe('true')
    ;(radios[2].element as HTMLElement).click()
    expect(set).toEqual(['comfortable'])
  })
})

describe('FilterToggle', () => {
  it('is a pressed toggle named by its label', async () => {
    const updates: boolean[] = []
    wrapper = mountWithProviders(FilterToggle, {
      modelValue: true,
      label: 'High priority only',
      activeClass: 'text-accent',
      'onUpdate:modelValue': (v: boolean) => updates.push(v),
    })
    await nextTick()
    const btn = wrapper.get('button')
    expect(btn.attributes('aria-pressed')).toBe('true')
    expect(btn.attributes('aria-label')).toBe('High priority only')
    expect(btn.classes()).toContain('text-accent')
    await btn.trigger('click')
    expect(updates).toEqual([false])
  })
})

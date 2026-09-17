import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

vi.mock('@/composables/useAssignmentPickerQueries', () => ({
  useAssignmentPickerQueries: () => ({
    allGroups: ref([{ id: 7, name: 'Helpdesk' }]),
    searchedUsers: ref([
      { uuid: 'u-1', name: 'Noah Bennett', email: 'noah@example.test', avatar_url: null },
    ]),
    loading: ref(false),
  }),
}))

import AssignmentPicker from '@/components/common/AssignmentPicker.vue'

const settle = (ms = 30) => new Promise((r) => setTimeout(r, ms))
let wrapper: VueWrapper | null = null
beforeEach(() => {
  Element.prototype.scrollIntoView = () => {}
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

describe('AssignmentPicker', () => {
  it('lists groups and users as grouped options and adds a chip on pick', async () => {
    const updates: unknown[] = []
    wrapper = mountWithProviders(AssignmentPicker, {
      selectedItems: [],
      'onUpdate:selectedItems': (v: unknown) => updates.push(v),
    })
    await settle()
    const input = wrapper.get('input').element as HTMLInputElement
    expect(input.getAttribute('role')).toBe('combobox')
    input.focus()
    input.dispatchEvent(new Event('focus'))
    await settle()
    const options = Array.from(document.body.querySelectorAll<HTMLElement>('[role="option"]'))
    expect(options.map((o) => o.textContent?.replace(/\s+/g, ' ').trim())).toEqual([
      'Helpdesk',
      'NNoah Bennettnoah@example.test',
    ])
    const groups = Array.from(document.body.querySelectorAll('[role="group"]'))
    expect(
      groups.map((g) => document.getElementById(g.getAttribute('aria-labelledby') ?? '')?.textContent?.trim()),
    ).toEqual(['assignment-picker-section-groups', 'assignment-picker-section-users'])
    // Opening highlights the first row; Enter picks it and it becomes a chip.
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }))
    await settle()
    expect(updates).toEqual([[{ type: 'group', id: '7', name: 'Helpdesk' }]])
  })

  it('renders chips for the selection with a remove control', async () => {
    const updates: unknown[] = []
    wrapper = mountWithProviders(AssignmentPicker, {
      selectedItems: [{ type: 'user', id: 'u-1', name: 'Noah Bennett' }],
      'onUpdate:selectedItems': (v: unknown) => updates.push(v),
    })
    await settle()
    const remove = wrapper.get('button[aria-label]')
    await remove.trigger('click')
    expect(updates).toEqual([[]])
  })
})

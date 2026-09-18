import { afterEach, describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import DatePicker from '@/components/common/DatePicker.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

const segment = (part: string) => wrapper!.get(`[data-reka-date-field-segment="${part}"]`)

async function key(el: Element, key: string) {
  el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
  await nextTick()
}

async function open() {
  await wrapper!.get('[aria-label="date-picker-open-calendar-aria"]').trigger('click')
  await nextTick()
  await nextTick()
}

const day = (iso: string) =>
  document.body.querySelector<HTMLElement>(`[data-reka-calendar-cell-trigger][data-value="${iso}"]`)

// Focus leaving the control (no related target) is what commits a
// typed value.
async function blur() {
  wrapper!.get('[role="group"]').element.dispatchEvent(new FocusEvent('focusout', { bubbles: true }))
  await nextTick()
}

async function escape() {
  document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
  await nextTick()
  await nextTick()
}

describe('DatePicker', () => {
  it('is a named group of three labelled spinbuttons with a calendar trigger', async () => {
    wrapper = mountWithProviders(DatePicker, { ariaLabel: 'Due date' })
    await nextTick()
    const group = wrapper.get('[role="group"]')
    expect(group.attributes('aria-label')).toBe('Due date')
    const spins = wrapper.findAll('[role="spinbutton"]')
    expect(spins).toHaveLength(3)
    const labels = spins.map((s) => s.attributes('aria-label')).sort()
    expect(labels).toEqual(['date-segment-day', 'date-segment-month', 'date-segment-year'])
    const trigger = wrapper.get('[aria-label="date-picker-open-calendar-aria"]')
    expect(trigger.attributes('aria-haspopup')).toBe('dialog')
    expect(trigger.attributes('aria-expanded')).toBe('false')
  })

  it('reflects the ISO value, steps it from a segment and commits on blur or Enter', async () => {
    const updates: string[] = []
    wrapper = mountWithProviders(DatePicker, {
      modelValue: '2026-09-17',
      'onUpdate:modelValue': (v: string) => updates.push(v),
    })
    await nextTick()
    expect(segment('day').attributes('aria-valuenow')).toBe('17')
    expect(segment('year').attributes('aria-valuenow')).toBe('2026')
    await key(segment('day').element, 'ArrowUp')
    expect(segment('day').attributes('aria-valuenow')).toBe('18')
    expect(updates).toEqual([])
    // Moving between segments is not a commit.
    segment('day').element.dispatchEvent(
      new FocusEvent('focusout', { bubbles: true, relatedTarget: segment('year').element }),
    )
    await nextTick()
    expect(updates).toEqual([])
    await blur()
    expect(updates).toEqual(['2026-09-18'])
    await wrapper.setProps({ modelValue: '2026-09-18' })
    await key(segment('day').element, 'ArrowUp')
    await key(segment('day').element, 'Enter')
    expect(updates).toEqual(['2026-09-18', '2026-09-19'])
  })

  it('follows the parent value, from empty and back', async () => {
    wrapper = mountWithProviders(DatePicker, { modelValue: '' })
    await nextTick()
    expect(segment('day').attributes('data-placeholder')).toBe('')
    await wrapper.setProps({ modelValue: '2027-01-02' })
    expect(segment('day').attributes('aria-valuenow')).toBe('2')
    expect(segment('month').attributes('aria-valuenow')).toBe('1')
    expect(segment('year').attributes('aria-valuenow')).toBe('2027')
    await wrapper.setProps({ modelValue: '' })
    expect(segment('day').attributes('data-placeholder')).toBe('')
  })

  it('leaves a value it cannot parse alone', async () => {
    const updates: string[] = []
    wrapper = mountWithProviders(DatePicker, {
      modelValue: 'not-a-date',
      'onUpdate:modelValue': (v: string) => updates.push(v),
    })
    await nextTick()
    expect(segment('day').attributes('data-placeholder')).toBe('')
    await blur()
    wrapper.unmount()
    wrapper = null
    expect(updates).toEqual([])
  })

  it('holds a value outside the bounds without committing it', async () => {
    const updates: string[] = []
    wrapper = mountWithProviders(DatePicker, {
      modelValue: '2026-09-17',
      min: '2026-09-17',
      'onUpdate:modelValue': (v: string) => updates.push(v),
    })
    await nextTick()
    await key(segment('day').element, 'ArrowDown')
    expect(segment('day').attributes('aria-valuenow')).toBe('16')
    await blur()
    expect(updates).toEqual([])
    expect(segment('day').attributes('aria-invalid')).toBe('true')
    expect(wrapper.get('[role="group"]').attributes('data-invalid')).toBe('')
    await key(segment('day').element, 'ArrowUp')
    expect(segment('day').attributes('aria-invalid')).toBeUndefined()
  })

  it('opens a calendar that selects on click and keeps the selection on a second click', async () => {
    const updates: string[] = []
    wrapper = mountWithProviders(DatePicker, {
      modelValue: '2026-09-17',
      'onUpdate:modelValue': (v: string) => updates.push(v),
    })
    await nextTick()
    await open()
    expect(document.body.querySelector('[role="dialog"]')).not.toBeNull()
    // Reka would call the grid "Event Date" otherwise.
    expect(document.body.querySelector('[role="dialog"] [aria-label$="2026"]')?.getAttribute('aria-label')).toBe(
      'date-picker-calendar-aria, September 2026',
    )
    expect(day('2026-09-17')?.getAttribute('data-selected')).toBe('true')
    // The grid opens on the value's month with the value as the roving stop.
    expect(day('2026-09-17')?.getAttribute('tabindex')).toBe('0')
    day('2026-09-17')!.click()
    await nextTick()
    expect(updates).toEqual([])
    day('2026-09-21')!.click()
    await nextTick()
    expect(updates).toEqual(['2026-09-21'])
  })

  it('opens on the value, not on today', async () => {
    wrapper = mountWithProviders(DatePicker, { modelValue: '2031-03-09' })
    await nextTick()
    await open()
    expect(day('2031-03-09')?.getAttribute('data-selected')).toBe('true')
    expect(day('2031-03-09')?.getAttribute('data-focused')).toBe('')
    expect(day('2031-03-09')?.getAttribute('tabindex')).toBe('0')
  })

  it('disables days outside the bounds', async () => {
    wrapper = mountWithProviders(DatePicker, { modelValue: '2026-09-17', min: '2026-09-10', max: '2026-09-20' })
    await nextTick()
    await open()
    expect(day('2026-09-09')?.hasAttribute('data-disabled')).toBe(true)
    expect(day('2026-09-10')?.hasAttribute('data-disabled')).toBe(false)
    expect(day('2026-09-21')?.hasAttribute('data-disabled')).toBe(true)
  })

  it('commits a range when the calendar closes: start only after one pick, both ordered after two', async () => {
    const events: Array<[string, string]> = []
    wrapper = mountWithProviders(DatePicker, {
      range: true,
      start: '2026-09-01',
      end: '2026-09-05',
      'onUpdate:start': (v: string) => events.push(['start', v]),
      'onUpdate:end': (v: string) => events.push(['end', v]),
    })
    await nextTick()
    const labels = wrapper.findAll('[role="spinbutton"]').map((s) => s.attributes('aria-label'))
    expect(labels).toContain('date-range-start-segment')
    expect(labels).toContain('date-range-end-segment')
    await open()
    day('2026-09-20')!.click()
    await nextTick()
    // One end picked: the calendar stays open and nothing is committed.
    expect(document.body.querySelector('[role="dialog"]')).not.toBeNull()
    expect(events).toEqual([])
    await escape()
    expect(document.body.querySelector('[role="dialog"]')).toBeNull()
    expect(events).toEqual([
      ['start', '2026-09-20'],
      ['end', ''],
    ])
    await wrapper.setProps({ start: '2026-09-20', end: '' })
    await open()
    day('2026-09-10')!.click()
    await nextTick()
    await nextTick()
    // Both ends picked: the calendar closes and the range commits ordered.
    await nextTick()
    expect(document.body.querySelector('[role="dialog"]')).toBeNull()
    expect(events.slice(2)).toEqual([
      ['start', '2026-09-10'],
      ['end', '2026-09-20'],
    ])
  })

  it('renders a label tied to the field and puts error on the segments, not the group', async () => {
    wrapper = mountWithProviders(DatePicker, { label: 'Expected return', error: true })
    await nextTick()
    const label = wrapper.get('label')
    expect(label.text()).toBe('Expected return')
    expect(wrapper.get(`#${label.attributes('for')}`).element.tagName).toBe('INPUT')
    const group = wrapper.get('[role="group"]')
    expect(group.attributes('aria-labelledby')).toBe(label.attributes('id'))
    expect(group.attributes('aria-invalid')).toBeUndefined()
    for (const spin of wrapper.findAll('[role="spinbutton"]')) {
      expect(spin.attributes('aria-invalid')).toBe('true')
    }
  })

  it('shows an error message or a description below the field and describes the group with it', async () => {
    wrapper = mountWithProviders(DatePicker, { label: 'Due', description: 'Working days only' })
    await nextTick()
    let group = wrapper.get('[role="group"]')
    expect(wrapper.get(`#${group.attributes('aria-describedby')}`).text()).toBe('Working days only')
    expect(wrapper.get('[role="spinbutton"]').attributes('aria-invalid')).toBeUndefined()
    await wrapper.setProps({ error: 'Pick a date after today' })
    group = wrapper.get('[role="group"]')
    expect(wrapper.get(`#${group.attributes('aria-describedby')}`).text()).toBe('Pick a date after today')
    expect(wrapper.get('[role="spinbutton"]').attributes('aria-invalid')).toBe('true')
  })
})

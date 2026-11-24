import { afterEach, describe, expect, it } from 'vitest'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

import BaseDropdown from '@/components/common/BaseDropdown.vue'
import FormNumber from '@/components/common/FormNumber.vue'
import TimeElapsedTriggerForm from '../TimeElapsedTriggerForm.vue'
import { ELAPSED_CLOCKS, type ElapsedDuration } from '../triggerConfig'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

function mountForm(
  props: Partial<{ clockName: string; after: ElapsedDuration; repeat: boolean; every: ElapsedDuration; showProblems: boolean }> = {},
) {
  wrapper = mountWithProviders(TimeElapsedTriggerForm, {
    clockName: '',
    after: { amount: null, unit: 'days' },
    repeat: false,
    every: { amount: 1, unit: 'days' },
    ...props,
  })
  return wrapper
}

const dropdown = (w: VueWrapper, test: string) =>
  w.findAllComponents(BaseDropdown).find((d) => d.vm.$attrs['data-test'] === test)!

describe('TimeElapsedTriggerForm', () => {
  it('lists every clock the server runs, each with a hint', () => {
    const w = mountForm()
    const options = dropdown(w, 'elapsed-clock').props('options') as { value: string; description?: string }[]
    expect(options.map((o) => o.value)).toEqual([...ELAPSED_CLOCKS])
    expect(options.map((o) => o.value)).not.toContain('until_sla_due_at')
    for (const o of options) expect(o.description).toBe(`admin-rule-editor-elapsed-clock-${o.value.replace(/_/g, '-')}-hint`)
    expect(options.find((o) => o.value === 'awaiting_requester')!.description).toBeTruthy()
  })

  it('shows the picked clock hint, and asks for time before the due date on that clock', () => {
    const w = mountForm({ clockName: 'until_due_at', after: { amount: 1, unit: 'days' } })
    expect(dropdown(w, 'elapsed-clock').props('description')).toBe('admin-rule-editor-elapsed-clock-until-due-at-hint')
    expect(w.findAllComponents(FormNumber)[0].props('label')).toBe('admin-rule-editor-elapsed-before-label')
  })

  it('keeps a missing pick quiet until a save attempt, but shows a time out of range at once', async () => {
    const w = mountForm()
    expect(w.text()).not.toContain('admin-rule-editor-elapsed-clock-missing')
    expect(w.text()).not.toContain('admin-rule-editor-elapsed-after-missing')
    await w.setProps({ showProblems: true })
    expect(w.text()).toContain('admin-rule-editor-elapsed-clock-missing')
    expect(w.text()).toContain('admin-rule-editor-elapsed-after-missing')

    const ranged = mountForm({ clockName: 'since_created', after: { amount: 366, unit: 'days' } })
    expect(ranged.text()).toContain('admin-rule-editor-elapsed-after-range')
  })

  it('sends the amount and unit it was given back together', () => {
    const w = mountForm({ clockName: 'since_created', after: { amount: 3, unit: 'days' } })
    dropdown(w, 'elapsed-after-unit').vm.$emit('update:modelValue', 'hours')
    w.findAllComponents(FormNumber)[0].vm.$emit('update:modelValue', 5)
    expect(w.findComponent(TimeElapsedTriggerForm).emitted('update:after')).toEqual([
      [{ amount: 3, unit: 'hours' }],
      [{ amount: 5, unit: 'days' }],
    ])
  })

  it('shows the repeat time only while the repeat is on', async () => {
    const w = mountForm({ clockName: 'since_created', after: { amount: 3, unit: 'days' } })
    expect(w.find('[data-test="elapsed-every"]').exists()).toBe(false)
    await w.setProps({ repeat: true })
    expect(w.findAllComponents(FormNumber)).toHaveLength(2)
    expect(w.findAllComponents(FormNumber)[1].props('modelValue')).toBe(1)
    expect(dropdown(w, 'elapsed-every-unit').props('modelValue')).toBe('days')
  })
})

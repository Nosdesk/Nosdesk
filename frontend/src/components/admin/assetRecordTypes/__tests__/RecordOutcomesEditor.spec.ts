import { afterEach, describe, expect, it } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import BaseDropdown from '@/components/common/BaseDropdown.vue'
import RecordOutcomesEditor from '../RecordOutcomesEditor.vue'
import { outcomeRows, outcomesFromRows, sameOutcomes, type OutcomeRow } from '../recordTypeForm'

let wrapper: VueWrapper | null = null
let rows: OutcomeRow[] = []

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

/** The editor bound to `rows`, as `v-model` would bind it. */
async function mountEditor(start: OutcomeRow[], typeSlug = 'repair', catalogueKeys: string[] = []) {
  rows = start
  wrapper = mountWithProviders(RecordOutcomesEditor, {
    modelValue: rows,
    typeSlug,
    catalogueKeys,
    'onUpdate:modelValue': async (next: OutcomeRow[]) => {
      rows = next
      await wrapper!.setProps({ modelValue: next })
    },
  })
  await flushPromises()
  return wrapper
}

const nameInputs = () => wrapper!.findAll('[data-test="outcome-row"] input[type="text"]')
const categories = () => wrapper!.findAllComponents(BaseDropdown)

describe('RecordOutcomesEditor', () => {
  it('gives a new outcome a key from its name, unique among the others', async () => {
    await mountEditor(outcomeRows([{ key: 'replaced', category: 'replaced', label: 'Swapped' }]))
    const add = wrapper!.findAll('button').find((b) => b.text().includes('admin-asset-record-types-outcome-add'))!
    await add.trigger('click')
    await nameInputs()[1].setValue('Replaced')
    await flushPromises()

    expect(rows.map((r) => r.key)).toEqual(['replaced', 'replaced_2'])
    expect(rows[1]).toMatchObject({ label: 'Replaced', category: 'done', isNew: true })
  })

  it('keeps a saved outcome\'s key when it is renamed', async () => {
    await mountEditor(outcomeRows([{ key: 'passed', category: 'done', label: 'Passed' }]))
    await nameInputs()[0].setValue('Passed first time')
    expect(rows[0]).toMatchObject({ key: 'passed', label: 'Passed first time' })
  })

  it('sets each outcome\'s category', async () => {
    await mountEditor(
      outcomeRows([
        { key: 'repaired', category: 'done', label: 'Repaired' },
        { key: 'swapped', category: 'done', label: 'Swapped' },
      ]),
    )
    expect(categories().map((d) => d.props('modelValue'))).toEqual(['done', 'done'])
    expect(categories()[0].props('options').map((o: { value: string }) => o.value)).toEqual([
      'done',
      'not_done',
      'no_fault',
      'replaced',
    ])

    categories()[1].vm.$emit('update:modelValue', 'replaced')
    await flushPromises()
    expect(rows.map((r) => r.category)).toEqual(['done', 'replaced'])
    expect(categories()[1].props('modelValue')).toBe('replaced')
  })

  it('reorders and removes outcomes', async () => {
    await mountEditor(
      outcomeRows([
        { key: 'a', category: 'done', label: 'A' },
        { key: 'b', category: 'not_done', label: 'B' },
        { key: 'c', category: 'no_fault', label: 'C' },
      ]),
    )
    await wrapper!.findAll('button[aria-label="admin-asset-record-types-outcome-move-up"]')[2].trigger('click')
    expect(rows.map((r) => r.key)).toEqual(['a', 'c', 'b'])
    await wrapper!.findAll('button[aria-label="admin-asset-record-types-outcome-remove"]')[0].trigger('click')
    expect(rows.map((r) => r.key)).toEqual(['c', 'b'])
  })

  it('shows a built-in outcome\'s catalogue name until it has its own, and saves no label for it', async () => {
    const stored = [
      { key: 'repaired', category: 'done' as const },
      { key: 'sent_back', category: 'not_done' as const, label: 'Sent back' },
    ]
    await mountEditor(outcomeRows(stored), 'repair', ['repaired'])
    const [catalogue, own] = nameInputs()
    expect((catalogue.element as HTMLInputElement).value).toBe('')
    expect(catalogue.attributes('placeholder')).toBe('asset-record-outcome-repair-repaired')
    expect(catalogue.attributes('required')).toBeUndefined()
    expect(own.attributes('placeholder')).toBe('admin-asset-record-types-outcome-name-placeholder')
    expect(own.attributes('required')).toBeDefined()

    const out = outcomesFromRows(rows)
    expect(out).toEqual([
      { key: 'repaired', category: 'done', label: null },
      { key: 'sent_back', category: 'not_done', label: 'Sent back' },
    ])
    expect(sameOutcomes(out, stored)).toBe(true)
  })
})

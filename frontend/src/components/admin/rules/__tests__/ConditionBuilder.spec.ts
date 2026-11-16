import { afterEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h, ref } from 'vue'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { RuleConditionLeaf, RuleConditionOp, RuleTriggerKind } from '@nosdesk/core/types/rule'

vi.mock('@/services/activeWorkspace', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/services/activeWorkspace')>()),
  workspaceReady: () => true,
}))
vi.mock('@nosdesk/core/stores/workflowStates', () => ({
  useWorkflowStatesStore: () => ({ load: async () => [], states: [{ id: 4, name: 'Open', archived_at: null }] }),
}))
vi.mock('@nosdesk/core/stores/tags', () => ({ useTagsStore: () => ({ tags: [{ id: 5, name: 'vip' }] }) }))
vi.mock('@nosdesk/core/services/categoryService', () => ({
  categoryService: { getAllCategoriesAdmin: async () => [{ id: 8, name: 'Hardware' }] },
}))
vi.mock('@/services/userService', () => ({
  default: { getUsersBatch: async () => [{ uuid: 'u-ana', name: 'Ana Silva' }] },
}))
// The pickers, reduced to what they were given and what they send back.
vi.mock('@/components/common/BaseDropdown.vue', () => ({
  default: {
    name: 'BaseDropdownStub',
    props: ['options', 'modelValue', 'multiple', 'ariaLabel', 'disabled', 'placeholder', 'size'],
    emits: ['update:modelValue'],
    template: '<div data-test="dropdown" :data-label="ariaLabel"></div>',
  },
}))
vi.mock('@/components/ticketComponents/UserPicker.vue', () => ({
  default: {
    name: 'UserPickerStub',
    props: ['modelValue', 'type', 'exclude', 'currentUser', 'placeholder'],
    emits: ['update:modelValue', 'pick'],
    template: '<div data-test="user-picker"></div>',
  },
}))
vi.mock('@/components/common/FormNumber.vue', () => ({
  default: {
    name: 'FormNumberStub',
    props: ['modelValue'],
    emits: ['update:modelValue'],
    template: '<input data-test="number" type="number" @input="$emit(\'update:modelValue\', Number($event.target.value))" />',
  },
}))

import ConditionBuilder from '../ConditionBuilder.vue'
import { addChild, defaultValue, emptyGroup, fieldDef, fieldsFor, opsFor, toWire, valueShape, type ConditionGroup } from '../conditionTree'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

async function mountBuilder(root: ConditionGroup, trigger: RuleTriggerKind = 'ticket_updated') {
  const model = ref(root)
  const Harness = defineComponent({
    setup: () => () =>
      h(ConditionBuilder, {
        modelValue: model.value,
        triggerKind: trigger,
        'onUpdate:modelValue': (next: ConditionGroup) => {
          model.value = next
        },
      }),
  })
  wrapper = mountWithProviders(Harness, {}, {}, [[PiniaColada, {}] as never])
  await flushPromises()
  return { w: wrapper, model }
}

const button = (label: string, within: ParentNode = document.body) =>
  Array.from(within.querySelectorAll<HTMLButtonElement>('button')).filter((b) => b.textContent?.includes(label))
const removeButtons = (label: string) =>
  Array.from(document.body.querySelectorAll<HTMLButtonElement>(`button[aria-label="${label}"]`))
const dropdowns = (w: VueWrapper, label: string) =>
  w.findAllComponents({ name: 'BaseDropdownStub' }).filter((d) => d.props('ariaLabel') === label)

describe('ConditionBuilder', () => {
  it('starts with no conditions and builds nested groups', async () => {
    const { w, model } = await mountBuilder(emptyGroup())
    expect(w.text()).toContain('admin-rule-conditions-empty')
    expect(toWire(model.value)).toEqual([])

    button('admin-rule-conditions-add')[0].click()
    await flushPromises()
    expect(model.value.children).toHaveLength(1)

    button('admin-rule-conditions-add-group')[0].click()
    await flushPromises()
    const nested = model.value.children[1] as ConditionGroup
    expect(nested).toMatchObject({ kind: 'group', mode: 'any' })
    expect(nested.children).toHaveLength(1)

    // The nested group's own add button adds inside it.
    const groupEl = document.body.querySelectorAll('[data-test="condition-group"]')[1]
    button('admin-rule-conditions-add', groupEl)[0].click()
    await flushPromises()
    expect((model.value.children[1] as ConditionGroup).children).toHaveLength(2)

    // Set the nested group to "none of these".
    const none = Array.from(groupEl.querySelectorAll<HTMLButtonElement>('[role="radio"]')).find(
      (b) => b.textContent?.trim() === 'admin-rule-conditions-mode-none',
    )
    none!.click()
    await flushPromises()
    expect((model.value.children[1] as ConditionGroup).mode).toBe('none')
    expect(toWire(model.value)).toMatchObject({
      kind: 'and',
      children: [{ kind: 'leaf' }, { kind: 'not', child: { kind: 'or', children: [{}, {}] } }],
    })

    // Remove the nested group's second condition, then the group.
    removeButtons('admin-rule-conditions-remove')[2].click()
    await flushPromises()
    expect((model.value.children[1] as ConditionGroup).children).toHaveLength(1)
    removeButtons('admin-rule-conditions-remove-group')[0].click()
    await flushPromises()
    expect(model.value.children).toHaveLength(1)

    removeButtons('admin-rule-conditions-remove')[0].click()
    await flushPromises()
    expect(toWire(model.value)).toEqual([])
  })

  it('offers only the operators the field takes, and changes the field', async () => {
    const { w, model } = await mountBuilder(addChild(emptyGroup(), [], { kind: 'leaf', field: 'ticket.title', op: 'contains', value: 'vpn' }))
    const ops = () => dropdowns(w, 'admin-rule-conditions-op-label')[0].props('options').map((o: { value: string }) => o.value)
    expect(ops()).not.toContain('is_null')

    dropdowns(w, 'admin-rule-conditions-field-label')[0].vm.$emit('update:modelValue', 'ticket.tag_ids')
    await flushPromises()
    expect(model.value.children[0]).toEqual({ kind: 'leaf', field: 'ticket.tag_ids', op: 'has_any', value: [] })
    expect(ops()).toEqual(['has_any', 'has_all', 'is_null', 'is_not_null'])
  })

  it('flags a condition on a field another trigger carries', async () => {
    const { w } = await mountBuilder(
      addChild(emptyGroup(), [], { kind: 'leaf', field: 'reply.kind', op: 'eq', value: 'agent' }),
      'ticket_created',
    )
    expect(w.text()).toContain('admin-rule-conditions-problem-trigger')
  })

  // Every field and operator the server accepts, with a value put in through
  // the picker the builder shows for it.
  const cases: [string, RuleConditionOp, RuleTriggerKind][] = []
  for (const trigger of ['ticket_updated', 'ticket_replied', 'time_elapsed'] as RuleTriggerKind[]) {
    for (const def of fieldsFor(trigger)) {
      if (trigger !== 'ticket_updated' && !def.triggerOnly) continue
      for (const op of opsFor(def.name)) cases.push([def.name, op, trigger])
    }
  }

  it.each(cases)('%s %s takes its value', async (field, op, trigger) => {
    const leaf: RuleConditionLeaf = { kind: 'leaf', field, op, value: defaultValue(field, op) }
    const { w, model } = await mountBuilder(addChild(emptyGroup(), [], leaf), trigger)
    const kind = fieldDef(field)!.kind
    const shape = valueShape(op)
    const valueNow = () => (toWire(model.value) as { children: RuleConditionLeaf[] }).children[0].value

    if (shape === 'none') {
      expect(dropdowns(w, 'admin-rule-conditions-value-label')).toHaveLength(0)
      expect(w.findAll('input')).toHaveLength(0)
      expect(w.find('[data-test="user-picker"]').exists()).toBe(false)
      expect(valueNow()).toBeNull()
      return
    }

    let expected: unknown
    if (kind === 'uuid') {
      const picker = w.findComponent({ name: 'UserPickerStub' })
      if (shape === 'list') {
        picker.vm.$emit('pick', { uuid: 'u-new', name: 'Noor Haddad' })
        expected = ['u-new']
      } else {
        picker.vm.$emit('update:modelValue', 'u-ana')
        expected = 'u-ana'
      }
    } else if (['enum', 'int_id', 'int_array', 'string_array'].includes(kind)) {
      const picker = dropdowns(w, 'admin-rule-conditions-value-label')[0]
      expect(Boolean(picker.props('multiple'))).toBe(shape === 'list')
      const first = picker.props('options')[0].value
      expect(first).toBeDefined()
      expected = shape === 'list' ? [first] : first
      picker.vm.$emit('update:modelValue', expected)
    } else if (kind === 'timestamp') {
      await w.get('input[type="datetime-local"]').setValue('2026-03-04T05:06')
      expected = new Date('2026-03-04T05:06').toISOString()
    } else if (kind === 'number') {
      await w.get('[data-test="number"]').setValue('15')
      expected = 15
    } else if (shape === 'list') {
      await w.get('input').setValue('alpha, beta')
      expected = ['alpha', 'beta']
    } else {
      await w.get('input').setValue('vpn')
      expected = 'vpn'
    }
    await flushPromises()
    expect(valueNow()).toEqual(expected)
    expect(w.text()).not.toContain('admin-rule-conditions-problem')
  })

  it('names the people a list of people holds, and drops one', async () => {
    const { w, model } = await mountBuilder(
      addChild(emptyGroup(), [], { kind: 'leaf', field: 'ticket.assignee_uuid', op: 'in', value: ['u-ana'] }),
    )
    await flushPromises()
    expect(w.text()).toContain('Ana Silva')
    removeButtons('admin-rule-conditions-person-remove')[0].click()
    await flushPromises()
    expect(model.value.children[0]).toMatchObject({ value: [] })
  })
})

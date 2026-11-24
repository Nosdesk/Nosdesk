import { afterEach, describe, expect, it, vi } from 'vitest'
import { type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { mountWithProviders } from '@/test/mountWithProviders'

// The pickers, reduced to what they were given and what they send back.
vi.mock('@/components/common/BaseDropdown.vue', () => ({
  default: {
    name: 'BaseDropdownStub',
    props: ['options', 'modelValue', 'size'],
    emits: ['update:modelValue'],
    template: '<div data-test="dropdown"></div>',
  },
}))
vi.mock('@/components/ticketComponents/UserPicker.vue', () => ({
  default: {
    name: 'UserPickerStub',
    props: ['modelValue', 'type', 'currentUser', 'placeholder', 'hideInlineClear'],
    emits: ['update:modelValue'],
    template: '<div data-test="user-picker"></div>',
  },
}))

import AssetRecordStepForm from '../AssetRecordStepForm.vue'
import NotifyStepForm from '../NotifyStepForm.vue'
import ReplyStepForm from '../ReplyStepForm.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

const ANA = { uuid: 'u-ana', name: 'Ana Silva' }
const BEN = { uuid: 'u-ben', name: 'Ben Ito' }
const userRow = (uuid: string) => [ANA, BEN].find((u) => u.uuid === uuid) ?? null

/** Every `update` the form sent, as [field, value] pairs. */
const updates = (w: VueWrapper, component: object) =>
  (w.findComponent(component as never).emitted('update') ?? []) as [string, unknown][]

describe('ReplyStepForm', () => {
  it('asks a rule that runs on its own who to send as, defaulting to its author', async () => {
    wrapper = mountWithProviders(ReplyStepForm, {
      config: { visibility: 'public', body: 'Hi' },
      automated: true,
      defaultSendAs: ANA.uuid,
      userRow,
    })
    const picker = wrapper.findComponent({ name: 'UserPickerStub' })
    expect(picker.props('type')).toBe('assignee')
    expect(picker.props('modelValue')).toBe(ANA.uuid)
    expect(picker.props('currentUser')).toEqual(ANA)

    picker.vm.$emit('update:modelValue', BEN.uuid)
    picker.vm.$emit('update:modelValue', '')
    expect(updates(wrapper, ReplyStepForm)).toEqual([
      ['send_as_uuid', BEN.uuid],
      ['send_as_uuid', null],
    ])
  })

  it('shows the agent the step names over the default', () => {
    wrapper = mountWithProviders(ReplyStepForm, {
      config: { body: 'Hi', send_as_uuid: BEN.uuid },
      automated: true,
      defaultSendAs: ANA.uuid,
      userRow,
    })
    expect(wrapper.findComponent({ name: 'UserPickerStub' }).props('modelValue')).toBe(BEN.uuid)
  })

  it('has no agent to pick on a manual rule, which replies as whoever applies it', () => {
    wrapper = mountWithProviders(ReplyStepForm, {
      config: { body: 'Hi' },
      automated: false,
      defaultSendAs: ANA.uuid,
    })
    expect(wrapper.find('[data-test="reply-send-as"]').exists()).toBe(false)
    expect(wrapper.findComponent({ name: 'UserPickerStub' }).exists()).toBe(false)
  })
})

describe('NotifyStepForm', () => {
  it('emails the requester with no person to pick', () => {
    wrapper = mountWithProviders(NotifyStepForm, {
      config: { recipient: 'requester', subject: 'Closing soon', body: 'Hi' },
      userRow,
    })
    expect(wrapper.findComponent({ name: 'UserPickerStub' }).exists()).toBe(false)
    expect((wrapper.find('[data-test="notify-subject"]').element as HTMLInputElement).value).toBe(
      'Closing soon',
    )
  })

  it('picks a person from the whole workspace', () => {
    wrapper = mountWithProviders(NotifyStepForm, {
      config: { recipient: 'user', user_uuid: BEN.uuid, subject: 's', body: 'b' },
      userRow,
    })
    const picker = wrapper.findComponent({ name: 'UserPickerStub' })
    expect(picker.props('type')).toBe('requester')
    expect(picker.props('currentUser')).toEqual(BEN)
    picker.vm.$emit('update:modelValue', ANA.uuid)
    expect(updates(wrapper, NotifyStepForm)).toEqual([['user_uuid', ANA.uuid]])
  })

  it('drops the person when it goes back to the requester', async () => {
    wrapper = mountWithProviders(NotifyStepForm, {
      config: { recipient: 'user', user_uuid: BEN.uuid, subject: 's', body: 'b' },
      userRow,
    })
    const requester = wrapper.findAll('[data-test="notify-recipient"] button').find((b) =>
      b.text().includes('admin-rule-editor-notify-to-requester'),
    )
    await requester!.trigger('click')
    expect(updates(wrapper, NotifyStepForm)).toEqual([
      ['recipient', 'requester'],
      ['user_uuid', undefined],
    ])
  })

  it('sends the subject and message as they are typed', async () => {
    wrapper = mountWithProviders(NotifyStepForm, { config: { recipient: 'requester' }, userRow })
    await wrapper.find('[data-test="notify-subject"]').setValue('Your request #12')
    await wrapper.find('[data-test="notify-body"]').setValue('We close it in 3 days.')
    expect(updates(wrapper, NotifyStepForm)).toEqual([
      ['subject', 'Your request #12'],
      ['body', 'We close it in 3 days.'],
    ])
  })
})

const TYPES = [
  {
    id: 1,
    slug: 'repair',
    label: null,
    outcomes: [
      { key: 'repaired', category: 'done', label: null },
      { key: 'beyond_repair', category: 'not_done', label: null },
    ],
    archived_at: null,
  },
  {
    id: 2,
    slug: 'calibration',
    label: 'Calibration',
    outcomes: [{ key: 'passed', category: 'done', label: 'In tolerance' }],
    archived_at: null,
  },
  { id: 3, slug: 'inspection', label: null, outcomes: [], archived_at: '2026-11-01T00:00:00Z' },
] as never[]

const dropdown = (w: VueWrapper, test: string) =>
  w.findAllComponents({ name: 'BaseDropdownStub' }).find((d) => d.attributes('data-test') === test)!
const optionValues = (w: VueWrapper, test: string) =>
  (dropdown(w, test).props('options') as { value: string; label: string }[]).map((o) => o.value)

describe('AssetRecordStepForm', () => {
  it('opens a record of a live type, with an optional summary', async () => {
    wrapper = mountWithProviders(AssetRecordStepForm, {
      kind: 'open_asset_record',
      config: { record_type: '' },
      types: TYPES,
    })
    expect(optionValues(wrapper, 'record-step-type')).toEqual(['repair', 'calibration'])
    expect(wrapper.find('[data-test="record-step-outcome"]').exists()).toBe(false)
    dropdown(wrapper, 'record-step-type').vm.$emit('update:modelValue', 'repair')
    await wrapper.find('[data-test="record-step-summary"]').setValue('Sent to the vendor')
    expect(updates(wrapper, AssetRecordStepForm)).toEqual([
      ['record_type', 'repair'],
      ['summary', 'Sent to the vendor'],
    ])
    expect(wrapper.text()).toContain('admin-rule-editor-records-open-hint')
  })

  it('keeps an archived type a step already names in its list', () => {
    wrapper = mountWithProviders(AssetRecordStepForm, {
      kind: 'open_asset_record',
      config: { record_type: 'inspection' },
      types: TYPES,
    })
    expect(optionValues(wrapper, 'record-step-type')).toEqual(['repair', 'calibration', 'inspection'])
  })

  it("offers the chosen type's outcomes, or every live type's with no type", async () => {
    wrapper = mountWithProviders(AssetRecordStepForm, {
      kind: 'complete_asset_records',
      config: { record_type: null, outcome: 'passed', create_if_none: false },
      types: TYPES,
    })
    expect(optionValues(wrapper, 'record-step-type')).toEqual(['', 'repair', 'calibration'])
    expect(dropdown(wrapper, 'record-step-outcome').props('options')).toEqual([
      { value: 'repaired', label: 'asset-record-outcome-repair-repaired' },
      { value: 'beyond_repair', label: 'asset-record-outcome-repair-beyond_repair' },
      { value: 'passed', label: 'In tolerance' },
    ])
    // With no type, a record can't be written where there is none.
    expect(wrapper.findComponent({ name: 'Checkbox' }).props('disabled')).toBe(true)

    // A type without the outcome clears it.
    dropdown(wrapper, 'record-step-type').vm.$emit('update:modelValue', 'repair')
    expect(updates(wrapper, AssetRecordStepForm)).toEqual([
      ['record_type', 'repair'],
      ['outcome', ''],
    ])
    await wrapper.setProps({ config: { record_type: 'repair', outcome: '', create_if_none: false } })
    expect(optionValues(wrapper, 'record-step-outcome')).toEqual(['repaired', 'beyond_repair'])
    expect(wrapper.findComponent({ name: 'Checkbox' }).props('disabled')).toBe(false)
  })

  it('turns off writing a record where there is none when the type goes', async () => {
    wrapper = mountWithProviders(AssetRecordStepForm, {
      kind: 'complete_asset_records',
      config: { record_type: 'repair', outcome: 'repaired', create_if_none: true },
      types: TYPES,
    })
    dropdown(wrapper, 'record-step-type').vm.$emit('update:modelValue', '')
    expect(updates(wrapper, AssetRecordStepForm)).toEqual([
      ['record_type', null],
      ['create_if_none', false],
    ])
  })
})

describe('AssetRecordStepForm with no record types', () => {
  it('points to where record types are added', () => {
    const router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/', component: { template: '<div />' } },
        { path: '/admin/asset-record-types', name: 'admin-asset-record-types', component: { template: '<div />' } },
      ],
    })
    wrapper = mountWithProviders(
      AssetRecordStepForm,
      { kind: 'open_asset_record', config: { record_type: '' }, types: [TYPES[2]] },
      {},
      [router],
    )
    const hint = wrapper.find('[data-test="record-step-no-types"]')
    expect(hint.text()).toContain('admin-rule-editor-records-no-types')
    expect(hint.find('a').attributes('href')).toBe('/admin/asset-record-types')
  })

  it('says nothing about it while the types load', () => {
    wrapper = mountWithProviders(AssetRecordStepForm, {
      kind: 'open_asset_record',
      config: { record_type: '' },
      types: [],
      loaded: false,
    })
    expect(wrapper.find('[data-test="record-step-no-types"]').exists()).toBe(false)
  })

  it('says nothing about it once there is a live type', () => {
    wrapper = mountWithProviders(AssetRecordStepForm, {
      kind: 'open_asset_record',
      config: { record_type: '' },
      types: TYPES,
    })
    expect(wrapper.find('[data-test="record-step-no-types"]').exists()).toBe(false)
  })
})

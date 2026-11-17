import { afterEach, describe, expect, it, vi } from 'vitest'
import { type VueWrapper } from '@vue/test-utils'
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

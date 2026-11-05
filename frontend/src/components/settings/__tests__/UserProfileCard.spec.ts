import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { User } from '@nosdesk/core/types/user'

const users = vi.hoisted(() => ({ updateUser: vi.fn() }))
vi.mock('@/services/userService', () => ({ default: users }))
// The voice recorder's encoder needs a Worker, which jsdom lacks.
vi.mock('extendable-media-recorder', () => ({ MediaRecorder: class {}, register: async () => {} }))
vi.mock('extendable-media-recorder-wav-encoder', () => ({ connect: async () => ({}) }))

vi.mock('vue-router', async (importOriginal) => {
  const actual = await importOriginal<typeof import('vue-router')>()
  const { defineComponent, h } = await import('vue')
  return { ...actual, RouterLink: defineComponent({ setup: (_, { slots }) => () => h('a', slots.default?.()) }) }
})

import UserProfileCard from '@/components/settings/UserProfileCard.vue'
import InlineEdit from '@/components/common/InlineEdit.vue'

const person = {
  uuid: 'person-1',
  name: 'QA person',
  email: 'person@example.test',
  role: 'user',
} as unknown as User

let wrapper: VueWrapper | null = null

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.clearAllMocks()
})

describe('UserProfileCard', () => {
  it('reports a saved name, so a page managing someone else shows it', async () => {
    users.updateUser.mockResolvedValue({ ...person, name: 'QA renamed' })
    wrapper = mountWithProviders(UserProfileCard, { user: person, canEdit: true, showEditableFields: true })
    const card = wrapper.findComponent(UserProfileCard)

    card.findComponent(InlineEdit).vm.$emit('update:modelValue', 'QA renamed')
    await flushPromises()

    expect(users.updateUser).toHaveBeenCalledWith('person-1', { name: 'QA renamed' })
    expect(card.emitted('updated')).toEqual([[{ name: 'QA renamed' }]])
  })

  it('shows a workspace owner as Owner and an admin as Administrator', () => {
    const owner = { ...person, workspace_role: 'owner' } as unknown as User
    wrapper = mountWithProviders(UserProfileCard, { user: owner })
    expect(wrapper.text()).toContain('settings-profile-role-owner')
    wrapper.unmount()

    const admin = { ...person, workspace_role: 'admin' } as unknown as User
    wrapper = mountWithProviders(UserProfileCard, { user: admin })
    expect(wrapper.text()).toContain('settings-profile-role-admin')
    expect(wrapper.text()).not.toContain('settings-profile-role-owner')
  })
})

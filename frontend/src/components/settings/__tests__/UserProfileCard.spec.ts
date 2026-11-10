import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import type { User } from '@nosdesk/core/types/user'

const users = vi.hoisted(() => ({ updateUser: vi.fn(), uploadImage: vi.fn() }))
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

  describe('picture upload', () => {
    /** Pick `file` with the avatar's file input. */
    async function pickAvatar(file: File) {
      wrapper = mountWithProviders(UserProfileCard, { user: person, canEdit: true, showEditableFields: true })
      const input = wrapper.find('[data-test="avatar-file"]')
      Object.defineProperty(input.element, 'files', { value: [file], configurable: true })
      await input.trigger('change')
      await flushPromises()
      return wrapper.findComponent(UserProfileCard)
    }
    const image = (bytes: number) => {
      const file = new File(['x'], 'me.png', { type: 'image/png' })
      Object.defineProperty(file, 'size', { value: bytes })
      return file
    }

    it('names the 10 MB limit when the server refuses the size', async () => {
      users.uploadImage.mockRejectedValue({
        response: { status: 413, data: { code: 'FILE_TOO_LARGE', file: 'me.png' } },
      })
      const card = await pickAvatar(image(1024))
      expect(users.uploadImage).toHaveBeenCalled()
      expect(card.emitted('error')).toEqual([['settings-profile-image-too-large']])
    })

    it('refuses a file over 10 MB without sending it', async () => {
      const card = await pickAvatar(image(12 * 1024 * 1024))
      expect(users.uploadImage).not.toHaveBeenCalled()
      expect(card.emitted('error')).toEqual([['settings-profile-image-too-large']])
    })

    it('says which image types work when the type is refused', async () => {
      users.uploadImage.mockRejectedValue({
        response: { status: 400, data: { code: 'FILE_TYPE_NOT_ALLOWED', file: 'me.png' } },
      })
      const card = await pickAvatar(image(1024))
      expect(card.emitted('error')).toEqual([['settings-profile-image-type-not-allowed']])
    })

    it('keeps the general message for any other failure', async () => {
      users.uploadImage.mockRejectedValue({ response: { status: 500, data: {} } })
      const card = await pickAvatar(image(1024))
      expect(card.emitted('error')).toEqual([['settings-profile-avatar-update-error']])
    })
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

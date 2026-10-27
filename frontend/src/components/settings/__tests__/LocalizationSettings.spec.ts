import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

const users = vi.hoisted(() => ({ updateUser: vi.fn() }))
vi.mock('@/services/userService', () => ({ default: users }))
// The voice recorder's encoder needs a Worker, which jsdom lacks.
vi.mock('extendable-media-recorder', () => ({ MediaRecorder: class {}, register: async () => {} }))
vi.mock('extendable-media-recorder-wav-encoder', () => ({ connect: async () => ({}) }))

import LocalizationSettings from '@/components/settings/LocalizationSettings.vue'
import BaseDropdown from '@/components/common/BaseDropdown.vue'
import Button from '@/components/common/Button.vue'

let wrapper: VueWrapper | null = null

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.clearAllMocks()
})

describe('LocalizationSettings', () => {
  it("reports someone else's saved language and timezone, so the page managing them keeps them", async () => {
    users.updateUser.mockResolvedValue({ uuid: 'person-1', locale: 'fr-FR', timezone: 'Europe/Paris' })
    wrapper = mountWithProviders(LocalizationSettings, {
      targetUserUuid: 'person-1',
      targetLocale: 'en-US',
      targetTimezone: 'Europe/Paris',
    })
    const card = wrapper.findComponent(LocalizationSettings)

    card.findComponent(BaseDropdown).vm.$emit('update:modelValue', 'fr-FR')
    await flushPromises()
    await card.findComponent(Button).trigger('click')
    await flushPromises()

    expect(users.updateUser).toHaveBeenCalledWith('person-1', { locale: 'fr-FR', timezone: 'Europe/Paris' })
    expect(card.emitted('updated')).toEqual([[{ locale: 'fr-FR', timezone: 'Europe/Paris' }]])
  })
})

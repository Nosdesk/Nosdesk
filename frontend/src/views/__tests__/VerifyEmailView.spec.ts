import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { mountWithProviders } from '@/test/mountWithProviders'

const users = vi.hoisted(() => ({ verifyEmailToken: vi.fn() }))
vi.mock('@/services/userService', () => ({ default: users }))

import VerifyEmailView from '@/views/VerifyEmailView.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.clearAllMocks()
})

async function open(path: string) {
  const stub = { template: '<div />' }
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', component: stub },
      { path: '/verify-email', component: VerifyEmailView },
    ],
  })
  await router.push(path)
  wrapper = mountWithProviders(VerifyEmailView, {}, {}, [router])
  await flushPromises()
}

describe('the address confirmation page', () => {
  it('confirms nothing on opening, so a mail scanner confirms nothing', async () => {
    await open('/verify-email?token=T0K')
    expect(users.verifyEmailToken).not.toHaveBeenCalled()
    expect(wrapper!.text()).toContain('verify-email-confirm-button')
  })

  it('confirms the address when the button is pressed', async () => {
    users.verifyEmailToken.mockResolvedValue({ email: 'sam@example.com' })
    await open('/verify-email?token=T0K')
    await wrapper!.get('button').trigger('click')
    await flushPromises()
    expect(users.verifyEmailToken).toHaveBeenCalledWith('T0K')
    expect(wrapper!.text()).toContain('verify-email-success-title')
  })
})

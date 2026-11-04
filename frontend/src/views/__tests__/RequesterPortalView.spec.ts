import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

// A requester who signed in at the native app holds no session in it: the
// screen opens the portal from the address their sign-in gave, and offers
// another account.
const openExternalUrl = vi.hoisted(() => vi.fn(async () => {}))
vi.mock('@/platform', () => ({ openExternalUrl }))
const route = vi.hoisted(() => ({ query: {} as Record<string, unknown> }))
const replace = vi.hoisted(() => vi.fn())
vi.mock('vue-router', async (orig) => ({
  ...(await orig<typeof import('vue-router')>()),
  useRoute: () => route,
  useRouter: () => ({ replace }),
}))
vi.mock('@/components/auth/AuthLayout.vue', () => ({ default: { template: '<div><slot /></div>' } }))

import RequesterPortalView from '@/views/RequesterPortalView.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  route.query = {}
  vi.clearAllMocks()
})

function button(label: string) {
  return wrapper!.findAll('button').find((b) => b.text() === label)
}

describe('a requester in the app', () => {
  it('opens the portal in the browser', async () => {
    route.query = { portal: 'https://help.example.com/portal/tickets' }
    wrapper = mountWithProviders(RequesterPortalView)
    await button('requester-portal-open')!.trigger('click')
    await flushPromises()
    expect(openExternalUrl).toHaveBeenCalledWith('https://help.example.com/portal/tickets')
  })

  it('says so when the portal could not be opened', async () => {
    route.query = { portal: 'https://help.example.com/portal/tickets' }
    openExternalUrl.mockRejectedValueOnce(new Error('no browser'))
    wrapper = mountWithProviders(RequesterPortalView)
    await button('requester-portal-open')!.trigger('click')
    await flushPromises()
    expect(wrapper.find('[role="alert"]').exists()).toBe(true)
  })

  it('offers no portal button without a web address, and goes back to sign in', async () => {
    route.query = { portal: 'javascript:alert(1)' }
    wrapper = mountWithProviders(RequesterPortalView)
    expect(button('requester-portal-open')).toBeUndefined()
    await button('requester-portal-another-account')!.trigger('click')
    expect(replace).toHaveBeenCalledWith({ name: 'login' })
  })
})

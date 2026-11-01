import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { PiniaColada } from '@pinia/colada'
import { createMemoryHistory, createRouter } from 'vue-router'
import { mountWithProviders } from '@/test/mountWithProviders'

const service = vi.hoisted(() => ({
  getMe: vi.fn(),
  getNotice: vi.fn(async () => ({ notice: null, following: false })),
  listMyApprovals: vi.fn(async () => []),
  followNotice: vi.fn(),
  signOut: vi.fn(async () => {}),
}))
vi.mock('@/portal/service', () => service)

import PortalLayout from '@/portal/components/PortalLayout.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
  vi.clearAllMocks()
})

async function mountAt(scope: unknown) {
  service.getMe.mockResolvedValue({ uuid: 'u', name: 'Pat', email: null, effective_locale: 'en-US', scope })
  const stub = { template: '<div />' }
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/tickets', component: stub },
      { path: '/tickets/new', component: stub },
      { path: '/tickets/:id', component: stub },
      { path: '/login', component: stub },
    ],
  })
  await router.push('/tickets/7')
  wrapper = mountWithProviders(PortalLayout, {}, {}, [router, [PiniaColada, {}] as never])
  await flushPromises()
  return { wrapper, router }
}

describe('a session opened from an email link', () => {
  it('offers sign-in instead of the request list and a new request', async () => {
    const { wrapper, router } = await mountAt({ kind: 'ticket', ticket_number: 7 })

    expect(wrapper.text()).not.toContain('portal-nav-requests')
    expect(wrapper.text()).not.toContain('portal-nav-new')
    const notice = wrapper.find('[data-testid="link-session-notice"]')
    expect(notice.text()).toContain('portal-link-session-notice')

    await notice.find('button').trigger('click')
    await flushPromises()
    expect(service.signOut).toHaveBeenCalled()
    expect(router.currentRoute.value.path).toBe('/login')
  })

  it('leaves a signed-in session as it was', async () => {
    const { wrapper } = await mountAt({ kind: 'full' })

    expect(wrapper.text()).toContain('portal-nav-requests')
    expect(wrapper.text()).toContain('portal-nav-new')
    expect(wrapper.find('[data-testid="link-session-notice"]').exists()).toBe(false)
  })
})

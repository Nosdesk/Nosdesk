import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import { mountWithProviders } from '@/test/mountWithProviders'

const service = vi.hoisted(() => ({ signInWithLink: vi.fn() }))
vi.mock('@/portal/service', () => service)

import SignInLinkView from '@/portal/views/SignInLinkView.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.clearAllMocks()
})

async function open(path: string): Promise<Router> {
  const stub = { template: '<div />' }
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/login', name: 'login', component: stub },
      { path: '/tickets', component: stub },
      { path: '/sign-in', name: 'sign-in-link', component: SignInLinkView },
    ],
  })
  await router.push(path)
  wrapper = mountWithProviders(SignInLinkView, {}, {}, [router])
  await flushPromises()
  return router
}

describe("a sign-in email's link", () => {
  it('signs nothing in on opening, so a mail scanner spends nothing', async () => {
    await open('/sign-in?token=T0K')
    expect(service.signInWithLink).not.toHaveBeenCalled()
    expect(wrapper!.text()).toContain('portal-sign-in-link-continue')
  })

  it('signs in with the token when Continue is pressed', async () => {
    service.signInWithLink.mockResolvedValue(undefined)
    const router = await open('/sign-in?token=T0K')
    await wrapper!.get('button').trigger('click')
    await flushPromises()
    expect(service.signInWithLink).toHaveBeenCalledWith('T0K')
    expect(router.currentRoute.value.path).toBe('/tickets')
  })

  it('sends a link that no longer works back to sign-in with the error', async () => {
    service.signInWithLink.mockRejectedValue(new Error('400'))
    const router = await open('/sign-in?token=used')
    await wrapper!.get('button').trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.name).toBe('login')
    expect(router.currentRoute.value.query.signin_error).toBe('1')
  })
})

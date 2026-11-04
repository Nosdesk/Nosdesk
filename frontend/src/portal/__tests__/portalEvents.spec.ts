import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { PiniaColada } from '@pinia/colada'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'
import { defineComponent, h } from 'vue'
import { mountWithProviders } from '@/test/mountWithProviders'
import { FakeBroadcastChannel } from '@/test/stepUpFakes'

const service = vi.hoisted(() => ({
  getMe: vi.fn(async () => ({ uuid: 'u', name: 'Pat', email: 'pat@example.com', effective_locale: 'en-US', scope: { kind: 'full' } })),
  getNotice: vi.fn(async () => ({ notice: null, following: false })),
  listMyApprovals: vi.fn(async () => []),
  followNotice: vi.fn(),
  signOut: vi.fn(async () => {}),
}))
vi.mock('@/portal/service', () => service)

import PortalLayout from '@/portal/components/PortalLayout.vue'
import { usePortalEvents } from '@/portal/usePortalEvents'

// jsdom has no EventSource. This one only records whether it was closed.
class FakeEventSource {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSED = 2
  static instances: FakeEventSource[] = []
  readyState = FakeEventSource.OPEN
  constructor(public url: string) {
    FakeEventSource.instances.push(this)
  }
  addEventListener() {}
  close() {
    this.readyState = FakeEventSource.CLOSED
  }
}

// The app root holds the stream and the layout offers sign-out, as in App.vue.
const Root = defineComponent({
  setup() {
    usePortalEvents()
    return () => h(PortalLayout)
  },
})

let wrapper: VueWrapper | null = null
beforeEach(() => {
  FakeEventSource.instances = []
  FakeBroadcastChannel.reset()
  vi.stubGlobal('EventSource', FakeEventSource)
  vi.stubGlobal('BroadcastChannel', FakeBroadcastChannel)
  // Without Web Locks the tab leads itself, so it connects straight away.
  vi.stubGlobal('navigator', { ...navigator, locks: undefined })
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
  vi.unstubAllGlobals()
  vi.clearAllMocks()
})

async function mount(): Promise<Router> {
  const stub = { template: '<div />' }
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/tickets', component: stub },
      { path: '/account', component: stub },
      { path: '/login', name: 'login', component: stub },
    ],
  })
  await router.push('/tickets')
  wrapper = mountWithProviders(Root, {}, {}, [router, [PiniaColada, {}] as never])
  await flushPromises()
  return router
}

async function signOut(): Promise<void> {
  await wrapper!.find('button[aria-label="portal-account-menu"]').trigger('click')
  await flushPromises()
  const row = [...document.body.querySelectorAll<HTMLElement>('button, [role="menuitem"]')].find(
    (el) => el.textContent?.trim() === 'portal-sign-out',
  )
  row!.click()
  await flushPromises()
}

describe('the portal live updates', () => {
  it('close before the sign-out request, and stay closed on the sign-in page', async () => {
    const router = await mount()
    expect(FakeEventSource.instances).toHaveLength(1)
    const stream = FakeEventSource.instances[0]!
    let openWhileSigningOut: boolean | null = null
    service.signOut.mockImplementationOnce(async () => {
      openWhileSigningOut = stream.readyState !== FakeEventSource.CLOSED
    })

    await signOut()

    expect(service.signOut).toHaveBeenCalled()
    expect(openWhileSigningOut).toBe(false)
    expect(router.currentRoute.value.path).toBe('/login')
    expect(FakeEventSource.instances).toHaveLength(1)
  })

  it('close in the tab that holds them when another tab signs out', async () => {
    await mount()
    await signOut()
    expect(FakeBroadcastChannel.instances[0]!.posted).toContainEqual({ kind: 'signed-out' })

    // A second tab's stream, closed by the first tab's sign-out.
    wrapper!.unmount()
    wrapper = null
    FakeEventSource.instances = []
    FakeBroadcastChannel.reset()
    await mount()
    FakeBroadcastChannel.deliver('nosdesk-portal-events', { kind: 'signed-out' })
    expect(FakeEventSource.instances[0]!.readyState).toBe(FakeEventSource.CLOSED)
  })

  it('close on the sign-in page and open again once signed back in', async () => {
    const router = await mount()
    await router.push('/account')
    expect(FakeEventSource.instances[0]!.readyState).toBe(FakeEventSource.OPEN)

    await router.push('/login')
    expect(FakeEventSource.instances[0]!.readyState).toBe(FakeEventSource.CLOSED)

    await router.push('/tickets')
    expect(FakeEventSource.instances).toHaveLength(2)
    expect(FakeEventSource.instances[1]!.readyState).toBe(FakeEventSource.OPEN)
  })
})

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter } from 'vue-router'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import { FakeBroadcastChannel } from '@/test/stepUpFakes'
import AuthConfirmedView from '@/views/auth/AuthConfirmedView.vue'
import routerSource from '@/router/index.ts?raw'

let wrapper: VueWrapper | null = null
let close: ReturnType<typeof vi.fn>

beforeEach(() => {
  FakeBroadcastChannel.reset()
  vi.stubGlobal('BroadcastChannel', FakeBroadcastChannel)
  close = vi.fn()
  vi.stubGlobal('close', close)
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.unstubAllGlobals()
})

async function visit(query: string) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/auth/confirmed', name: 'auth-confirmed', component: AuthConfirmedView }],
  })
  await router.push(`/auth/confirmed${query}`)
  await router.isReady()
  wrapper = mountWithProviders(AuthConfirmedView, {}, {}, [router])
  await flushPromises()
  return wrapper
}

describe('AuthConfirmedView', () => {
  it('tells the dialog it worked, tries to close, and says so', async () => {
    await visit('?result=ok')
    expect(FakeBroadcastChannel.instances).toHaveLength(1)
    const channel = FakeBroadcastChannel.instances[0]!
    expect(channel.name).toBe('nosdesk-step-up')
    expect(channel.posted).toEqual([{ type: 'step-up', result: 'ok' }])
    expect(channel.closed).toBe(true)
    expect(close).toHaveBeenCalled()
    expect(wrapper!.text()).toContain('step-up-popup-confirmed')
    expect(wrapper!.text()).not.toContain('step-up-popup-close')
  })

  it.each([
    ['cancelled', 'step-up-result-cancelled'],
    ['wrong_account', 'step-up-result-wrong-account-no-email'],
    ['not_fresh', 'step-up-result-not-fresh'],
    ['failed', 'step-up-result-failed'],
  ])('passes on %s and says the window can be closed', async (result, key) => {
    await visit(`?result=${result}`)
    expect(FakeBroadcastChannel.instances[0]!.posted).toEqual([{ type: 'step-up', result }])
    expect(wrapper!.text()).toContain(key)
    expect(wrapper!.text()).toContain('step-up-popup-close')
  })

  it('treats a missing or unknown result as a failure', async () => {
    await visit('?result=<script>')
    expect(FakeBroadcastChannel.instances[0]!.posted).toEqual([{ type: 'step-up', result: 'failed' }])
  })

  it('is a public route that renders without the app shell', () => {
    const record = routerSource.slice(routerSource.indexOf("path: '/auth/confirmed'"))
    const meta = record.slice(0, record.indexOf('}\n    },'))
    expect(meta).toContain("name: 'auth-confirmed'")
    expect(meta).toContain('requiresAuth: false')
    expect(meta).toContain("layout: 'blank'")
    // No session lookup or app runtime for the popup.
    expect(routerSource).toContain("if (to.name === 'auth-confirmed') return;")
  })
})

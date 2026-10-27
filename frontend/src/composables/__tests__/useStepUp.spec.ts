import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import { FakeBroadcastChannel, inMinutes, stepUpStatus } from '@/test/stepUpFakes'

const api = vi.hoisted(() => ({ status: vi.fn(), confirm: vi.fn() }))
vi.mock('@/services/stepUpService', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/services/stepUpService')>()
  return { ...actual, default: api, stepUpService: api }
})

const passkeys = vi.hoisted(() => ({ isSupported: vi.fn(() => true), confirmStepUp: vi.fn() }))
vi.mock('@/services/passkeyService', () => ({ default: passkeys, passkeyService: passkeys }))

import {
  useStepUp,
  STEP_UP_POLL_MS,
  STEP_UP_SLOW_AFTER_MS,
  STEP_UP_SLOW_POLL_MS,
  STEP_UP_WAIT_MS,
} from '@/composables/useStepUp'

type StepUp = ReturnType<typeof useStepUp>

let wrapper: VueWrapper | null = null
let stepUp: StepUp

function mountStepUp() {
  const Host = defineComponent({
    setup() {
      stepUp = useStepUp()
      return () => h('div')
    },
  })
  wrapper = mountWithProviders(Host, {}, {}, [PiniaColada])
}

beforeEach(() => {
  FakeBroadcastChannel.reset()
  vi.stubGlobal('BroadcastChannel', FakeBroadcastChannel)
  passkeys.isSupported.mockReturnValue(true)
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.useRealTimers()
  vi.unstubAllGlobals()
  vi.clearAllMocks()
})

describe('useStepUp status', () => {
  it('is loading until the status arrives, then maps it', async () => {
    let resolve!: (v: unknown) => void
    api.status.mockReturnValue(new Promise((r) => (resolve = r)))
    mountStepUp()
    expect(stepUp.loading.value).toBe(true)

    resolve(
      stepUpStatus({
        methods: ['provider', 'password'],
        provider: { provider_type: 'oidc', name: 'Nosdesk' },
        email: 'admin@acme.test',
        window_minutes: 15,
      }),
    )
    await flushPromises()
    expect(stepUp.loading.value).toBe(false)
    expect(stepUp.confirmed.value).toBe(false)
    expect(stepUp.methods.value).toEqual(['provider', 'password'])
    expect(stepUp.provider.value).toEqual({ provider_type: 'oidc', name: 'Nosdesk' })
    expect(stepUp.email.value).toBe('admin@acme.test')
    expect(stepUp.windowMinutes.value).toBe(15)
  })

  it('leaves out passkeys when the browser has none', async () => {
    passkeys.isSupported.mockReturnValue(false)
    api.status.mockResolvedValue(stepUpStatus({ methods: ['passkey', 'password'] }))
    mountStepUp()
    await flushPromises()
    expect(stepUp.methods.value).toEqual(['password'])
  })

  it('flips back to unconfirmed when confirmed_until passes', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'Date'] })
    api.status.mockResolvedValue(stepUpStatus({ confirmed_until: inMinutes(1) }))
    mountStepUp()
    await flushPromises()
    expect(stepUp.confirmed.value).toBe(true)

    vi.advanceTimersByTime(59_000)
    expect(stepUp.confirmed.value).toBe(true)
    vi.advanceTimersByTime(2_000)
    expect(stepUp.confirmed.value).toBe(false)
  })

  it('reports a status that failed to load', async () => {
    api.status.mockRejectedValue(new Error('offline'))
    mountStepUp()
    await flushPromises()
    expect(stepUp.loading.value).toBe(false)
    expect(stepUp.loadFailed.value).toBe(true)
  })
})

describe('useStepUp credentials', () => {
  it('confirms with a password and becomes confirmed', async () => {
    api.status.mockResolvedValue(stepUpStatus())
    api.confirm.mockResolvedValue({ confirmed_until: inMinutes(10) })
    mountStepUp()
    await flushPromises()

    expect(await stepUp.confirmWithCredential({ password: 'hunter2' })).toBe(true)
    expect(api.confirm).toHaveBeenCalledWith({ password: 'hunter2' })
    expect(stepUp.confirmed.value).toBe(true)
    expect(stepUp.error.value).toBeNull()
  })

  it("keeps the server's reason when the credential is wrong", async () => {
    api.status.mockResolvedValue(stepUpStatus())
    api.confirm.mockRejectedValue({ response: { status: 400, data: { message: "That isn't right. Try again." } } })
    mountStepUp()
    await flushPromises()

    expect(await stepUp.confirmWithCredential({ mfa_code: '123456' })).toBe(false)
    expect(stepUp.error.value).toBe("That isn't right. Try again.")
    expect(stepUp.confirmed.value).toBe(false)
  })

  it('says nothing when the passkey sheet is dismissed', async () => {
    api.status.mockResolvedValue(stepUpStatus({ methods: ['passkey'] }))
    passkeys.confirmStepUp.mockRejectedValue(Object.assign(new Error('dismissed'), { name: 'NotAllowedError' }))
    mountStepUp()
    await flushPromises()

    expect(await stepUp.confirmWithPasskey()).toBe(false)
    expect(stepUp.error.value).toBeNull()
  })

  it('confirms with a passkey', async () => {
    api.status.mockResolvedValue(stepUpStatus({ methods: ['passkey'] }))
    passkeys.confirmStepUp.mockResolvedValue({ confirmed_until: inMinutes(10) })
    mountStepUp()
    await flushPromises()

    expect(await stepUp.confirmWithPasskey()).toBe(true)
    expect(stepUp.confirmed.value).toBe(true)
  })
})

describe('useStepUp provider popup', () => {
  const provider = { provider_type: 'oidc' as const, name: 'Nosdesk' }

  async function openPopup() {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'Date'] })
    const open = vi.fn(() => null)
    vi.stubGlobal('open', open)
    api.status.mockResolvedValue(stepUpStatus({ methods: ['provider'], provider }))
    mountStepUp()
    await flushPromises()
    stepUp.openProviderPopup()
    return open
  }

  it('opens the provider page in a named popup, synchronously', async () => {
    const open = await openPopup()
    expect(open).toHaveBeenCalledWith('/api/auth/step-up/provider', 'nosdesk-step-up', 'popup,width=520,height=720')
    expect(stepUp.waiting.value).toBe(true)
    expect(FakeBroadcastChannel.instances.map((c) => c.name)).toEqual(['nosdesk-step-up'])
  })

  it('shows a result the popup reports, and stops listening', async () => {
    await openPopup()
    FakeBroadcastChannel.deliver('nosdesk-step-up', { type: 'step-up', result: 'wrong_account' })
    await flushPromises()
    expect(stepUp.waiting.value).toBe(false)
    expect(stepUp.popupResult.value).toBe('wrong_account')
    expect(FakeBroadcastChannel.instances[0]!.closed).toBe(true)
  })

  it('becomes confirmed when the popup reports ok', async () => {
    await openPopup()
    api.status.mockResolvedValue(stepUpStatus({ methods: ['provider'], provider, confirmed_until: inMinutes(10) }))
    FakeBroadcastChannel.deliver('nosdesk-step-up', { type: 'step-up', result: 'ok' })
    await flushPromises()
    expect(stepUp.confirmed.value).toBe(true)
    expect(stepUp.waiting.value).toBe(false)
    expect(stepUp.popupResult.value).toBeNull()
  })

  it('ignores messages that are not step-up results', async () => {
    await openPopup()
    FakeBroadcastChannel.deliver('nosdesk-step-up', { type: 'other', result: 'ok' })
    FakeBroadcastChannel.deliver('nosdesk-step-up', { type: 'step-up', result: 'bogus' })
    await flushPromises()
    expect(stepUp.waiting.value).toBe(true)
  })

  it('notices a confirmation by polling, then stops polling', async () => {
    await openPopup()
    const callsBefore = api.status.mock.calls.length
    api.status.mockResolvedValue(stepUpStatus({ methods: ['provider'], provider, confirmed_until: inMinutes(10) }))

    await vi.advanceTimersByTimeAsync(STEP_UP_POLL_MS)
    await flushPromises()
    expect(api.status.mock.calls.length).toBeGreaterThan(callsBefore)
    expect(stepUp.confirmed.value).toBe(true)
    expect(stepUp.waiting.value).toBe(false)

    const callsAfter = api.status.mock.calls.length
    await vi.advanceTimersByTimeAsync(STEP_UP_POLL_MS * 3)
    expect(api.status.mock.calls.length).toBe(callsAfter)
  })

  it('re-checks when the window regains focus', async () => {
    await openPopup()
    api.status.mockResolvedValue(stepUpStatus({ methods: ['provider'], provider, confirmed_until: inMinutes(10) }))
    window.dispatchEvent(new Event('focus'))
    await flushPromises()
    expect(stepUp.confirmed.value).toBe(true)
    expect(stepUp.waiting.value).toBe(false)
  })

  it('still takes an answer from a popup open longer than five minutes', async () => {
    await openPopup()
    await vi.advanceTimersByTimeAsync(STEP_UP_SLOW_AFTER_MS + STEP_UP_POLL_MS)
    expect(stepUp.waiting.value).toBe(true)
    FakeBroadcastChannel.deliver('nosdesk-step-up', { type: 'step-up', result: 'wrong_account' })
    await flushPromises()
    expect(stepUp.popupResult.value).toBe('wrong_account')
  })

  it('asks less often once the popup has been open a while', async () => {
    await openPopup()
    await vi.advanceTimersByTimeAsync(STEP_UP_SLOW_AFTER_MS + STEP_UP_POLL_MS)
    const calls = api.status.mock.calls.length
    // The last quick poll ran at five minutes; the next is a slow poll later.
    await vi.advanceTimersByTimeAsync(STEP_UP_SLOW_POLL_MS - STEP_UP_POLL_MS - 1)
    expect(api.status.mock.calls.length).toBe(calls)
    await vi.advanceTimersByTimeAsync(1)
    expect(api.status.mock.calls.length).toBe(calls + 1)
  })

  it('gives up with a message once the sign-in can no longer finish', async () => {
    await openPopup()
    await vi.advanceTimersByTimeAsync(STEP_UP_WAIT_MS)
    expect(stepUp.waiting.value).toBe(false)
    expect(stepUp.popupResult.value).toBe('failed')
    expect(FakeBroadcastChannel.instances[0]!.closed).toBe(true)
  })

  it('stops everything on unmount', async () => {
    await openPopup()
    wrapper!.unmount()
    wrapper = null
    const calls = api.status.mock.calls.length
    await vi.advanceTimersByTimeAsync(STEP_UP_POLL_MS * 3)
    expect(api.status.mock.calls.length).toBe(calls)
    expect(FakeBroadcastChannel.instances[0]!.closed).toBe(true)
  })
})

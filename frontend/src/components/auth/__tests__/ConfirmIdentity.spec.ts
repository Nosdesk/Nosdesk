import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, h, ref } from 'vue'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import { FakeBroadcastChannel, inMinutes, stepUpStatus } from '@/test/stepUpFakes'
import type { StepUpStatus } from '@/services/stepUpService'

const api = vi.hoisted(() => ({ status: vi.fn(), confirm: vi.fn() }))
vi.mock('@/services/stepUpService', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/services/stepUpService')>()
  return { ...actual, default: api, stepUpService: api }
})

const passkeys = vi.hoisted(() => ({ isSupported: vi.fn(() => true), confirmStepUp: vi.fn() }))
vi.mock('@/services/passkeyService', () => ({ default: passkeys, passkeyService: passkeys }))

import ConfirmIdentity from '@/components/auth/ConfirmIdentity.vue'

type Exposed = { confirm: () => Promise<boolean> }

let wrapper: VueWrapper | null = null
const ready = ref<boolean | null>(null)
const submits = ref(0)
let identity: { value: Exposed | null }

async function mountWith(status: StepUpStatus | Promise<StepUpStatus>) {
  api.status.mockReturnValue(Promise.resolve(status))
  ready.value = null
  submits.value = 0
  const Host = defineComponent({
    setup() {
      const inner = ref<Exposed | null>(null)
      identity = inner
      return () =>
        h(ConfirmIdentity, {
          ref: inner,
          'onUpdate:ready': (v: boolean) => (ready.value = v),
          onSubmit: () => submits.value++,
        })
    },
  })
  wrapper = mountWithProviders(Host, {}, {}, [PiniaColada])
  await flushPromises()
  return wrapper
}

const text = () => wrapper!.text()
const buttonByText = (label: string) => wrapper!.findAll('button').find((b) => b.text().includes(label))

beforeEach(() => {
  FakeBroadcastChannel.reset()
  vi.stubGlobal('BroadcastChannel', FakeBroadcastChannel)
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.unstubAllGlobals()
  vi.clearAllMocks()
})

describe('ConfirmIdentity', () => {
  it('is not ready while the status loads, and says what it needs', async () => {
    api.status.mockReturnValue(new Promise(() => {}))
    const Host = defineComponent({
      setup: () => () => h(ConfirmIdentity, { 'onUpdate:ready': (v: boolean) => (ready.value = v) }),
    })
    wrapper = mountWithProviders(Host, {}, {}, [PiniaColada])
    await flushPromises()
    expect(ready.value).toBe(false)
    expect(text()).toContain('step-up-lead')
    expect(wrapper.find('input').exists()).toBe(false)
  })

  it('asks for the password first, and is ready once one is typed', async () => {
    await mountWith(stepUpStatus({ methods: ['password', 'code'] }))
    const input = wrapper!.get('input')
    expect(input.attributes('autocomplete')).toBe('current-password')
    expect(input.attributes('type')).toBe('password')
    expect(text()).toContain('step-up-password-label')
    expect(ready.value).toBe(false)

    await input.setValue('hunter2')
    expect(ready.value).toBe(true)

    await input.trigger('keyup', { key: 'Enter' })
    expect(submits.value).toBe(1)
  })

  it('sends the typed password on confirm', async () => {
    api.confirm.mockResolvedValue({ confirmed_until: inMinutes(10) })
    await mountWith(stepUpStatus())
    await wrapper!.get('input').setValue('hunter2')

    expect(await identity.value!.confirm()).toBe(true)
    expect(api.confirm).toHaveBeenCalledWith({ password: 'hunter2' })
    await flushPromises()
    expect(text()).toContain('step-up-confirmed')
  })

  it("shows the server's reason inline when the password is wrong", async () => {
    api.confirm.mockRejectedValue({ response: { status: 400, data: { message: "That isn't right. Try again." } } })
    await mountWith(stepUpStatus())
    await wrapper!.get('input').setValue('nope')

    expect(await identity.value!.confirm()).toBe(false)
    await flushPromises()
    expect(wrapper!.get('[role="alert"]').text()).toBe("That isn't right. Try again.")
  })

  it('asks for a code with a numeric keypad', async () => {
    api.confirm.mockResolvedValue({ confirmed_until: inMinutes(10) })
    await mountWith(stepUpStatus({ methods: ['code'] }))
    const input = wrapper!.get('input')
    expect(input.attributes('inputmode')).toBe('numeric')
    expect(input.attributes('autocomplete')).toBe('one-time-code')
    expect(text()).toContain('step-up-code-label')

    await input.setValue(' 123456 ')
    expect(await identity.value!.confirm()).toBe(true)
    expect(api.confirm).toHaveBeenCalledWith({ mfa_code: '123456' })
  })

  it('shows only a quiet line when already confirmed', async () => {
    await mountWith(stepUpStatus({ confirmed_until: inMinutes(5) }))
    expect(text()).toContain('step-up-confirmed')
    expect(text()).not.toContain('step-up-lead')
    expect(wrapper!.find('input').exists()).toBe(false)
    expect(ready.value).toBe(true)
    expect(await identity.value!.confirm()).toBe(true)
    expect(api.confirm).not.toHaveBeenCalled()
  })

  it('says how to get a method back when there is none', async () => {
    await mountWith(stepUpStatus({ methods: [] }))
    expect(text()).toContain('step-up-no-methods')
    expect(ready.value).toBe(false)
    expect(await identity.value!.confirm()).toBe(false)
  })

  it('offers the other methods behind "Use another way"', async () => {
    await mountWith(stepUpStatus({ methods: ['password', 'code'] }))
    expect(text()).not.toContain('step-up-choice-code')
    await buttonByText('step-up-use-another-way')!.trigger('click')
    await buttonByText('step-up-choice-code')!.trigger('click')
    expect(wrapper!.get('input').attributes('inputmode')).toBe('numeric')
    // And the password is now the other way.
    await buttonByText('step-up-use-another-way')!.trigger('click')
    expect(buttonByText('step-up-choice-password')).toBeTruthy()
  })

  it('confirms with a passkey on its own button', async () => {
    passkeys.confirmStepUp.mockResolvedValue({ confirmed_until: inMinutes(10) })
    await mountWith(stepUpStatus({ methods: ['passkey'] }))
    expect(ready.value).toBe(false)
    await buttonByText('step-up-passkey-button')!.trigger('click')
    await flushPromises()
    expect(passkeys.confirmStepUp).toHaveBeenCalled()
    expect(ready.value).toBe(true)
    expect(text()).toContain('step-up-confirmed')
  })

  describe('provider sign-in', () => {
    const provider = { provider_type: 'oidc' as const, name: 'Nosdesk' }
    let open: ReturnType<typeof vi.fn>

    beforeEach(() => {
      open = vi.fn(() => null)
      vi.stubGlobal('open', open)
    })

    it('opens the popup from the button and waits', async () => {
      await mountWith(stepUpStatus({ methods: ['provider'], provider }))
      expect(text()).toContain('step-up-provider-help')
      await buttonByText('step-up-provider-button')!.trigger('click')
      expect(open).toHaveBeenCalledTimes(1)
      expect(text()).toContain('step-up-waiting')

      // "Try again" opens it again.
      await buttonByText('step-up-try-again')!.trigger('click')
      expect(open).toHaveBeenCalledTimes(2)
    })

    it.each([
      ['wrong_account', 'step-up-result-wrong-account'],
      ['cancelled', 'step-up-result-cancelled'],
      ['not_fresh', 'step-up-result-not-fresh'],
      ['failed', 'step-up-result-failed'],
    ])('shows %s from the popup', async (result, key) => {
      await mountWith(stepUpStatus({ methods: ['provider'], provider }))
      await buttonByText('step-up-provider-button')!.trigger('click')
      FakeBroadcastChannel.deliver('nosdesk-step-up', { type: 'step-up', result })
      await flushPromises()
      expect(wrapper!.get('[role="alert"]').text()).toBe(key)
      expect(text()).not.toContain('step-up-waiting')
      // Back to the button, to try again.
      expect(buttonByText('step-up-provider-button')).toBeTruthy()
    })

    it('becomes confirmed when the popup reports ok', async () => {
      await mountWith(stepUpStatus({ methods: ['provider'], provider }))
      await buttonByText('step-up-provider-button')!.trigger('click')
      api.status.mockResolvedValue(stepUpStatus({ methods: ['provider'], provider, confirmed_until: inMinutes(10) }))
      FakeBroadcastChannel.deliver('nosdesk-step-up', { type: 'step-up', result: 'ok' })
      await flushPromises()
      expect(text()).toContain('step-up-confirmed')
      expect(ready.value).toBe(true)
    })
  })
})

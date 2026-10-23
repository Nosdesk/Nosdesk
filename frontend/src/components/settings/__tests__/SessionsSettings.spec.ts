import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'
import { FakeBroadcastChannel, inMinutes, stepUpStatus } from '@/test/stepUpFakes'

const auth = vi.hoisted(() => ({
  getSessions: vi.fn(),
  revokeAllOtherSessions: vi.fn(),
  revokeSession: vi.fn(),
}))
vi.mock('@nosdesk/core/services/authService', () => ({ default: auth }))

const stepUpApi = vi.hoisted(() => ({ status: vi.fn(), confirm: vi.fn() }))
vi.mock('@/services/stepUpService', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/services/stepUpService')>()
  return { ...actual, default: stepUpApi, stepUpService: stepUpApi }
})

// The modal's own behaviour is covered elsewhere; this stands in for its
// body and its confirm button.
vi.mock('@/components/common/ConfirmModal.vue', async () => {
  const { defineComponent, h } = await import('vue')
  return {
    default: defineComponent({
      props: { show: Boolean, confirmDisabled: Boolean },
      emits: ['confirm', 'close'],
      setup(props, { emit, slots }) {
        return () =>
          props.show
            ? h('div', { 'data-test': 'modal' }, [
                slots.body?.(),
                h('button', {
                  'data-test': 'confirm',
                  disabled: props.confirmDisabled,
                  onClick: () => emit('confirm'),
                }),
              ])
            : null
      },
    }),
  }
})

import SessionsSettings from '@/components/settings/SessionsSettings.vue'

const session = (id: string, current: boolean) => ({
  session_id: id,
  device_name: null,
  ip_address: null,
  user_agent: null,
  location: null,
  created_at: '2026-10-22T00:00:00Z',
  last_active: '2026-10-22T00:00:00Z',
  expires_at: '2026-11-22T00:00:00Z',
  is_current: current,
})

let wrapper: VueWrapper | null = null
let successes: string[] = []

beforeEach(() => {
  FakeBroadcastChannel.reset()
  vi.stubGlobal('BroadcastChannel', FakeBroadcastChannel)
  auth.getSessions.mockResolvedValue([session('here', true), session('there', false)])
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.unstubAllGlobals()
  vi.clearAllMocks()
})

async function openSignOutOthers() {
  successes = []
  wrapper = mountWithProviders(
    SessionsSettings,
    { onSuccess: (m: string) => successes.push(m) },
    {},
    [PiniaColada],
  )
  await flushPromises()
  const open = wrapper.findAll('button').find((b) => b.text() === 'settings-sessions-revoke-others-button')
  await open!.trigger('click')
  await flushPromises()
  return wrapper
}

const confirmButton = () => wrapper!.get('[data-test="confirm"]')

async function signOutOthers(result: Record<string, unknown>) {
  stepUpApi.status.mockResolvedValue(stepUpStatus({ confirmed_until: inMinutes(5) }))
  auth.revokeAllOtherSessions.mockResolvedValue(result)
  await openSignOutOthers()
  await confirmButton().trigger('click')
  await flushPromises()
  return successes
}

describe('SessionsSettings sign out all other sessions', () => {
  it('confirms plainly when the other sessions are all ended', async () => {
    expect(await signOutOthers({ revoked_count: 1, remembered_sign_ins_ended: true })).toEqual([
      'settings-sessions-revoke-others-success',
    ])
    // Self-hosted: no remembered sign-ins to speak of.
    wrapper!.unmount()
    wrapper = null
    expect(await signOutOthers({ revoked_count: 1 })).toEqual(['settings-sessions-revoke-others-success'])
  })

  it('says other browsers may sign back in when the Nosdesk account sign-ins could not be ended', async () => {
    expect(await signOutOthers({ revoked_count: 1, remembered_sign_ins_ended: false })).toEqual([
      'settings-sessions-revoke-others-success-remembered-kept',
    ])
  })
})

describe('SessionsSettings confirms it is you first', () => {
  it('keeps the button off until the status has loaded', async () => {
    let resolve!: (v: unknown) => void
    stepUpApi.status.mockReturnValue(new Promise((r) => (resolve = r)))
    await openSignOutOthers()
    expect(confirmButton().attributes('disabled')).toBeDefined()

    resolve(stepUpStatus({ confirmed_until: inMinutes(5) }))
    await flushPromises()
    expect(confirmButton().attributes('disabled')).toBeUndefined()
  })

  it('keeps the button off for a session with no way to confirm', async () => {
    stepUpApi.status.mockResolvedValue(stepUpStatus({ methods: [] }))
    await openSignOutOthers()
    expect(confirmButton().attributes('disabled')).toBeDefined()
    expect(wrapper!.text()).toContain('step-up-no-methods')
  })

  it('sends the password first, then signs out the others without a credential', async () => {
    stepUpApi.status.mockResolvedValue(stepUpStatus())
    stepUpApi.confirm.mockResolvedValue({ confirmed_until: inMinutes(10) })
    auth.revokeAllOtherSessions.mockResolvedValue({ revoked_count: 1 })
    await openSignOutOthers()
    expect(confirmButton().attributes('disabled')).toBeDefined()

    await wrapper!.get('input[type="password"]').setValue('hunter2')
    expect(confirmButton().attributes('disabled')).toBeUndefined()
    await confirmButton().trigger('click')
    await flushPromises()

    expect(stepUpApi.confirm).toHaveBeenCalledWith({ password: 'hunter2' })
    expect(auth.revokeAllOtherSessions).toHaveBeenCalledWith()
    expect(stepUpApi.confirm.mock.invocationCallOrder[0]).toBeLessThan(
      auth.revokeAllOtherSessions.mock.invocationCallOrder[0]!,
    )
    expect(successes).toEqual(['settings-sessions-revoke-others-success'])
  })

  it('shows a wrong password inline and keeps the dialog open', async () => {
    stepUpApi.status.mockResolvedValue(stepUpStatus())
    stepUpApi.confirm.mockRejectedValue({ response: { status: 400, data: { message: "That isn't right. Try again." } } })
    await openSignOutOthers()

    await wrapper!.get('input[type="password"]').setValue('nope')
    await confirmButton().trigger('click')
    await flushPromises()

    expect(auth.revokeAllOtherSessions).not.toHaveBeenCalled()
    expect(wrapper!.find('[data-test="modal"]').exists()).toBe(true)
    expect(wrapper!.text()).toContain("That isn't right. Try again.")
  })

  it('shows a failed sign-out inline and re-checks the confirmation', async () => {
    stepUpApi.status.mockResolvedValue(stepUpStatus({ confirmed_until: inMinutes(5) }))
    auth.revokeAllOtherSessions.mockRejectedValue({ response: { status: 403, data: { message: 'Confirm it is you first.' } } })
    await openSignOutOthers()
    const statusCalls = stepUpApi.status.mock.calls.length

    await confirmButton().trigger('click')
    await flushPromises()

    expect(wrapper!.find('[data-test="modal"]').exists()).toBe(true)
    expect(wrapper!.text()).toContain('Confirm it is you first.')
    expect(stepUpApi.status.mock.calls.length).toBeGreaterThan(statusCalls)
  })
})

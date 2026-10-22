import { afterEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

const auth = vi.hoisted(() => ({
  getSessions: vi.fn(),
  revokeAllOtherSessions: vi.fn(),
  revokeSession: vi.fn(),
  getUserAuthIdentities: vi.fn(async () => []),
  getMFAStatus: vi.fn(async () => ({ enabled: false })),
}))
vi.mock('@nosdesk/core/services/authService', () => ({ default: auth }))

// The modal's own behaviour is covered elsewhere; this stands in for its
// confirm button.
vi.mock('@/components/common/ConfirmModal.vue', async () => {
  const { defineComponent, h } = await import('vue')
  return {
    default: defineComponent({
      props: { show: Boolean },
      emits: ['confirm', 'close'],
      setup(props, { emit }) {
        return () =>
          props.show ? h('button', { 'data-test': 'confirm', onClick: () => emit('confirm') }) : null
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
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.clearAllMocks()
})

async function signOutOthers(result: Record<string, unknown>) {
  auth.getSessions.mockResolvedValue([session('here', true), session('there', false)])
  auth.revokeAllOtherSessions.mockResolvedValue(result)
  const successes: string[] = []
  wrapper = mountWithProviders(
    SessionsSettings,
    { onSuccess: (m: string) => successes.push(m) },
    {},
    [PiniaColada],
  )
  await flushPromises()
  const open = wrapper.findAll('button').find((b) => b.text() === 'settings-sessions-revoke-others-button')
  await open!.trigger('click')
  await wrapper.get('[data-test="confirm"]').trigger('click')
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

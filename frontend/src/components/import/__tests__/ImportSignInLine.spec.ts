import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import type { SignInStatus } from '@nosdesk/core/services/importService'

import enUS from '../../../../../i18n/locales/en-US/main.ftl?raw'

const api = vi.hoisted(() => ({ sendInvites: vi.fn() }))
vi.mock('@nosdesk/core/services/importService', () => ({
  importService: { sendInvites: (...a: unknown[]) => api.sendInvites(...a) },
}))

import ImportSignInLine from '@/components/import/ImportSignInLine.vue'

/** The real en-US catalogue, so the sentences are checked as written. */
function mountLine(status: SignInStatus) {
  const bundle = new FluentBundle('en-US', { useIsolating: false })
  bundle.addResource(new FluentResource(enUS))
  return mount(ImportSignInLine, {
    props: { jobId: 'job-1', status },
    global: { plugins: [createFluentVue({ bundles: [bundle] })] },
  })
}

const invite = (overrides: Partial<SignInStatus> = {}): SignInStatus => ({
  pending: 7,
  invited: 0,
  method: 'invite',
  email_ready: true,
  ...overrides,
})

beforeEach(() => api.sendInvites.mockReset())

describe('ImportSignInLine', () => {
  it('says who can not sign in yet and offers to send invites', () => {
    const w = mountLine(invite())
    expect(w.find('[data-test="import-sign-in-pending"]').text()).toBe(
      "These 7 people can't sign in yet.",
    )
    expect(w.find('[data-test="import-sign-in-send"]').text()).toBe('Send invites')
    expect(w.find('[data-test="import-sign-in-sso"]').exists()).toBe(false)
  })

  it('words one person in the singular', () => {
    const w = mountLine(invite({ pending: 1 }))
    expect(w.find('[data-test="import-sign-in-pending"]').text()).toBe(
      "This person can't sign in yet.",
    )
    expect(w.find('[data-test="import-sign-in-send"]').text()).toBe('Send invite')
  })

  it('sends the invites and shows the line as it stands', async () => {
    api.sendInvites.mockResolvedValue({ sent: 7, failed: 0, sign_in: invite({ invited: 7 }) })
    const w = mountLine(invite())
    await w.find('[data-test="import-sign-in-send"]').trigger('click')
    await flushPromises()
    expect(api.sendInvites).toHaveBeenCalledWith('job-1')
    const updated = w.emitted('updated')?.[0]?.[0] as SignInStatus
    expect(updated.invited).toBe(7)
    await w.setProps({ status: updated })
    expect(w.find('[data-test="import-sign-in-send"]').exists()).toBe(false)
    expect(w.find('[data-test="import-sign-in-sent"]').text()).toBe('Invites sent to 7 people.')
  })

  it('offers the rest when some have an invite already', () => {
    const w = mountLine(invite({ invited: 4 }))
    expect(w.find('[data-test="import-sign-in-send"]').text()).toBe('Send invites')
  })

  it('says how many were not sent, or that sending failed', async () => {
    api.sendInvites.mockResolvedValueOnce({ sent: 5, failed: 2, sign_in: invite({ invited: 5 }) })
    const w = mountLine(invite())
    await w.find('[data-test="import-sign-in-send"]').trigger('click')
    await flushPromises()
    expect(w.find('[data-test="import-sign-in-failed"]').text()).toBe(
      "2 invites weren't sent. Try again.",
    )
    api.sendInvites.mockRejectedValueOnce(new Error('offline'))
    await w.find('[data-test="import-sign-in-send"]').trigger('click')
    await flushPromises()
    expect(w.find('[data-test="import-sign-in-failed"]').text()).toBe(
      "Couldn't send the invites. Try again.",
    )
  })

  it('says email is not set up instead of offering invites', () => {
    const w = mountLine(invite({ email_ready: false }))
    expect(w.find('[data-test="import-sign-in-send"]').exists()).toBe(false)
    expect(w.find('[data-test="import-sign-in-email-off"]').text()).toBe(
      "Email isn't set up, so invites can't be sent.",
    )
  })

  it('names the sign-in provider instead of invites', () => {
    const w = mountLine({
      pending: 7,
      invited: 0,
      method: 'sso',
      provider: 'Microsoft Entra ID',
      email_ready: false,
    })
    expect(w.find('[data-test="import-sign-in-pending"]').text()).toBe(
      "These 7 people can't sign in yet.",
    )
    expect(w.find('[data-test="import-sign-in-sso"]').text()).toBe(
      'They sign in with Microsoft Entra ID once their accounts exist there.',
    )
    expect(w.find('[data-test="import-sign-in-send"]').exists()).toBe(false)
  })
})

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { PiniaColada } from '@pinia/colada'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { Plugin } from 'vue'
import { mountWithProviders } from '@/test/mountWithProviders'
import { FakeBroadcastChannel, stepUpStatus } from '@/test/stepUpFakes'
import { ACCOUNT_API } from '@/composables/useAccountApi'
import type { AccountApi } from '@/services/accountApi'

const service = vi.hoisted(() => ({
  getMe: vi.fn(),
  getAccount: vi.fn(),
  updateAccount: vi.fn(async () => {}),
  getNotice: vi.fn(async () => ({ notice: null, following: false })),
  listMyApprovals: vi.fn(async () => []),
  followNotice: vi.fn(),
  signOut: vi.fn(async () => {}),
}))
vi.mock('@/portal/service', () => service)

// The agent app's services must never be reached from the portal's page.
const agent = vi.hoisted(() => ({
  getSessions: vi.fn(),
  revokeSession: vi.fn(),
  revokeAllOtherSessions: vi.fn(),
  changePassword: vi.fn(),
  getMFAStatus: vi.fn(),
  setupMFA: vi.fn(),
}))
vi.mock('@nosdesk/core/services/authService', () => ({ default: agent }))

import AccountView from '@/portal/views/AccountView.vue'
import PortalLayout from '@/portal/components/PortalLayout.vue'

const portalAccount = {
  stepUp: {
    status: vi.fn(async () => stepUpStatus({ methods: [] })),
    confirm: vi.fn(),
    confirmWithPasskey: vi.fn(),
  },
  sessions: {
    list: vi.fn(async () => [
      {
        session_id: 'here',
        device_name: null,
        ip_address: null,
        user_agent: null,
        location: null,
        created_at: '2026-11-01T00:00:00Z',
        last_active: '2026-11-01T00:00:00Z',
        expires_at: '2026-12-01T00:00:00Z',
        is_current: true,
      },
    ]),
    revoke: vi.fn(),
    revokeOthers: vi.fn(),
  },
  password: { change: vi.fn(async () => {}) },
  mfa: {
    status: vi.fn(async () => ({ enabled: false })),
    setup: vi.fn(async () => ({ secret: 'ABC', qr_code: 'data:image/png;base64,AAAA', backup_codes: [] })),
    verify: vi.fn(),
    enable: vi.fn(),
    disable: vi.fn(),
    regenerateBackupCodes: vi.fn(),
  },
  passkeys: { list: vi.fn(async () => []), register: vi.fn(), rename: vi.fn(), remove: vi.fn() },
} as unknown as AccountApi & { [k: string]: Record<string, ReturnType<typeof vi.fn>> }

const providePortalAccount: Plugin = { install: (app) => void app.provide(ACCOUNT_API, portalAccount) }

let wrapper: VueWrapper | null = null
beforeEach(() => {
  FakeBroadcastChannel.reset()
  vi.stubGlobal('BroadcastChannel', FakeBroadcastChannel)
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
  vi.unstubAllGlobals()
  vi.clearAllMocks()
})

const NO_SIGN_IN = { name: true, password: false, authenticator: false, passkeys: false }
const ALL = { name: true, password: true, authenticator: true, passkeys: true }

async function mount(component: unknown, scope: unknown = { kind: 'full' }, sections = NO_SIGN_IN) {
  service.getMe.mockResolvedValue({ uuid: 'u', name: 'Pat', email: 'pat@example.com', effective_locale: 'en-US', scope })
  service.getAccount.mockResolvedValue({
    uuid: 'u',
    name: 'Pat',
    email: 'pat@example.com',
    pronouns: null,
    locale: null,
    timezone: 'Australia/Sydney',
    sections,
  })
  const stub = { template: '<div />' }
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/account', component: stub },
      { path: '/tickets', component: stub },
      { path: '/tickets/new', component: stub },
      { path: '/tickets/:id', component: stub },
      { path: '/login', component: stub },
    ],
  })
  await router.push('/account')
  wrapper = mountWithProviders(component as never, {}, {}, [router, [PiniaColada, {}] as never, providePortalAccount])
  await flushPromises()
  return { wrapper, router }
}

describe('the portal account page', () => {
  it('shows the profile and saves only what the page edits', async () => {
    const { wrapper } = await mount(AccountView)
    const name = wrapper.find('input[autocomplete="name"]')
    expect((name.element as HTMLInputElement).value).toBe('Pat')

    await name.setValue('Pat Smith')
    await wrapper.find('form').trigger('submit')
    await flushPromises()

    expect(service.updateAccount).toHaveBeenCalledWith({
      name: 'Pat Smith',
      pronouns: '',
      locale: '',
      timezone: 'Australia/Sydney',
    })
    expect(wrapper.text()).toContain('portal-account-saved')
  })

  it('lists sessions through the portal, never the agent app', async () => {
    await mount(AccountView)
    expect(portalAccount.sessions.list).toHaveBeenCalled()
    expect(agent.getSessions).not.toHaveBeenCalled()
  })
})

describe('signing in, on the account page', () => {
  it('shows no password, authenticator or passkeys the server leaves out', async () => {
    const { wrapper } = await mount(AccountView)
    expect(wrapper.text()).not.toContain('settings-security-title')
    expect(wrapper.text()).not.toContain('settings-mfa-title')
    expect(wrapper.text()).not.toContain('settings-passkey-section-title')
  })

  it('changes the password through the portal', async () => {
    const { wrapper } = await mount(AccountView, { kind: 'full' }, ALL)
    const inputs = wrapper.findAll('input[type="password"]')
    await inputs[0]!.setValue('old-password')
    await inputs[1]!.setValue('new-password-1')
    await inputs[2]!.setValue('new-password-1')
    await inputs[0]!.element.form!.dispatchEvent(new Event('submit'))
    await flushPromises()
    expect(portalAccount.password.change).toHaveBeenCalledWith('old-password', 'new-password-1')
    expect(agent.changePassword).not.toHaveBeenCalled()
    expect(wrapper.text()).toContain('settings-security-success-changed')
  })

  it('sets up an authenticator and lists passkeys through the portal', async () => {
    // jsdom has no WebAuthn; a browser that has it lists passkeys.
    vi.stubGlobal('PublicKeyCredential', class {})
    const { wrapper } = await mount(AccountView, { kind: 'full' }, ALL)
    expect(portalAccount.mfa.status).toHaveBeenCalled()
    expect(portalAccount.passkeys.list).toHaveBeenCalled()
    const setUp = wrapper.findAll('button').find((b) => b.text() === 'portal-account-authenticator-set-up')
    await setUp!.trigger('click')
    await flushPromises()
    expect(portalAccount.mfa.setup).toHaveBeenCalled()
    expect(wrapper.find('img[alt="settings-mfa-qr-alt"]').exists()).toBe(true)
    expect(agent.getMFAStatus).not.toHaveBeenCalled()
    expect(agent.setupMFA).not.toHaveBeenCalled()
  })
})

describe('the account menu', () => {
  async function menuText(scope: unknown): Promise<string> {
    const { wrapper } = await mount(PortalLayout, scope)
    await wrapper.find('button[aria-label="portal-account-menu"]').trigger('click')
    await flushPromises()
    return document.body.textContent ?? ''
  }

  it('offers the account page to a signed-in requester', async () => {
    expect(await menuText({ kind: 'full' })).toContain('portal-account-settings')
  })

  it('leaves it out for a session opened from a link', async () => {
    expect(await menuText({ kind: 'ticket', ticket_number: 7 })).not.toContain('portal-account-settings')
  })
})

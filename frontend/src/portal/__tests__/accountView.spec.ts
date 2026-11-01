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
const agent = vi.hoisted(() => ({ getSessions: vi.fn(), revokeSession: vi.fn(), revokeAllOtherSessions: vi.fn() }))
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
} satisfies AccountApi

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

async function mount(component: unknown, scope: unknown = { kind: 'full' }) {
  service.getMe.mockResolvedValue({ uuid: 'u', name: 'Pat', email: 'pat@example.com', effective_locale: 'en-US', scope })
  service.getAccount.mockResolvedValue({
    uuid: 'u',
    name: 'Pat',
    email: 'pat@example.com',
    pronouns: null,
    locale: null,
    timezone: 'Australia/Sydney',
    sections: { name: true, password: false, authenticator: false, passkeys: false },
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

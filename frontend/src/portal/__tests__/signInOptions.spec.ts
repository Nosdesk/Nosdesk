import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { PiniaColada } from '@pinia/colada'
import { createMemoryHistory, createRouter } from 'vue-router'
import { mountWithProviders } from '@/test/mountWithProviders'

const service = vi.hoisted(() => ({
  getSso: vi.fn(),
  requestMagicLink: vi.fn(),
  signInWithCode: vi.fn(),
  SSO_START_URL: '/api/portal/auth/sso/start',
  getMe: vi.fn(async () => ({ uuid: 'u', name: 'Pat', email: null, effective_locale: 'en-US', scope: { kind: 'full' } })),
  getNotice: vi.fn(async () => ({ notice: null, following: false })),
  listMyApprovals: vi.fn(async () => []),
  followNotice: vi.fn(),
  signOut: vi.fn(async () => {}),
  listRequestTypes: vi.fn(async () => []),
  searchHelpArticles: vi.fn(async () => []),
  createMyTicket: vi.fn(),
}))
vi.mock('@/portal/service', () => service)

const publicSettings = vi.hoisted(() => ({ getSettings: vi.fn() }))
vi.mock('@nosdesk/core/services/publicService', () => ({ publicService: publicSettings }))

import LoginView from '@/portal/views/LoginView.vue'
import NewTicketView from '@/portal/views/NewTicketView.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
  vi.clearAllMocks()
})

async function mount(component: unknown, path: string) {
  const stub = { template: '<div />' }
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/login', component: stub },
      { path: '/tickets', component: stub },
      { path: '/tickets/new', component: stub },
    ],
  })
  await router.push(path)
  wrapper = mountWithProviders(component as never, {}, {}, [router, [PiniaColada, {}] as never])
  await flushPromises()
  return wrapper
}

describe('the portal sign-in page', () => {
  it('links to the password sign-in when the server takes passwords', async () => {
    service.getSso.mockResolvedValue({ enabled: false, password: true })
    const page = await mount(LoginView, '/login')
    const link = page.find('a[href="/login"]')
    expect(link.exists()).toBe(true)
    expect(link.text()).toBe('portal-sign-in-password')
  })

  it('has no password link on hosted', async () => {
    service.getSso.mockResolvedValue({ enabled: false, password: false })
    const page = await mount(LoginView, '/login')
    expect(page.find('a[href="/login"]').exists()).toBe(false)
  })

  it('has no password link when an older server leaves the flag out', async () => {
    service.getSso.mockResolvedValue({ enabled: false })
    const page = await mount(LoginView, '/login')
    expect(page.find('a[href="/login"]').exists()).toBe(false)
  })
})

describe('a new portal request', () => {
  async function typeSubject(kb: { docs: boolean; search: boolean }) {
    publicSettings.getSettings.mockResolvedValue({
      guest_public_docs_enabled: kb.docs,
      guest_kb_search_enabled: kb.search,
    })
    const page = await mount(NewTicketView, '/tickets/new')
    vi.useFakeTimers()
    await page.find('input').setValue('Printer jammed')
    vi.runAllTimers()
    vi.useRealTimers()
    await flushPromises()
  }

  it('suggests help articles when the help centre search is on', async () => {
    await typeSubject({ docs: true, search: true })
    expect(service.searchHelpArticles).toHaveBeenCalledWith('Printer jammed')
  })

  it('searches no articles when the help centre is off', async () => {
    await typeSubject({ docs: false, search: false })
    expect(service.searchHelpArticles).not.toHaveBeenCalled()
  })

  it('searches no articles when only its search is off', async () => {
    await typeSubject({ docs: true, search: false })
    expect(service.searchHelpArticles).not.toHaveBeenCalled()
  })
})

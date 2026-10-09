import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import { createMemoryHistory, createRouter } from 'vue-router'
import enUS from '../../../../i18n/locales/en-US/main.ftl?raw'

// Signed out, a requester who opens a portal page is sent to sign in and,
// once signed in, lands back on that page.

const service = vi.hoisted(() => ({
  getSso: vi.fn(async () => ({ enabled: false })),
  requestMagicLink: vi.fn(async () => {}),
  signInWithCode: vi.fn(async () => {}),
  SSO_START_URL: '/api/portal/auth/sso/start',
}))
vi.mock('../service', () => service)
vi.mock('@/stores/theme', () => ({ useThemeStore: () => ({ isDarkMode: false }) }))
vi.mock('@/stores/branding', () => ({
  useBrandingStore: () => ({ appName: 'Nosdesk', getLogoUrl: () => null }),
}))

// The portal's own router, as the API client sees it on an expired session.
const appRouter = vi.hoisted(() => ({
  currentRoute: { value: { name: 'ticket-new', fullPath: '/tickets/new' } },
  push: vi.fn(),
}))
vi.mock('../router', () => ({ default: appRouter }))
vi.mock('../embed', () => ({ isEmbed: false, embedBearer: () => null, embedHome: '/embed', signInEmbedded: vi.fn() }))

import axios, { AxiosError } from 'axios'
import portalApi from '../api'
import LoginView from '../views/LoginView.vue'

function makeRouter() {
  const page = { render: () => null }
  return createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/login', name: 'login', component: LoginView },
      { path: '/tickets', name: 'tickets', component: page },
      { path: '/tickets/new', name: 'ticket-new', component: page },
      { path: '/tickets/:number(\\d+)', name: 'ticket', component: page },
    ],
  })
}

/** Sign in with an emailed code from `/login<query>`; the path it lands on. */
async function signInFrom(loginUrl: string): Promise<string> {
  const router = makeRouter()
  await router.push(loginUrl)
  const bundle = new FluentBundle('en-US', { useIsolating: false })
  bundle.addResource(new FluentResource(enUS))
  const wrapper = mount(LoginView, {
    global: { plugins: [createFluentVue({ bundles: [bundle] }), router] },
  })
  await flushPromises()
  await wrapper.find('input[type="email"]').setValue('requester@example.com')
  await wrapper.find('form').trigger('submit')
  await flushPromises()
  await wrapper.find('input[autocomplete="one-time-code"]').setValue('123456')
  await wrapper.find('form').trigger('submit')
  await flushPromises()
  const landed = router.currentRoute.value.fullPath
  wrapper.unmount()
  return landed
}

beforeEach(() => {
  service.signInWithCode.mockClear()
})

describe('portal sign-in', () => {
  it('returns to the page the requester was going to', async () => {
    expect(await signInFrom('/login?redirect=/tickets/new')).toBe('/tickets/new')
    expect(service.signInWithCode).toHaveBeenCalledWith('requester@example.com', '123456')
  })

  it('lands on the request list without a destination', async () => {
    expect(await signInFrom('/login')).toBe('/tickets')
  })

  it('never follows a destination off the portal', async () => {
    for (const redirect of ['//evil.example', '/\\evil.example', 'https://evil.example/x', '/login?redirect=/tickets/new']) {
      expect(await signInFrom(`/login?redirect=${encodeURIComponent(redirect)}`)).toBe('/tickets')
    }
  })
})

describe('an expired portal session', () => {
  it('sends the requester to sign in with the page they were on', async () => {
    // The session can't be renewed.
    vi.spyOn(axios, 'post').mockRejectedValue(new Error('refresh refused'))
    portalApi.defaults.adapter = async (config) => {
      throw new AxiosError('expired', 'ERR_BAD_REQUEST', config, null, {
        status: 401,
        statusText: '',
        data: {},
        headers: {},
        config,
      })
    }

    await expect(portalApi.get('/me')).rejects.toBeTruthy()

    expect(appRouter.push).toHaveBeenCalledWith({ path: '/login', query: { redirect: '/tickets/new' } })
  })
})

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { mountWithProviders } from '@/test/mountWithProviders'

// First-run setup signs the new administrator in and stays on its own finish
// screen, which offers the workspace or an import, instead of landing in the app.

const push = vi.hoisted(() => vi.fn())
vi.mock('vue-router', () => ({
  useRouter: () => ({ push, replace: vi.fn(), currentRoute: { value: { name: 'onboarding', query: {} } } }),
}))
const landAfterLogin = vi.hoisted(() => vi.fn(async () => {}))
vi.mock('@/router', () => ({
  default: { currentRoute: { value: { name: 'onboarding', query: {} } }, push, replace: vi.fn() },
  landAfterLogin,
}))
const post = vi.hoisted(() => vi.fn())
vi.mock('@nosdesk/core/apiClient', () => ({
  default: {
    post,
    get: vi.fn(),
    defaults: { headers: { common: {} } },
    interceptors: { request: { use: vi.fn() }, response: { use: vi.fn() } },
  },
}))
const authService = vi.hoisted(() => ({
  checkSetupStatus: vi.fn(),
  setupInitialAdmin: vi.fn(),
  clearSetupStatusCache: vi.fn(),
}))
vi.mock('@nosdesk/core/services/authService', () => ({ default: authService }))
// The animated hero needs a 2D canvas, which jsdom lacks.
vi.mock('@/components/auth/AuthLayout.vue', () => ({ default: { template: '<div><slot /></div>' } }))

import OnboardingView from '@/views/OnboardingView.vue'
import { useAuthStore } from '@/stores/auth'

const admin = { uuid: 'u-1', name: 'Ada Admin', email: 'ada@example.com', role: 'admin' }

let wrapper: VueWrapper | null = null

beforeEach(() => {
  sessionStorage.setItem('nosdesk-bootstrap-token', 'setup-token')
  authService.checkSetupStatus.mockResolvedValue({ requires_setup: true })
  authService.setupInitialAdmin.mockResolvedValue({ success: true })
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  sessionStorage.clear()
  vi.clearAllMocks()
})

async function createAdministrator() {
  wrapper = mountWithProviders(OnboardingView)
  await flushPromises()
  await wrapper.find('input[autocomplete="name"]').setValue(admin.name)
  await wrapper.find('input[autocomplete="email"]').setValue(admin.email)
  await wrapper.find('#admin-password').setValue('correct horse')
  await wrapper.find('#confirm-password').setValue('correct horse')
  await wrapper.find('form').trigger('submit')
  await flushPromises()
  return wrapper
}

describe('OnboardingView', () => {
  it('stays on its finish screen, signed in, after creating the administrator', async () => {
    post.mockResolvedValue({ data: { success: true, csrf_token: 'csrf', user: admin } })

    const view = await createAdministrator()

    expect(post).toHaveBeenCalledWith('/auth/login', { email: admin.email, password: 'correct horse' })
    expect(useAuthStore().user?.email).toBe(admin.email)
    expect(landAfterLogin).not.toHaveBeenCalled()
    expect(push).not.toHaveBeenCalled()
    expect(view.find('[data-test="onboarding-open"]').exists()).toBe(true)
    expect(view.find('[data-test="onboarding-import"]').exists()).toBe(true)
  })

  it('says welcome once, with the setup header and its banner gone', async () => {
    post.mockResolvedValue({ data: { success: true, csrf_token: 'csrf', user: admin } })

    const view = await createAdministrator()

    const text = view.text()
    expect(text).toContain('onboarding-complete-title')
    expect(text).not.toContain('onboarding-welcome-title')
    expect(text).not.toContain('onboarding-welcome-subtitle')
    expect(text).not.toContain('onboarding-success-logging-in')
    expect(text).not.toContain('onboarding-progress-title')
    expect(text).toContain(admin.name)
    expect(text).toContain(admin.email)
    expect(view.find('h1').text()).toBe('onboarding-complete-title')
  })

  it('shows the workspace opening while the navigation runs', async () => {
    post.mockResolvedValue({ data: { success: true, csrf_token: 'csrf', user: admin } })
    const view = await createAdministrator()
    // The sync bootstrap keeps this navigation pending for a few seconds.
    push.mockReturnValueOnce(new Promise(() => {}))

    await view.find('[data-test="onboarding-open"]').trigger('click')

    const open = view.find('[data-test="onboarding-open"]')
    expect(open.text()).toContain('onboarding-complete-opening')
    expect(open.attributes('disabled')).toBeDefined()
    expect(open.find('[role="status"]').exists()).toBe(true)
    expect(view.find('[data-test="onboarding-import"]').attributes('disabled')).toBeDefined()
  })

  it('goes into the app or the import from the finish screen', async () => {
    post.mockResolvedValue({ data: { success: true, csrf_token: 'csrf', user: admin } })
    const view = await createAdministrator()

    await view.find('[data-test="onboarding-import"]').trigger('click')
    expect(push).toHaveBeenLastCalledWith({ name: 'admin-workspace-import' })

    await view.find('[data-test="onboarding-open"]').trigger('click')
    expect(push).toHaveBeenLastCalledWith('/')
  })
})

describe('signing in elsewhere', () => {
  it('still lands in the app', async () => {
    setActivePinia(createPinia())
    post.mockResolvedValue({ data: { success: true, csrf_token: 'csrf', user: admin } })

    expect(await useAuthStore().login({ email: admin.email, password: 'secret' })).toBe(true)
    expect(landAfterLogin).toHaveBeenCalledTimes(1)
  })
})

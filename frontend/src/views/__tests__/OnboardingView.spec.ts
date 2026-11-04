import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

// The setup page's hero names the steps that follow. A server that doesn't
// require admins to have a second factor (REQUIRE_ADMIN_MFA=false) has no such
// step, so the hero mustn't promise one.
vi.mock('vue-router', () => ({
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
}))
vi.mock('@/composables/useAutoLogin', () => ({
  useAutoLogin: () => ({ attemptLogin: vi.fn() }),
}))
vi.mock('@/stores/theme', () => ({ useThemeStore: () => ({ resetToDefault: vi.fn() }) }))
// The animated hero needs a 2D canvas, which jsdom lacks.
vi.mock('@/components/auth/AuthLayout.vue', () => ({
  default: { template: '<div><p data-test="hero-subtitle"><slot name="hero-subtitle" /></p><slot /></div>' },
}))
const checkSetupStatus = vi.hoisted(() => vi.fn())
vi.mock('@nosdesk/core/services/authService', () => ({ default: { checkSetupStatus } }))

import OnboardingView from '@/views/OnboardingView.vue'

let wrapper: VueWrapper | null = null

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

async function heroSubtitle(status: Record<string, unknown>): Promise<string> {
  checkSetupStatus.mockResolvedValue({ requires_setup: true, user_count: 0, ...status })
  wrapper = mountWithProviders(OnboardingView)
  await flushPromises()
  return wrapper.find('[data-test="hero-subtitle"]').text()
}

describe('the onboarding hero', () => {
  it('mentions the second factor when admins must add one', async () => {
    expect(await heroSubtitle({ admin_mfa_required: true })).toBe('onboarding-hero-subtitle')
  })

  it('leaves the second factor out when admins needn\'t add one', async () => {
    expect(await heroSubtitle({ admin_mfa_required: false })).toBe('onboarding-hero-subtitle-no-mfa')
  })

  it('mentions it for a server that doesn\'t say, as its default asks', async () => {
    expect(await heroSubtitle({})).toBe('onboarding-hero-subtitle')
  })
})

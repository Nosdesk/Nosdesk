import { afterEach, describe, expect, it, vi } from 'vitest'
import { computed, ref } from 'vue'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

// The contact cards (custom fields, phones, addresses) show only where the
// server reports `editable.contact`: an agent on a colleague's profile would
// only fetch refusals.
const profile = vi.hoisted(() => ({ contact: false, role: 'technician' as string }))

vi.mock('vue-router', () => ({
  useRoute: () => ({ params: { uuid: 'colleague' }, query: {} }),
  useRouter: () => ({ push: vi.fn(), replace: vi.fn(), resolve: () => ({ href: '#', matched: [] }) }),
  RouterLink: { template: '<a><slot /></a>' },
}))
vi.mock('@/stores/auth', () => ({
  useAuthStore: () => ({
    user: { uuid: 'agent', workspace_role: 'agent' },
    isAdmin: false,
    isTechnician: true,
    isPlatformAdmin: false,
  }),
}))
vi.mock('@/composables/useUserProfileBundle', () => ({
  useUserProfileBundle: () => ({
    bundle: computed(() => ({
      user: {
        uuid: 'colleague',
        name: 'Sam',
        email: 'sam@example.test',
        workspace_role: profile.role,
      },
      devices: [],
      loans: [],
      groups: [],
      editable: { contact: profile.contact, emails: false },
    })),
    isLoading: computed(() => false),
    error: ref(null),
    refetch: vi.fn(),
  }),
}))
vi.mock('@/composables/useSyncActions', () => ({ useSyncActions: vi.fn() }))
vi.mock('@nosdesk/core/services/instanceConfig', async (importOriginal) => ({
  ...(await importOriginal<object>()),
  isHostedDeployment: () => false,
  isIdentityExternallyManaged: () => false,
}))
vi.mock('@/services/activeWorkspace', () => ({ openControlPlaneSeats: vi.fn() }))
vi.mock('@/services/userService', () => ({
  default: { getEmailConfigStatus: async () => ({ is_configured: true, enabled: true }) },
}))
// The page's other cards (assigned tickets and the like) fetch; nothing here
// depends on them.
vi.mock('@nosdesk/core/apiClient', () => ({
  default: { get: async () => ({ data: [] }), post: async () => ({ data: {} }) },
}))
vi.mock('@/plugins/components/PluginSlot.vue', () => ({ default: { template: '<div />' } }))
vi.mock('@/components/settings/UserContactCard.vue', () => ({
  default: { props: ['uuid', 'editable'], template: '<div data-test="UserContactCard" />' },
}))
vi.mock('@/components/settings/UserPhonesCard.vue', () => ({
  default: { props: ['uuid', 'editable'], template: '<div data-test="UserPhonesCard" />' },
}))
vi.mock('@/components/settings/UserAddressesCard.vue', () => ({
  default: { props: ['uuid', 'editable'], template: '<div data-test="UserAddressesCard" />' },
}))

import UserProfileView from '@/views/UserProfileView.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

async function shownCards() {
  wrapper = mountWithProviders(UserProfileView, {}, {}, [])
  await flushPromises()
  return ['UserContactCard', 'UserPhonesCard', 'UserAddressesCard'].filter((c) =>
    wrapper!.find(`[data-test="${c}"]`).exists(),
  )
}

describe("UserProfileView contact cards", () => {
  it("are left out of a colleague's profile an agent can't see the contact details of", async () => {
    profile.contact = false
    profile.role = 'technician'
    expect(await shownCards()).toEqual([])
  })

  it("show on a requester's profile, where the agent may change them", async () => {
    profile.contact = true
    profile.role = 'member'
    expect(await shownCards()).toEqual(['UserContactCard', 'UserPhonesCard', 'UserAddressesCard'])
  })
})

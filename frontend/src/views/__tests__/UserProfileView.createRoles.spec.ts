import { afterEach, describe, expect, it, vi } from 'vitest'
import { computed, ref } from 'vue'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

// The add-user form offers Admin only to a caller who may grant it, as the
// member-role menu does: the workspace owner.
const caller = vi.hoisted(() => ({ role: 'owner' as string }))

vi.mock('vue-router', () => ({
  useRoute: () => ({ params: { uuid: 'new' }, query: {} }),
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
}))
vi.mock('@/stores/auth', () => ({
  useAuthStore: () => ({
    user: { uuid: 'caller', workspace_role: caller.role },
    isAdmin: true,
    isTechnician: true,
    isPlatformAdmin: false,
  }),
}))
vi.mock('@/composables/useUserProfileBundle', () => ({
  useUserProfileBundle: () => ({
    bundle: computed(() => undefined),
    isLoading: computed(() => false),
    error: ref(null),
    refetch: vi.fn(),
  }),
}))
vi.mock('@/composables/useSyncActions', () => ({ useSyncActions: vi.fn() }))
vi.mock('@nosdesk/core/services/instanceConfig', () => ({
  isHostedDeployment: () => false,
  isIdentityExternallyManaged: () => false,
}))
vi.mock('@/services/activeWorkspace', () => ({ openControlPlaneSeats: vi.fn() }))
vi.mock('@/services/userService', () => ({
  default: {
    getEmailConfigStatus: async () => ({ is_configured: true, enabled: true }),
    createUser: vi.fn(),
    updateUser: vi.fn(),
  },
}))
vi.mock('@/components/common/BaseDropdown.vue', () => ({
  default: {
    props: ['options', 'modelValue'],
    template: `<div data-test="dropdown"><span v-for="o in options" :key="o.value" :data-value="o.value" /></div>`,
  },
}))
vi.mock('@/plugins/components/PluginSlot.vue', () => ({ default: { template: '<div />' } }))

import UserProfileView from '@/views/UserProfileView.vue'

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

async function offeredRoles(role: string) {
  caller.role = role
  wrapper = mountWithProviders(UserProfileView, {}, {}, [])
  await flushPromises()
  const picker = wrapper
    .findAll('[data-test="dropdown"]')
    .find((d) => d.find('[data-value="technician"]').exists())
  expect(picker).toBeTruthy()
  return picker!.findAll('[data-value]').map((o) => o.attributes('data-value'))
}

describe('UserProfileView create form roles', () => {
  it('offers Admin to the owner', async () => {
    expect(await offeredRoles('owner')).toContain('admin')
  })

  it('offers an admin every role but Admin', async () => {
    expect(await offeredRoles('admin')).toEqual(['user', 'technician', 'audit_reviewer'])
  })
})

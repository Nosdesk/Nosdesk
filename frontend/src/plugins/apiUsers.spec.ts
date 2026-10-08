/**
 * A requester-role member gets only other people's name and avatar from the
 * server. Plugins running in their session still get a whole `PluginUser`:
 * the email and role come through empty rather than missing.
 */
import { describe, expect, it, vi } from 'vitest'
import type { Plugin } from '@nosdesk/core/types/plugin'

const others = [
  { uuid: '0190a5b2-0000-7000-8000-0000000000a1', name: 'Sam Rivera', avatar_url: null },
]

vi.mock('@/services/userService', () => ({
  default: {
    getPaginatedUsers: vi.fn(async () => ({ data: others, total: others.length })),
    getUserByUuid: vi.fn(async () => others[0]),
  },
}))
// The real store pulls in the router and every view.
vi.mock('@/stores/auth', () => ({ useAuthStore: vi.fn() }))

const { createPluginAPI } = await import('./api')

const plugin = {
  uuid: '0190a5b2-0000-7000-8000-000000000002',
  name: 'people-reader',
  consented_permissions: ['user:read'],
  manifest: { permissions: ['user:read'] },
} as unknown as Plugin

describe('plugin users seen by a requester-role member', () => {
  it('lists someone whose email and role are withheld', async () => {
    const { users } = await createPluginAPI(plugin).users.list()
    expect(users).toEqual([
      { uuid: others[0].uuid, name: 'Sam Rivera', email: '', avatarUrl: null, role: '' },
    ])
  })

  it('gets one such person by uuid', async () => {
    const user = await createPluginAPI(plugin).users.get(others[0].uuid)
    expect(user?.email).toBe('')
    expect(user?.role).toBe('')
  })
})

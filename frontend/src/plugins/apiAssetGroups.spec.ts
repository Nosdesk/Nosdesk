/**
 * `assets.groups` lists the workspace's active asset groups from the app's
 * cached store, narrowed to id, name and colour, behind `asset:read`.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { asPluginApiError } from '@nosdesk/plugin-sdk'
import type { Plugin } from '@nosdesk/core/types/plugin'

const store = vi.hoisted(() => ({
  error: null as string | null,
  load: vi.fn(),
}))
vi.mock('@/stores/assetGroups', () => ({ useAssetGroupsStore: () => store }))
// The real store pulls in the router and every view.
vi.mock('@/stores/auth', () => ({ useAuthStore: vi.fn() }))

const { createPluginAPI } = await import('./api')

const reader = {
  uuid: '0190a5b2-0000-7000-8000-000000000006',
  name: 'group-reader',
  consented_permissions: ['asset:read'],
  manifest: { permissions: ['asset:read'] },
} as unknown as Plugin

beforeEach(() => {
  store.error = null
  store.load.mockReset()
  store.load.mockResolvedValue([
    {
      id: 4,
      uuid: '0190a5b2-0000-7000-8000-0000000000a4',
      name: 'Lab',
      description: 'Bench kit',
      color: '#3b82f6',
      display_order: 0,
      archived_at: null,
      asset_count: 12,
    },
    { id: 7, uuid: '0190a5b2-0000-7000-8000-0000000000a7', name: 'Loaners', display_order: 1, asset_count: 3 },
  ])
})

describe('plugin asset groups', () => {
  it('lists id, name and colour only', async () => {
    const groups = await createPluginAPI(reader).assets.groups()
    expect(groups).toEqual([
      { id: 4, name: 'Lab', color: '#3b82f6' },
      { id: 7, name: 'Loaners', color: null },
    ])
  })

  it('needs asset:read', async () => {
    const plugin = { ...reader, consented_permissions: [], manifest: { permissions: [] } } as unknown as Plugin
    const err = asPluginApiError(await createPluginAPI(plugin).assets.groups().catch((e: unknown) => e))
    expect(err?.code).toBe('denied')
    expect(store.load).not.toHaveBeenCalled()
  })

  it('reports a failed load as upstream', async () => {
    store.load.mockImplementation(async () => {
      store.error = 'Failed to load asset groups'
      return []
    })
    const err = asPluginApiError(await createPluginAPI(reader).assets.groups().catch((e: unknown) => e))
    expect(err?.code).toBe('upstream')
  })
})

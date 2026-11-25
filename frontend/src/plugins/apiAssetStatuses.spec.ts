/**
 * `assets.statuses` lists the workspace's named asset statuses from
 * `GET /assets/statuses`, in camel case, behind `asset:read`; a server
 * older than named statuses has none.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { asPluginApiError } from '@nosdesk/plugin-sdk'
import type { Plugin } from '@nosdesk/core/types/plugin'

const service = vi.hoisted(() => ({ list: vi.fn() }))
vi.mock('@nosdesk/core/services/assetStatusService', () => ({ assetStatusService: service }))
// The real store pulls in the router and every view.
vi.mock('@/stores/auth', () => ({ useAuthStore: vi.fn() }))

const { createPluginAPI } = await import('./api')

const reader = {
  uuid: '0190a5b2-0000-7000-8000-000000000007',
  name: 'status-reader',
  consented_permissions: ['asset:read'],
  manifest: { permissions: ['asset:read'] },
} as unknown as Plugin

beforeEach(() => {
  service.list.mockReset()
  service.list.mockResolvedValue({
    categories: [],
    statuses: [
      {
        id: 3,
        category: 'in_repair',
        slug: 'at_vendor',
        label: 'At vendor',
        description: null,
        color: '#aa3300',
        position: 0,
        is_default: false,
        applies_to_kinds: ['laptop'],
        archived: false,
      },
    ],
  })
})

describe('plugin asset statuses', () => {
  it('lists the named statuses', async () => {
    expect(await createPluginAPI(reader).assets.statuses()).toEqual([
      {
        id: 3,
        category: 'in_repair',
        slug: 'at_vendor',
        label: 'At vendor',
        description: null,
        color: '#aa3300',
        position: 0,
        isDefault: false,
        appliesToKinds: ['laptop'],
        archived: false,
      },
    ])
  })

  it('has none from a server without them', async () => {
    service.list.mockResolvedValue(null)
    expect(await createPluginAPI(reader).assets.statuses()).toEqual([])
  })

  it('needs asset:read', async () => {
    const plugin = { ...reader, consented_permissions: [], manifest: { permissions: [] } } as unknown as Plugin
    const err = asPluginApiError(await createPluginAPI(plugin).assets.statuses().catch((e: unknown) => e))
    expect(err?.code).toBe('denied')
    expect(service.list).not.toHaveBeenCalled()
  })

  it('reports a failed load as upstream', async () => {
    service.list.mockRejectedValue(new Error('boom'))
    const err = asPluginApiError(await createPluginAPI(reader).assets.statuses().catch((e: unknown) => e))
    expect(err?.code).toBe('upstream')
  })
})

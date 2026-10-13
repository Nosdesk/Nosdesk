/**
 * `PUT /assets/{id}` ignores `status`, so a plugin patch carrying it used to
 * come back as a success with nothing changed. The bridge refuses it instead.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { asPluginApiError } from '@nosdesk/plugin-sdk'
import type { Plugin } from '@nosdesk/core/types/plugin'

const updateAsset = vi.fn(async (id: number, patch: object) => ({ id, ...patch }))

vi.mock('@/services/assetService', () => ({
  getAssetById: vi.fn(),
  getAssets: vi.fn(),
  updateAsset: (...args: [number, object]) => updateAsset(...args),
}))
// The real store pulls in the router and every view.
vi.mock('@/stores/auth', () => ({ useAuthStore: vi.fn() }))

const { createPluginAPI } = await import('./api')

const plugin = {
  uuid: '0190a5b2-0000-7000-8000-000000000003',
  name: 'asset-writer',
  consented_permissions: ['asset:write'],
  manifest: { permissions: ['asset:write'] },
} as unknown as Plugin

describe('plugin asset updates', () => {
  beforeEach(() => updateAsset.mockClear())

  it('refuses a patch that changes status', async () => {
    const err = await createPluginAPI(plugin)
      .assets.update(5, { name: 'Laptop', status: 'retired' })
      .catch((e: unknown) => e)
    const pe = asPluginApiError(err)
    expect(pe?.code).toBe('invalid')
    expect(pe?.message).toMatch(/asset lifecycle/)
    expect(updateAsset).not.toHaveBeenCalled()
  })

  it('still applies a patch without status', async () => {
    const asset = await createPluginAPI(plugin).assets.update(5, { name: 'Laptop' })
    expect(updateAsset).toHaveBeenCalledOnce()
    expect(asset).toMatchObject({ id: 5, name: 'Laptop' })
  })
})

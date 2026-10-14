/**
 * Plugin storage compare-and-set. `getEntry` returns the value with its
 * version; `compareAndSet` sends the expected version and turns the server's
 * 409 into a `conflict` PluginApiError, so two people editing a shared
 * document can't silently overwrite each other.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { asPluginApiError } from '@nosdesk/plugin-sdk'
import type { Plugin } from '@nosdesk/core/types/plugin'

const store = vi.hoisted(() => ({
  getStorage: vi.fn(),
  setStorage: vi.fn(),
  deleteStorage: vi.fn(),
}))
vi.mock('@nosdesk/core/services/pluginService', () => ({ default: store }))
// The real store pulls in the router and every view.
vi.mock('@/stores/auth', () => ({ useAuthStore: vi.fn() }))

const { createPluginAPI } = await import('./api')

const plugin = {
  uuid: '0190a5b2-0000-7000-8000-000000000005',
  name: 'taxonomy',
  consented_permissions: ['storage:plugin'],
  manifest: { permissions: ['storage:plugin'] },
} as unknown as Plugin

describe('plugin storage compare-and-set', () => {
  beforeEach(() => {
    store.getStorage.mockReset()
    store.setStorage.mockReset()
  })

  it('reads the value with its version', async () => {
    store.getStorage.mockResolvedValue({ key: 'taxonomy', value: { lists: [] }, version: 4 })
    expect(await createPluginAPI(plugin).storage.getEntry('taxonomy')).toEqual({
      value: { lists: [] },
      version: 4,
    })
  })

  it('writes with the expected version and returns the new one', async () => {
    store.setStorage.mockResolvedValue({ key: 'taxonomy', value: { lists: [1] }, version: 5 })
    const version = await createPluginAPI(plugin).storage.compareAndSet('taxonomy', { lists: [1] }, 4)
    expect(version).toBe(5)
    expect(store.setStorage).toHaveBeenCalledWith(plugin.uuid, {
      key: 'taxonomy',
      value: { lists: [1] },
      expected_version: 4,
    })
  })

  it('reports a lost race as a conflict', async () => {
    store.setStorage.mockRejectedValue({ response: { status: 409, data: { version: 6 } } })
    const err = await createPluginAPI(plugin)
      .storage.compareAndSet('taxonomy', {}, 4)
      .catch((e: unknown) => e)
    const pe = asPluginApiError(err)
    expect(pe?.code).toBe('conflict')
    expect(pe?.message).toMatch(/version 6/)
  })

  it('refuses a malformed expected version without a request', async () => {
    const err = await createPluginAPI(plugin)
      .storage.compareAndSet('taxonomy', {}, -1)
      .catch((e: unknown) => e)
    expect(asPluginApiError(err)?.code).toBe('invalid')
    expect(store.setStorage).not.toHaveBeenCalled()
  })
})

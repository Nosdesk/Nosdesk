/**
 * `tickets.update` and `assets.update` hit the same endpoints the app uses,
 * which accept far more than a plugin may write. The bridge sends only the
 * `PluginTicketPatch` / `PluginAssetPatch` fields and refuses any other key.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { asPluginApiError } from '@nosdesk/plugin-sdk'
import type { Plugin } from '@nosdesk/core/types/plugin'

const updateTicket = vi.fn(async (id: number, patch: object) => ({ id, ...patch }))
const updateAsset = vi.fn(async (id: number, patch: object) => ({ id, ...patch }))

vi.mock('@nosdesk/core/services/ticketService', () => ({
  getTicketById: vi.fn(),
  getTickets: vi.fn(),
  addCommentToTicket: vi.fn(),
  updateTicket: (...args: [number, object]) => updateTicket(...args),
  deleteTicket: vi.fn(),
}))
vi.mock('@/services/assetService', () => ({
  getAssetById: vi.fn(),
  getAssets: vi.fn(),
  getPaginatedAssets: vi.fn(),
  updateAsset: (...args: [number, object]) => updateAsset(...args),
}))
// The real store pulls in the router and every view.
vi.mock('@/stores/auth', () => ({ useAuthStore: vi.fn() }))

const { createPluginAPI } = await import('./api')

const plugin = {
  uuid: '0190a5b2-0000-7000-8000-000000000005',
  name: 'record-writer',
  consented_permissions: ['ticket:write', 'asset:write'],
  manifest: { permissions: ['ticket:write', 'asset:write'] },
} as unknown as Plugin

async function refusal(p: Promise<unknown>) {
  return asPluginApiError(await p.then(() => null, (e: unknown) => e))
}

beforeEach(() => {
  updateTicket.mockClear()
  updateAsset.mockClear()
})

describe('plugin asset patches', () => {
  it('sends name and location only', async () => {
    await createPluginAPI(plugin).assets.update(5, { name: 'Laptop', location: null })
    expect(updateAsset).toHaveBeenCalledOnce()
    expect(updateAsset.mock.calls[0][1]).toEqual({ name: 'Laptop', location: null })
  })

  it.each(['kind', 'attributes', 'notes', 'asset_tag'])('refuses %s, naming it', async (key) => {
    const patch = { name: 'Laptop', [key]: 'x' }
    const err = await refusal(createPluginAPI(plugin).assets.update(5, patch))
    expect(err?.code).toBe('invalid')
    expect(err?.message).toContain(`"${key}"`)
    expect(updateAsset).not.toHaveBeenCalled()
  })

  it('still refuses status with the lifecycle message', async () => {
    const err = await refusal(createPluginAPI(plugin).assets.update(5, { status: 'retired' }))
    expect(err?.message).toMatch(/asset lifecycle/)
  })

  it('refuses a patch that is not an object', async () => {
    for (const patch of [null, 'name', ['name']]) {
      const err = await refusal(createPluginAPI(plugin).assets.update(5, patch as never))
      expect(err?.code).toBe('invalid')
    }
    expect(updateAsset).not.toHaveBeenCalled()
  })

  it('skips keys left undefined', async () => {
    const patch = { name: 'Laptop', notes: undefined } as never
    await createPluginAPI(plugin).assets.update(5, patch)
    expect(updateAsset.mock.calls[0][1]).toEqual({ name: 'Laptop' })
  })
})

describe('plugin ticket patches', () => {
  it('sends the ticket patch fields', async () => {
    const patch = { title: 'Printer jam', priority: 'high', workflow_state_id: 4, assignee: null }
    await createPluginAPI(plugin).tickets.update(9, patch)
    expect(updateTicket).toHaveBeenCalledOnce()
    expect(updateTicket.mock.calls[0][1]).toEqual(patch)
  })

  it.each(['requester', 'description', 'closed_at', 'status'])('refuses %s, naming it', async (key) => {
    const patch = { title: 'Printer jam', [key]: 'x' }
    const err = await refusal(createPluginAPI(plugin).tickets.update(9, patch))
    expect(err?.code).toBe('invalid')
    expect(err?.message).toContain(`"${key}"`)
    expect(updateTicket).not.toHaveBeenCalled()
  })
})

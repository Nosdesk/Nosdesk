/**
 * A requester in a workspace that shows requesters no assets loses every
 * asset record the pool holds, and nothing else, as soon as the flag says so.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import * as pool from '@nosdesk/core/sync/pool'

vi.mock('@nosdesk/core/services/ticketService', () => ({ default: {} }))
vi.mock('@/sync/stores/tickets', () => ({ apiTicketToSync: (t: unknown) => t }))

const { followAssetVisibility } = await import('./lifecycle')

function seeded() {
  pool.reset()
  pool.upsert('ticket', 1, { id: 1, title: 'Laptop will not boot' })
  pool.upsert('asset', 7, { id: 7, name: 'Laptop' })
  pool.upsert('ticket_asset', '1:7', { ticket_id: 1, asset_id: 7 })
}

describe('followAssetVisibility', () => {
  beforeEach(seeded)

  it('drops every asset and ticket link when assets are hidden', async () => {
    await followAssetVisibility(false, 'delta')
    expect(pool.has('asset', 7)).toBe(false)
    expect(pool.has('ticket_asset', '1:7')).toBe(false)
    expect(pool.has('ticket', 1)).toBe(true)
  })

  it('keeps them while assets are shown, or when the flag is absent', async () => {
    await followAssetVisibility(true, 'delta')
    await followAssetVisibility(undefined, 'delta')
    expect(pool.has('asset', 7)).toBe(true)
    expect(pool.has('ticket_asset', '1:7')).toBe(true)
  })
})

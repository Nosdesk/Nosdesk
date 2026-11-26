import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import * as pool from '@nosdesk/core/sync/pool'

// A ticket known only by id: the pool answers for one it holds, any other
// is read once and its number and title remembered, and one that can't be
// read is asked for no more.

const service = vi.hoisted(() => ({ getTicketById: vi.fn() }))
vi.mock('@nosdesk/core/services/ticketService', () => ({ default: service }))
vi.mock('@nosdesk/core/services/instanceConfig', () => ({
  getWorkspaceRouting: () => 'path',
  serverHasTicketNumbers: () => true,
}))
vi.mock('@/services/activeWorkspace', () => ({ activeWorkspaceSlug: () => 'acme' }))

const { numberForTicketId, titleForTicketId } = await import('../useTicketNumberLookup')

describe('useTicketNumberLookup', () => {
  beforeEach(() => {
    pool.reset()
    service.getTicketById.mockReset()
  })

  it('answers from the pool without a read', () => {
    pool.upsert('ticket', 40, { id: 40, number: 7, title: 'Printer jams on tray 2' })
    expect(numberForTicketId(40)).toBe(7)
    expect(titleForTicketId(40)).toBe('Printer jams on tray 2')
    expect(service.getTicketById).not.toHaveBeenCalled()
  })

  it('reads a ticket the pool lacks once, for its number and title', async () => {
    service.getTicketById.mockResolvedValue({ id: 50, number: 12, title: 'Fan noise' })
    expect(numberForTicketId(50)).toBeUndefined()
    expect(titleForTicketId(50)).toBeUndefined()
    await flushPromises()
    expect(numberForTicketId(50)).toBe(12)
    expect(titleForTicketId(50)).toBe('Fan noise')
    expect(service.getTicketById).toHaveBeenCalledTimes(1)
  })

  it("gives nothing for a ticket it can't read, and doesn't ask again", async () => {
    service.getTicketById.mockRejectedValue(new Error('not found'))
    expect(numberForTicketId(60)).toBeUndefined()
    await flushPromises()
    expect(numberForTicketId(60)).toBeUndefined()
    expect(titleForTicketId(60)).toBeUndefined()
    expect(service.getTicketById).toHaveBeenCalledTimes(1)
  })
})

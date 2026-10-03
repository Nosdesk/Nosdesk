/**
 * A pasted or dropped ticket URL names a ticket only when it's this
 * workspace's: the same origin, and in path routing the active slug.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import * as pool from '@nosdesk/core/sync/pool'

const routing = vi.hoisted(() => ({ mode: 'path' as 'path' | 'host', slug: 'acme' }))
vi.mock('@nosdesk/core/services/instanceConfig', () => ({
  getWorkspaceRouting: () => routing.mode,
  serverHasTicketNumbers: () => true,
}))
vi.mock('@/services/activeWorkspace', () => ({ activeWorkspaceSlug: () => routing.slug }))
vi.mock('@nosdesk/core/services/ticketService', () => ({ default: {} }))

const { ticketIdFromUrl } = await import('./ticketNumbers')

describe('ticketIdFromUrl', () => {
  const { origin, host } = window.location

  beforeEach(() => {
    pool.reset()
    routing.mode = 'path'
    routing.slug = 'acme'
    // Ticket 40 is #7 in this workspace.
    pool.upsert('ticket', 40, { id: 40, number: 7 })
  })

  it("reads this workspace's ticket link, with or without its scheme", () => {
    expect(ticketIdFromUrl(`${origin}/acme/tickets/7`)).toBe(40)
    expect(ticketIdFromUrl(`  ${host}/acme/tickets/7  `)).toBe(40)
    expect(ticketIdFromUrl(`${origin}/acme/tickets/id/40`)).toBe(40)
  })

  it("ignores another workspace's link, another site's, and a number it doesn't hold", () => {
    expect(ticketIdFromUrl(`${origin}/other/tickets/7`)).toBeNull()
    expect(ticketIdFromUrl('https://elsewhere.example/acme/tickets/7')).toBeNull()
    expect(ticketIdFromUrl(`${origin}/acme/tickets/99`)).toBeNull()
    expect(ticketIdFromUrl('see ticket 7')).toBeNull()
  })

  it('reads a bare ticket path in host routing', () => {
    routing.mode = 'host'
    expect(ticketIdFromUrl(`${origin}/tickets/7`)).toBe(40)
    expect(ticketIdFromUrl(`${origin}/acme/tickets/7`)).toBeNull()
  })
})

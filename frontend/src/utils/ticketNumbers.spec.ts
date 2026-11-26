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

const { ticketIdFromUrl, ticketLabel, ticketReferenceForId } = await import('./ticketNumbers')

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

describe('ticketLabel', () => {
  it('reads as a number and a title, or the number alone', () => {
    expect(ticketLabel(123, 'Printer jams on tray 2')).toBe('#123 Printer jams on tray 2')
    expect(ticketLabel(123, '  Printer jams  ')).toBe('#123 Printer jams')
    expect(ticketLabel(123, '')).toBe('#123')
    expect(ticketLabel(123, null)).toBe('#123')
    expect(ticketLabel(123)).toBe('#123')
  })
})

describe('ticketReferenceForId', () => {
  beforeEach(() => pool.reset())

  it("answers from the pool with the ticket's number and title", async () => {
    pool.upsert('ticket', 40, { id: 40, number: 7, title: 'Printer jams on tray 2' })
    expect(await ticketReferenceForId(40)).toEqual({ number: 7, title: 'Printer jams on tray 2' })
  })

  it("is undefined for a ticket it can't read", async () => {
    // The mocked service has no getTicketById, so the read fails.
    expect(await ticketReferenceForId(41)).toBeUndefined()
  })
})

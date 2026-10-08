import { describe, expect, it } from 'vitest'

import { isClosed, isMerged, type PortalTicket, type StateCategory } from './service'

function ticket(category: StateCategory): PortalTicket {
  return {
    id: 1,
    number: 1,
    title: 'Laptop won’t boot',
    state: { name: category, category },
  } as PortalTicket
}

describe('portal ticket state', () => {
  it('counts a merged request as closed', () => {
    expect(isClosed(ticket('merged'))).toBe(true)
    expect(isMerged(ticket('merged'))).toBe(true)
  })

  it('keeps open and closed as before', () => {
    expect(isClosed(ticket('done'))).toBe(true)
    expect(isClosed(ticket('cancelled'))).toBe(true)
    expect(isClosed(ticket('active'))).toBe(false)
    expect(isMerged(ticket('done'))).toBe(false)
  })
})

import { describe, it, expect } from 'vitest'
import { pluginEventsFor } from './eventDispatcher'

describe('pluginEventsFor', () => {
  // A merge moves each source ticket to the merged state. Webhooks hear it as
  // ticket.updated; plugins hear the same.
  it('tells plugins a merged ticket was updated', () => {
    expect(pluginEventsFor('ticket.merged_into')).toEqual(['ticket:updated'])
  })

  it('leaves the destination-side merge event alone', () => {
    expect(pluginEventsFor('ticket.merged')).toEqual([])
  })
})

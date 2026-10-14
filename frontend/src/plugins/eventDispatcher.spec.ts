import { describe, it, expect, vi } from 'vitest'
import { notifySyncActions } from '@nosdesk/core/sync/observers'
import type { SyncAction } from '@nosdesk/core/sync/types'

const handler = vi.hoisted(() => vi.fn())
vi.mock('./loader', () => ({
  getLoadedPlugin: () => ({ plugin: { trust_level: 'verified' } }),
}))
vi.mock('./pluginInstances', () => ({
  forEachLiveInstance: (fn: (uuid: string, api: unknown) => void) =>
    fn('p-1', { _getEventHandlers: () => [handler] }),
}))
vi.mock('./permissions', () => ({ effectivePermissions: () => ['ticket:read'] }))

import { initializeEventDispatcher, pluginEventsFor } from './eventDispatcher'

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

describe('initializeEventDispatcher', () => {
  // `prior` is the row this client held before a delete. It never came from
  // the server for this action, so a plugin doesn't get it.
  it('forwards an action without the client-only prior row', () => {
    const stop = initializeEventDispatcher()
    const action: SyncAction = {
      sync_id: 1,
      aggregate: 'ticket',
      aggregate_id: '7',
      op: 'U',
      event_type: 'ticket.updated',
      schema_version: 1,
      data: { id: 7, title: 'Printer' },
      groups: ['workspace:1'],
      prior: { id: 7, title: 'Printer', requester_uuid: 'u-1' },
    }
    notifySyncActions([action])
    stop()

    expect(handler).toHaveBeenCalledTimes(1)
    const forwarded = handler.mock.calls[0]![0] as Record<string, unknown>
    expect(forwarded).not.toHaveProperty('prior')
    expect(forwarded).toMatchObject({ event_type: 'ticket.updated', data: { id: 7, title: 'Printer' } })
  })
})

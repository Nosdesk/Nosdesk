import { beforeEach, describe, it, expect, vi } from 'vitest'
import { notifySyncActions } from '@nosdesk/core/sync/observers'
import type { SyncAction } from '@nosdesk/core/sync/types'

const handler = vi.hoisted(() => vi.fn())
// The loaded plugin's manifest; each test sets what it subscribes to.
const manifest = vi.hoisted(() => ({ value: {} as { events?: string[]; subscriptions?: string[] } }))
vi.mock('./loader', () => ({
  getLoadedPlugin: () => ({ plugin: { trust_level: 'verified', manifest: manifest.value } }),
}))
vi.mock('./pluginInstances', () => ({
  forEachLiveInstance: (fn: (uuid: string, api: unknown) => void) =>
    fn('p-1', { _getEventHandlers: () => [handler] }),
}))
vi.mock('./permissions', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./permissions')>()),
  effectivePermissions: () => ['ticket:read'],
}))

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

function updated(): SyncAction {
  return {
    sync_id: 1,
    aggregate: 'ticket',
    aggregate_id: '7',
    op: 'U',
    event_type: 'ticket.updated',
    schema_version: 1,
    data: { id: 7, title: 'Printer' },
    groups: ['workspace:1'],
  }
}

/** Dispatch one action and return how many times the handler heard it. */
function dispatch(action: SyncAction): number {
  const stop = initializeEventDispatcher()
  notifySyncActions([action])
  stop()
  return handler.mock.calls.length
}

describe('initializeEventDispatcher', () => {
  beforeEach(() => {
    handler.mockClear()
    manifest.value = { events: [], subscriptions: ['ticket:updated'] }
  })

  // `prior` is the row this client held before a delete. It never came from
  // the server for this action, so a plugin doesn't get it.
  it('forwards an action without the client-only prior row', () => {
    const action: SyncAction = { ...updated(), prior: { id: 7, title: 'Printer', requester_uuid: 'u-1' } }
    expect(dispatch(action)).toBe(1)
    const forwarded = handler.mock.calls[0]![0] as Record<string, unknown>
    expect(forwarded).not.toHaveProperty('prior')
    expect(forwarded).toMatchObject({ event_type: 'ticket.updated', data: { id: 7, title: 'Printer' } })
  })

  it('skips a plugin whose manifest does not subscribe to the event', () => {
    manifest.value = { events: [], subscriptions: ['ticket:created'] }
    expect(dispatch(updated())).toBe(0)
  })

  // The events a plugin emits aren't the events it hears.
  it('reads an empty subscription list as none, whatever the plugin emits', () => {
    manifest.value = { events: ['p:updated'], subscriptions: [] }
    expect(dispatch(updated())).toBe(0)
  })

  // A manifest from before `subscriptions` heard any host event.
  it('dispatches any host event to a manifest without subscriptions', () => {
    manifest.value = { events: [] }
    expect(dispatch(updated())).toBe(1)
  })
})

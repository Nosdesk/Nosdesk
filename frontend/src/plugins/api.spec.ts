/**
 * An attachment row keeps its stored `/uploads/...` path, which the server
 * refuses. Plugins must be handed, and fetch from, the `/api/files/...` URL
 * that is actually served.
 */
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Plugin } from '@nosdesk/core/types/plugin'

vi.mock('@nosdesk/core/services/ticketService', () => ({
  getTicketById: vi.fn(async () => ({
    id: 12,
    comments: [
      {
        id: 3,
        attachments: [
          {
            id: 7,
            name: 'report.pdf',
            url: '/uploads/tickets/12/0190a5b2_report.pdf',
            mime_type: 'application/pdf',
            file_size: 4,
          },
        ],
      },
    ],
  })),
  getTickets: vi.fn(),
  addCommentToTicket: vi.fn(),
  updateTicket: vi.fn(),
  deleteTicket: vi.fn(),
}))
const emitEvent = vi.hoisted(() => vi.fn(async () => undefined))
vi.mock('@nosdesk/core/services/pluginService', () => ({ default: { emitEvent } }))
// The real store pulls in the router and every view.
vi.mock('@/stores/auth', () => ({ useAuthStore: vi.fn() }))

const { createPluginAPI } = await import('./api')

const plugin = {
  uuid: '0190a5b2-0000-7000-8000-000000000001',
  name: 'attachment-reader',
  consented_permissions: ['ticket:read'],
  manifest: { permissions: ['ticket:read'] },
} as unknown as Plugin

describe('plugin attachments', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('lists the served URL, not the stored path', async () => {
    const [attachment] = await createPluginAPI(plugin).attachments.list(12)
    expect(attachment.url).toBe('/api/files/tickets/12/0190a5b2_report.pdf')
  })

  it('fetches the content from the served URL', async () => {
    const fetchMock = vi.fn(async () => new Response('%PDF'))
    vi.stubGlobal('fetch', fetchMock)

    const content = await createPluginAPI(plugin).attachments.getContent(7, 12)

    expect(fetchMock).toHaveBeenCalledWith('/api/files/tickets/12/0190a5b2_report.pdf', {
      credentials: 'same-origin',
    })
    expect(content?.name).toBe('report.pdf')
  })
})

describe('plugin event subscriptions', () => {
  const withManifest = (manifest: Record<string, unknown>) =>
    ({ ...plugin, manifest: { permissions: ['ticket:read'], ...manifest } }) as unknown as Plugin

  it('registers a handler for an event the manifest subscribes to', () => {
    const api = createPluginAPI(withManifest({ events: [], subscriptions: ['ticket:updated'] }))
    const handler = vi.fn()
    const off = api.on('ticket:updated', handler)
    expect(api._getEventHandlers('ticket:updated')).toEqual([handler])
    off()
    expect(api._getEventHandlers('ticket:updated')).toEqual([])
  })

  it('refuses a host event outside the subscriptions the manifest lists', () => {
    const api = createPluginAPI(withManifest({ events: [], subscriptions: ['ticket:updated'] }))
    expect(() => api.on('ticket:created', vi.fn())).toThrow(/denied/)
    expect(api._getEventHandlers('ticket:created')).toEqual([])
  })

  // As before the list existed; the plugin frame warns instead.
  it('accepts any host event when the manifest lists no subscriptions', () => {
    const api = createPluginAPI(withManifest({ events: ['attachment-reader:scanned'] }))
    const handler = vi.fn()
    api.on('ticket:created', handler)
    expect(api._getEventHandlers('ticket:created')).toEqual([handler])
  })
})

describe('plugin event emits', () => {
  const withEvents = (events: string[]) =>
    ({ ...plugin, manifest: { permissions: ['ticket:read'], events } }) as unknown as Plugin

  afterEach(() => {
    emitEvent.mockClear()
  })

  it('records an event the manifest declares, as the plugin', async () => {
    const api = createPluginAPI(withEvents(['attachment-reader:scanned']))
    await api.emit('attachment-reader:scanned', { ticket_id: 12 })
    expect(emitEvent).toHaveBeenCalledWith(plugin.uuid, 'attachment-reader:scanned', { ticket_id: 12 })
  })

  it('refuses a name the manifest does not declare', async () => {
    const api = createPluginAPI(withEvents(['attachment-reader:scanned']))
    for (const name of ['attachment-reader:deleted', 'ticket:created', 'other:scanned']) {
      await expect(api.emit(name, {})).rejects.toThrow(/denied/)
    }
    expect(emitEvent).not.toHaveBeenCalled()
  })
})

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

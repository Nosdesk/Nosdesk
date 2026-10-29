import { describe, expect, it } from 'vitest'
import type { RouteLocation } from 'vue-router'
import {
  isAllowedPdfPath,
  legacyPdfViewerRedirect,
  pdfNameFromPath,
  pdfPathFromSource,
  ticketIdFromPdfPath,
} from '@/utils/pdfSource'

const stored = '/uploads/tickets/7/019eb4e2-dbaa-75e5-9eb2-aa3dc7d8a7cb_invoice.pdf'

describe('isAllowedPdfPath', () => {
  it.each([
    stored,
    '/api/files/tickets/7/019eb4e2-dbaa-75e5-9eb2-aa3dc7d8a7cb_invoice.pdf',
    '/uploads/temp/019eb4e2-dbaa-75e5-9eb2-aa3dc7d8a7cb_draft.pdf',
  ])('opens a ticket or upload file: %s', (path) => {
    expect(isAllowedPdfPath(path)).toBe(true)
  })

  it.each([
    'https://evil.example/x.pdf',
    '//evil.example/x.pdf',
    'javascript:alert(1)',
    '/uploads/tickets/7/../../../etc/passwd',
    '/uploads/tickets/7/..',
    '/uploads/tickets/abc/x.pdf',
    '/api/users/me',
    '/uploads/tickets/7/a\\b.pdf',
    ['/uploads/tickets/7/x.pdf'],
    undefined,
  ])('refuses anything else: %s', (path) => {
    expect(isAllowedPdfPath(path)).toBe(false)
  })
})

describe('pdfNameFromPath', () => {
  it('drops the upload uuid from the stored name', () => {
    expect(pdfNameFromPath(stored)).toBe('invoice.pdf')
  })

  it('decodes an encoded name, and keeps a literal "%"', () => {
    expect(pdfNameFromPath('/uploads/tickets/7/019eb4e2-dbaa-75e5-9eb2-aa3dc7d8a7cb_50%25%20off.pdf')).toBe('50% off.pdf')
    expect(pdfNameFromPath('/uploads/tickets/7/019eb4e2-dbaa-75e5-9eb2-aa3dc7d8a7cb_50% off.pdf')).toBe('50% off.pdf')
  })
})

describe('pdfPathFromSource', () => {
  it('finds the stored path behind the viewer sources the app has used', () => {
    expect(pdfPathFromSource('/api/files/tickets/7/a.pdf')).toBe('/api/files/tickets/7/a.pdf')
    expect(pdfPathFromSource('nosdesk-asset://localhost/api/files/tickets/7/a.pdf')).toBe('/api/files/tickets/7/a.pdf')
    expect(pdfPathFromSource('https://desk.example/uploads/tickets/7/a.pdf?x=1')).toBe('/uploads/tickets/7/a.pdf')
    expect(pdfPathFromSource('blob:https://desk.example/123')).toBeNull()
    expect(ticketIdFromPdfPath('/api/files/tickets/7/a.pdf')).toBe(7)
    expect(ticketIdFromPdfPath('/uploads/temp/a.pdf')).toBeNull()
  })
})

describe('legacyPdfViewerRedirect', () => {
  const to = (query: Record<string, string>, params: Record<string, string> = {}) =>
    ({ query, params }) as unknown as RouteLocation

  it("opens a ticket's PDF over its ticket, on the same page", () => {
    expect(legacyPdfViewerRedirect(to({ src: '/api/files/tickets/7/a.pdf', filename: 'a.pdf', page: '3' }, { workspace: 'acme' }))).toEqual({
      name: 'ticket-by-id',
      params: { workspace: 'acme', id: '7' },
      query: { pdf: '/api/files/tickets/7/a.pdf', page: '3' },
    })
  })

  it('opens an upload with no ticket over the home page, and anything else goes home', () => {
    expect(legacyPdfViewerRedirect(to({ src: '/api/files/temp/a.pdf' }))).toEqual({
      name: 'home',
      params: {},
      query: { pdf: '/api/files/temp/a.pdf' },
    })
    expect(legacyPdfViewerRedirect(to({ src: 'https://evil.example/x.pdf' }))).toEqual({ name: 'home', params: {} })
  })
})

import { describe, expect, it, vi } from 'vitest'

const api = vi.hoisted(() => ({ get: vi.fn() }))
vi.mock('@nosdesk/core/apiClient', () => ({ default: api }))

import {
  DocumentationExportError,
  downloadDocumentationExport,
  fallbackExportName,
  fileNameFromDisposition,
} from './markdownExportService'

describe('fileNameFromDisposition', () => {
  it('reads a quoted file name', () => {
    expect(fileNameFromDisposition('attachment; filename="nosdesk-docs-2026-10-19.zip"')).toBe(
      'nosdesk-docs-2026-10-19.zip'
    )
  })

  it('prefers the encoded name', () => {
    expect(
      fileNameFromDisposition(`attachment; filename="docs.zip"; filename*=UTF-8''d%C3%B6cs.zip`)
    ).toBe('döcs.zip')
  })

  it('reads an unquoted name', () => {
    expect(fileNameFromDisposition('attachment; filename=docs.zip')).toBe('docs.zip')
  })

  it('is null without a header', () => {
    expect(fileNameFromDisposition(undefined)).toBeNull()
    expect(fileNameFromDisposition('inline')).toBeNull()
  })
})

describe('downloadDocumentationExport', () => {
  it('saves under the name the server gives and returns it', async () => {
    api.get.mockResolvedValueOnce({
      data: new Blob(['zip']),
      headers: { 'content-disposition': 'attachment; filename="acme-documentation-2026-10-22.zip"' },
    })
    URL.createObjectURL = vi.fn(() => 'blob:docs')
    URL.revokeObjectURL = vi.fn()
    const clicked: string[] = []
    const click = vi
      .spyOn(HTMLAnchorElement.prototype, 'click')
      .mockImplementation(function (this: HTMLAnchorElement) {
        clicked.push(this.download)
      })

    await expect(downloadDocumentationExport()).resolves.toBe('acme-documentation-2026-10-22.zip')
    expect(api.get).toHaveBeenCalledWith('/documentation/export.zip', { responseType: 'blob' })
    expect(clicked).toEqual(['acme-documentation-2026-10-22.zip'])
    click.mockRestore()
  })

  it('names a refusal apart from any other failure', async () => {
    api.get.mockRejectedValueOnce(Object.assign(new Error('Forbidden'), { response: { status: 403 } }))
    await expect(downloadDocumentationExport()).rejects.toMatchObject({ reason: 'forbidden' })
    api.get.mockRejectedValueOnce(Object.assign(new Error('Boom'), { response: { status: 500 } }))
    const failure = await downloadDocumentationExport().catch((e: unknown) => e)
    expect(failure).toBeInstanceOf(DocumentationExportError)
    expect(failure).toMatchObject({ reason: 'failed' })
  })

  it('falls back to a dated name', () => {
    expect(fallbackExportName(new Date('2026-10-22T05:00:00Z'))).toBe('documentation-2026-10-22.zip')
  })
})

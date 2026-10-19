import { describe, expect, it } from 'vitest'
import { fileNameFromDisposition } from './markdownExportService'

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

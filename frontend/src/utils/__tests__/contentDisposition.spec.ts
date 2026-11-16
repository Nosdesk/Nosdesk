import { describe, expect, it } from 'vitest'
import { fileNameFromDisposition } from '@nosdesk/core/utils/contentDisposition'

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

  it('reads the exact name the server adds when the plain one lost characters', () => {
    expect(
      fileNameFromDisposition(`inline; filename="R_sum_.pdf"; filename*=UTF-8''R%C3%A9sum%C3%A9.pdf`)
    ).toBe('Résumé.pdf')
  })

  it('is null without a header', () => {
    expect(fileNameFromDisposition(undefined)).toBeNull()
    expect(fileNameFromDisposition('inline')).toBeNull()
  })
})

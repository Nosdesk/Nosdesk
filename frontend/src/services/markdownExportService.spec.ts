import { describe, expect, it } from 'vitest'
import { yamlString } from './markdownExportService'

describe('yamlString', () => {
  it('quotes a plain value', () => {
    expect(yamlString('Getting started')).toBe('"Getting started"')
  })

  it('escapes double quotes', () => {
    expect(yamlString('a "b" c')).toBe('"a \\"b\\" c"')
  })

  // The original bug: quotes were escaped but backslashes were not, so the
  // escape character itself leaked through into the frontmatter.
  it('escapes backslashes before quotes', () => {
    expect(yamlString('a\\b')).toBe('"a\\\\b"')
    expect(yamlString('a\\"b')).toBe('"a\\\\\\"b"')
  })

  it('does not leave a trailing backslash unterminated', () => {
    const out = yamlString('ends with\\')
    expect(out).toBe('"ends with\\\\"')
    // An odd number of trailing backslashes would swallow the closing quote.
    expect(out.match(/\\*"$/)?.[0].length ?? 0).toBe(3)
  })

  it('drops control characters rather than emitting invalid escapes', () => {
    expect(yamlString(`a${String.fromCharCode(1)}b${String.fromCharCode(127)}c`)).toBe('"abc"')
  })

  it('tolerates empty and nullish input', () => {
    expect(yamlString('')).toBe('""')
    expect(yamlString(undefined as unknown as string)).toBe('""')
  })
})

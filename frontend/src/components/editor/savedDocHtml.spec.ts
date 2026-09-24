import { describe, expect, it } from 'vitest'
import * as Y from 'yjs'
import { savedDocToHtml } from './savedDocHtml'

function page(build: (fragment: Y.XmlFragment) => void): Uint8Array {
  const ydoc = new Y.Doc()
  build(ydoc.getXmlFragment('prosemirror'))
  return Y.encodeStateAsUpdate(ydoc)
}

function paragraph(text: string): Y.XmlElement {
  const p = new Y.XmlElement('paragraph')
  p.insert(0, [new Y.XmlText(text)])
  return p
}

describe('savedDocToHtml', () => {
  it('renders the saved content through the editor schema', () => {
    const html = savedDocToHtml(page((f) => f.insert(0, [paragraph('Restart the VPN')])))
    expect(html).toContain('<p>Restart the VPN</p>')
  })

  it('leaves out omitted node types, so a public page never shows a private embed', () => {
    const update = page((f) => {
      const embed = new Y.XmlElement('embedded_document')
      embed.setAttribute('documentUuid', '00000000-0000-0000-0000-000000000001')
      embed.setAttribute('documentTitle', 'Private runbook')
      f.insert(0, [paragraph('Public intro'), embed])
    })
    expect(savedDocToHtml(update)).toContain('Private runbook')
    const publicHtml = savedDocToHtml(update, { omit: ['embedded_document', 'image'] })
    expect(publicHtml).toContain('Public intro')
    expect(publicHtml).not.toContain('Private runbook')
  })

  it('returns an empty string for an empty page', () => {
    expect(savedDocToHtml(page(() => {}))).toBe('')
  })
})

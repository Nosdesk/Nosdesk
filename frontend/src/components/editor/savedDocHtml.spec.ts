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

  it('renders a table with its scroll wrapper, column widths, spans and alignment', () => {
    // Attributes as y-prosemirror stores them: numbers, a number array, a string.
    const cell = (type: string, text: string, attrs: Record<string, unknown> = {}) => {
      const el = new Y.XmlElement(type)
      el.setAttribute('colspan', 1 as never)
      el.setAttribute('rowspan', 1 as never)
      for (const [key, value] of Object.entries(attrs)) el.setAttribute(key, value as never)
      el.insert(0, [paragraph(text)])
      return el
    }
    const row = (...cells: Y.XmlElement[]) => {
      const el = new Y.XmlElement('table_row')
      el.insert(0, cells)
      return el
    }
    const update = page((f) => {
      const table = new Y.XmlElement('table')
      table.insert(0, [
        row(cell('table_header', 'Name', { colwidth: [160] }), cell('table_header', 'Owner', { align: 'center' })),
        row(cell('table_cell', 'VPN', { colspan: 2, align: 'right' })),
      ])
      f.insert(0, [table, paragraph('After')])
    })
    expect(savedDocToHtml(update)).toBe(
      '<div class="table-scroll"><table><colgroup><col width="160"><col></colgroup><tbody>' +
        '<tr><th><p>Name</p></th><th class="align-center"><p>Owner</p></th></tr>' +
        '<tr><td colspan="2" class="align-right"><p>VPN</p></td></tr>' +
        '</tbody></table></div><p>After</p>',
    )
  })

  it('returns an empty string for an empty page', () => {
    expect(savedDocToHtml(page(() => {}))).toBe('')
  })
})

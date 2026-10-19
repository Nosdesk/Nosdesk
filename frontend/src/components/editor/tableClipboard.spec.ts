import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { EditorState, TextSelection } from 'prosemirror-state'
import { EditorView } from 'prosemirror-view'
import type { Node } from 'prosemirror-model'
import { CellSelection, TableMap } from 'prosemirror-tables'
import { schema } from './schema'
import { createTableGuardPlugin } from './tableGuard'
import { allowTables, resetTablesAllowed } from './tablesAllowed'
import { createTablePlugins } from './tablePlugins'
import { createImageUploadPlugin } from './imageUploadPlugin'
import { gfmTable, tidyPastedTableHtml, tsvTable } from './tableClipboard'
import { CELL_SEPARATOR } from './tableText'

// Tables pasted from the places people copy them, and cells copied out.

const upload = vi.hoisted(() => vi.fn())
vi.mock('@/services/editorImageService', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/services/editorImageService')>()),
  uploadEditorImage: upload,
}))

const DOC_ID = 'ws-1_doc-1'

let views: EditorView[] = []
beforeEach(() => {
  allowTables(DOC_ID)
  upload.mockReset()
  upload.mockResolvedValue({ url: '/uploads/pasted.png', name: 'pasted.png' })
})
afterEach(() => {
  views.forEach((v) => v.destroy())
  views = []
  resetTablesAllowed()
})

const p = (text = '') => schema.nodes.paragraph.create(null, text ? schema.text(text) : null)
const td = (text: string) => schema.nodes.table_cell.create(null, p(text))
const th = (text: string) => schema.nodes.table_header.create(null, p(text))
const tr = (...cells: Node[]) => schema.nodes.table_row.create(null, cells)
const table = (...rows: Node[]) => schema.nodes.table.create(null, rows)
const doc = (...blocks: Node[]) => schema.nodes.doc.create(null, blocks)

function open(start: Node = doc(p('Intro'), p())): EditorView {
  const tables = createTablePlugins({ docId: () => DOC_ID, resizable: false })
  const view = new EditorView(document.createElement('div'), {
    state: EditorState.create({
      doc: start,
      plugins: [
        createTableGuardPlugin(() => DOC_ID),
        tables.keymap,
        createImageUploadPlugin({ docId: DOC_ID, uploadingLabel: () => '' }),
        ...tables.plugins,
      ],
    }),
  })
  views.push(view)
  return view
}

/** A paste event as the browser sends it (jsdom has no ClipboardEvent). */
function paste(view: EditorView, data: { html?: string; text?: string; files?: File[] }, shift = false) {
  // ProseMirror reads Shift from the last key event; plain-text paste is Shift.
  ;(view as unknown as { input: { shiftKey: boolean } }).input.shiftKey = shift
  const event = new Event('paste', { bubbles: true, cancelable: true })
  Object.defineProperty(event, 'clipboardData', {
    value: {
      types: [...(data.html ? ['text/html'] : []), ...(data.text ? ['text/plain'] : [])],
      files: data.files ?? [],
      items: [],
      getData: (type: string) => (type === 'text/html' ? data.html : type === 'text/plain' ? data.text : '') ?? '',
    },
  })
  view.dom.dispatchEvent(event)
}

function caretInLastParagraph(view: EditorView) {
  view.dispatch(view.state.tr.setSelection(TextSelection.atEnd(view.state.doc)))
}

/** The first table: each row's cells as text, header cells marked `#`. */
function grid(view: EditorView): string[][] | null {
  let found: Node | null = null
  view.state.doc.forEach((node) => {
    if (!found && node.type === schema.nodes.table) found = node
  })
  if (!found) return null
  const out: string[][] = []
  ;(found as Node).forEach((row) => {
    const cells: string[] = []
    row.forEach((c) => cells.push((c.type === schema.nodes.table_header ? '#' : '') + c.textContent))
    out.push(cells)
  })
  return out
}

function firstTable(view: EditorView): Node {
  let found: Node | null = null
  view.state.doc.forEach((node) => {
    if (!found && node.type === schema.nodes.table) found = node
  })
  return found!
}

const SHEETS =
  '<meta charset="utf-8"><google-sheets-html-origin><style type="text/css"><!--td {border: 1px solid #cccccc;}br {mso-data-placement:same-cell;}--></style>' +
  '<table xmlns="http://www.w3.org/1999/xhtml" cellspacing="0" cellpadding="0" dir="ltr" border="1" style="table-layout:fixed;font-size:10pt;font-family:Arial;width:0px;border-collapse:collapse;border:none" data-sheets-root="1">' +
  '<colgroup><col width="100"><col width="120"></colgroup><tbody>' +
  '<tr style="height:21px;"><td style="overflow:hidden;padding:2px 3px 2px 3px;vertical-align:bottom;font-weight:bold;">Name</td><td style="overflow:hidden;padding:2px 3px 2px 3px;vertical-align:bottom;font-weight:bold;">Owner</td></tr>' +
  '<tr style="height:21px;"><td style="overflow:hidden;padding:2px 3px 2px 3px;vertical-align:bottom;">VPN</td><td style="overflow:hidden;padding:2px 3px 2px 3px;vertical-align:bottom;"></td></tr>' +
  '</tbody></table></google-sheets-html-origin>'

const EXCEL =
  '<html xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office" xmlns:x="urn:schemas-microsoft-com:office:excel" xmlns="http://www.w3.org/TR/REC-html40">' +
  '<head><meta name=ProgId content=Excel.Sheet><meta name=Generator content="Microsoft Excel 15"><style><!--table {mso-displayed-decimal-separator:"\\.";} .xl65 {font-weight:700;}--></style></head>' +
  '<body link="#0563C1" vlink="#954F72"><table border=0 cellpadding=0 cellspacing=0 width=128 style=\'border-collapse:collapse;width:96pt\'>' +
  '<!--StartFragment--><col width=64 span=2 style=\'width:48pt\'>' +
  '<tr height=20 style=\'height:15.0pt\'><td height=20 class=xl65 width=64 style=\'height:15.0pt;width:48pt\'>Host</td><td class=xl65 width=64 style=\'width:48pt\'>Port</td></tr>' +
  '<tr height=20 style=\'height:15.0pt\'><td height=20 style=\'height:15.0pt\'>db1</td><td align=right>5432</td></tr>' +
  '<!--EndFragment--></table></body></html>'

const CONFLUENCE =
  '<div class="table-wrap"><table class="confluenceTable"><colgroup><col><col><col></colgroup><tbody>' +
  '<tr><th class="confluenceTh"><p>System</p></th><th class="confluenceTh"><p>Owner</p></th><th class="confluenceTh"><p>Notes</p></th></tr>' +
  '<tr><td class="confluenceTd"><p>VPN</p><ul><li>primary</li><li>backup</li></ul></td><td class="confluenceTd"><p><strong>Ana</strong></p></td>' +
  '<td class="confluenceTd" rowspan="2"><pre>restart.sh</pre></td></tr>' +
  '<tr><td class="confluenceTd" colspan="2"><p>Shared</p></td></tr>' +
  '</tbody></table></div>'

const NOTION =
  '<meta charset="utf-8"><table class="simple-table"><tbody>' +
  '<tr id="a1"><th class="simple-table-header-color simple-table-header">Host</th><td>db1</td></tr>' +
  '<tr id="a2"><th class="simple-table-header-color simple-table-header">Port</th><td><code>5432</code></td></tr>' +
  '</tbody></table>'

describe('pasting HTML tables', () => {
  it('from Google Sheets', () => {
    const view = open()
    caretInLastParagraph(view)
    paste(view, { html: SHEETS, text: 'Name\tOwner\nVPN\t' })
    expect(grid(view)).toEqual([
      ['Name', 'Owner'],
      ['VPN', ''],
    ])
    view.state.doc.check()
  })

  it('from Excel, keeping cell alignment', () => {
    const view = open()
    caretInLastParagraph(view)
    paste(view, { html: EXCEL, text: 'Host\tPort\ndb1\t5432\n' })
    expect(grid(view)).toEqual([
      ['Host', 'Port'],
      ['db1', '5432'],
    ])
    expect(firstTable(view).child(1).child(1).attrs.align).toBe('right')
  })

  it('from Excel with the picture of the range it also copies', () => {
    const view = open()
    caretInLastParagraph(view)
    const picture = new File([new Uint8Array([137, 80, 78, 71])], 'image.png', { type: 'image/png' })
    paste(view, { html: EXCEL, text: 'Host\tPort\ndb1\t5432\n', files: [picture] })
    expect(grid(view)).toEqual([
      ['Host', 'Port'],
      ['db1', '5432'],
    ])
    expect(upload).not.toHaveBeenCalled()
  })

  it('leaves a dataURL image paste without a table to the uploader, as before', () => {
    const view = open()
    caretInLastParagraph(view)
    paste(view, { html: '<p>Logo <img src="data:image/png;base64,iVBORw0KGgo=" alt="logo"></p>' })
    expect(upload).toHaveBeenCalledTimes(1)
    expect(view.state.doc.textContent).toBe('Intro')
  })

  it('with a dataURL image in a cell, uploading the image and keeping the table', async () => {
    const view = open()
    caretInLastParagraph(view)
    paste(view, {
      html:
        '<table><tr><th>Name</th><th>Logo</th></tr>' +
        '<tr><td>VPN</td><td><img src="data:image/png;base64,iVBORw0KGgo=" alt="logo"></td></tr></table>',
    })
    expect(grid(view)).toEqual([
      ['#Name', '#Logo'],
      ['VPN', ''],
    ])
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(upload).toHaveBeenCalledTimes(1)
    const sources: string[] = []
    view.state.doc.descendants((node) => {
      if (node.type === schema.nodes.image) sources.push(node.attrs.src as string)
    })
    expect(sources).toEqual(['/uploads/pasted.png'])
    expect(grid(view)).toHaveLength(2)
  })

  it('from Confluence, keeping headers, spans, lists and code', () => {
    const view = open()
    caretInLastParagraph(view)
    paste(view, { html: CONFLUENCE })
    const t = firstTable(view)
    t.check()
    expect(grid(view)![0]).toEqual(['#System', '#Owner', '#Notes'])
    expect(t.child(1).child(0).child(1).type).toBe(schema.nodes.bullet_list)
    expect(t.child(1).child(1).firstChild!.firstChild!.marks.map((m) => m.type.name)).toEqual(['strong'])
    expect(t.child(1).child(2).attrs.rowspan).toBe(2)
    expect(t.child(1).child(2).firstChild!.type).toBe(schema.nodes.code_block)
    expect(t.child(2).child(0).attrs.colspan).toBe(2)
    expect(TableMap.get(t).problems ?? null).toBeNull()
  })

  it('keeps a table whole around headings, quotes, rules and nested tables in its cells', () => {
    const view = open()
    caretInLastParagraph(view)
    paste(view, {
      html:
        '<table><tr><th><h3>Runbook</h3></th><td><blockquote><p>quoted</p></blockquote><hr><p>after</p></td></tr>' +
        '<tr><td><table><tr><td>in</td><td>ner</td></tr></table></td><td>end</td></tr></table>',
    })
    const t = firstTable(view)
    expect(view.state.doc.toJSON().content.map((n: { type: string }) => n.type)).toEqual(['paragraph', 'table'])
    expect(grid(view)).toEqual([
      ['#Runbook', 'quotedafter'],
      [`in${CELL_SEPARATOR}ner`, 'end'],
    ])
    expect(t.child(0).child(0).firstChild!.firstChild!.marks.map((m) => m.type.name)).toEqual(['strong'])
  })

  it('leaves HTML that needs no tidying untouched', () => {
    expect(tidyPastedTableHtml(NOTION)).toBe(NOTION)
  })

  it('from Notion, keeping a header column', () => {
    const view = open()
    caretInLastParagraph(view)
    paste(view, { html: NOTION })
    expect(grid(view)).toEqual([
      ['#Host', 'db1'],
      ['#Port', '5432'],
    ])
    expect(firstTable(view).child(1).child(1).firstChild!.firstChild!.marks.map((m) => m.type.name)).toEqual(['code'])
  })
})

describe('pasting text', () => {
  it('turns a GFM pipe table into a table with a header row, alignment and marks', () => {
    const view = open()
    caretInLastParagraph(view)
    paste(view, {
      text: '| Name | Owner | Port |\n|:-----|:-----:|-----:|\n| **VPN** | Ana \\| Bo | `51820` |\n| [Docs](https://example.com) | | |\n',
    })
    expect(grid(view)).toEqual([
      ['#Name', '#Owner', '#Port'],
      ['VPN', 'Ana | Bo', '51820'],
      ['Docs', '', ''],
    ])
    const t = firstTable(view)
    expect([0, 1, 2].map((c) => t.child(1).child(c).attrs.align)).toEqual(['left', 'center', 'right'])
    const marks = (r: number, c: number) => t.child(r).child(c).firstChild!.firstChild!.marks
    expect(marks(1, 0).map((m) => m.type.name)).toEqual(['strong'])
    expect(marks(1, 2).map((m) => m.type.name)).toEqual(['code'])
    expect(marks(2, 0)[0]!.attrs.href).toBe('https://example.com')
  })

  it('turns tab-separated lines into a table', () => {
    const view = open()
    caretInLastParagraph(view)
    paste(view, { text: 'Host\tPort\r\ndb1\t5432\r\ndb2\t\r\n' })
    expect(grid(view)).toEqual([
      ['Host', 'Port'],
      ['db1', '5432'],
      ['db2', ''],
    ])
  })

  it.each([
    ['one tab-separated line', 'Host\tPort'],
    ['lines with different tab counts', 'a\tb\nc\td\te'],
    ['lines with no tabs', 'one\ntwo'],
    ['a pipe in prose', 'use a | b to pipe'],
  ])('leaves %s as text', (_name, text) => {
    const view = open()
    caretInLastParagraph(view)
    paste(view, { text })
    expect(grid(view)).toBeNull()
  })

  it('leaves text as text on a plain-text paste (Shift)', () => {
    const view = open()
    caretInLastParagraph(view)
    paste(view, { text: 'Host\tPort\ndb1\t5432' }, true)
    expect(grid(view)).toBeNull()
  })

  it('leaves text as text where tables are not allowed', () => {
    resetTablesAllowed()
    const view = open()
    caretInLastParagraph(view)
    paste(view, { text: 'Host\tPort\ndb1\t5432' })
    paste(view, { text: '| a | b |\n|---|---|\n| c | d |' })
    expect(grid(view)).toBeNull()
    expect(view.state.doc.textContent).toContain('db1')
  })

  it('parses only whole tables', () => {
    expect(gfmTable(schema, 'Intro\n\n| a | b |\n|---|---|\n| c | d |')).toBeNull()
    expect(tsvTable(schema, 'a\tb\n\nc\td')).toBeNull()
    expect(gfmTable(schema, '| a | b |\n|---|---|')!.childCount).toBe(1)
  })

  it('does not link unsafe URLs', () => {
    const t = gfmTable(schema, '| a | b |\n|---|---|\n| [x](javascript:alert(1)) | y |')!
    expect(t.child(1).child(0).firstChild!.firstChild!.marks).toEqual([])
  })
})

describe('pasting cells into a table', () => {
  it('fills from the caret and grows the table', () => {
    const view = open(doc(table(tr(th('Name'), th('Owner')), tr(td('VPN'), td('Ana'))), p()))
    // Caret in the cell holding "Ana".
    const pos = 1 + view.state.doc.child(0).child(0).nodeSize + 1 + view.state.doc.child(0).child(1).child(0).nodeSize + 2
    view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, pos)))
    paste(view, { text: 'x1\tx2\ny1\ty2' })
    expect(grid(view)).toEqual([
      ['#Name', '#Owner', '#'],
      ['VPN', 'x1', 'x2'],
      ['', 'y1', 'y2'],
    ])
    expect(view.state.doc.childCount).toBe(2)
  })
})

describe('copying cells', () => {
  it('writes an HTML table and tab-separated text', () => {
    const view = open(doc(table(tr(th('Name'), th('Owner'), th('Port')), tr(td('VPN'), td('Ana'), td('51820'))), p()))
    const cellPos = (row: number, col: number) => {
      const t = view.state.doc.child(0)
      const map = TableMap.get(t)
      return 1 + map.map[row * map.width + col]!
    }
    view.dispatch(view.state.tr.setSelection(CellSelection.create(view.state.doc, cellPos(0, 0), cellPos(1, 1))))
    const { dom, text } = view.serializeForClipboard(view.state.selection.content())
    expect(text).toBe('Name\tOwner\nVPN\tAna')
    expect(dom.querySelector('table')).not.toBeNull()
    expect(dom.querySelectorAll('th')).toHaveLength(2)
    expect(dom.querySelectorAll('td')).toHaveLength(2)
  })

  it('leaves the slots a merged cell covers empty', () => {
    const merged = schema.nodes.table_cell.create({ colspan: 2 }, p('wide'))
    const view = open(doc(table(tr(merged), tr(td('a'), td('b'))), p()))
    view.dispatch(view.state.tr.setSelection(CellSelection.create(view.state.doc, 2, 2 + merged.nodeSize + 2 + td('a').nodeSize)))
    const { text } = view.serializeForClipboard(view.state.selection.content())
    expect(text).toBe('wide\t\na\tb')
  })

  it('leaves a text selection to the default', () => {
    const view = open(doc(p('Plain words'), p()))
    view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, 1, 6)))
    expect(view.serializeForClipboard(view.state.selection.content()).text).toBe('Plain')
  })
})

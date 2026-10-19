import { afterEach, describe, expect, it } from 'vitest'
import { DOMParser, Fragment, Node, Slice } from 'prosemirror-model'
import { EditorState, Plugin, TextSelection } from 'prosemirror-state'
import { EditorView } from 'prosemirror-view'
import { ySyncPluginKey } from 'y-prosemirror'
import { commentSchema, schema } from './schema'
import { createTableGuardPlugin } from './tableGuard'
import { allowTables, resetTablesAllowed, tablesAllowed } from './tablesAllowed'
import { CELL_SEPARATOR, tablesToTextHtml } from './tableText'

// Until a document's server allows tables (its hello, wired in the
// collaboration session), no local change may add one; remote changes pass.

const DOC_ID = 'ws-1_doc-1'

let views: EditorView[] = []
afterEach(() => {
  views.forEach((v) => v.destroy())
  views = []
  resetTablesAllowed()
})

function open(plugins: Plugin[] = []): EditorView {
  const start = schema.node('doc', null, [schema.node('paragraph', null, [schema.text('Intro')])])
  const view = new EditorView(document.createElement('div'), {
    state: EditorState.create({ doc: start, plugins: [createTableGuardPlugin(() => DOC_ID), ...plugins] }),
  })
  views.push(view)
  return view
}

// jsdom has no ClipboardEvent; the paste path only needs an event to hand on.
const paste = (view: EditorView, html: string) => view.pasteHTML(html, new Event('paste') as ClipboardEvent)

// Editor HTML for two cells copied from a row: no <table> element, but the
// slice's context rebuilds the table around them.
const PM_CELLS_HTML =
  '<td data-pm-slice="0 0 [&quot;table&quot;,null,&quot;table_row&quot;,null]"><p>left</p></td><td><p>right</p></td>'

const newTable = () =>
  schema.nodes.table.create(null, [
    schema.nodes.table_row.create(null, [
      schema.nodes.table_header.createAndFill()!,
      schema.nodes.table_header.createAndFill()!,
    ]),
  ])

const hasTable = (doc: Node) => {
  let found = false
  doc.descendants((node) => {
    if (node.type.spec.tableRole) found = true
    return !found
  })
  return found
}

/** Dispatch and report whether the document changed. */
function applies(view: EditorView, build: (state: EditorState) => EditorState['tr']): boolean {
  const before = view.state.doc
  view.dispatch(build(view.state))
  return !view.state.doc.eq(before)
}

const entryPaths: [string, (state: EditorState) => EditorState['tr']][] = [
  ['an insert command', (s) => s.tr.insert(s.doc.content.size, newTable())],
  ['a replace of the selection', (s) => s.tr.replaceSelectionWith(newTable())],
  [
    'a paste-shaped slice',
    (s) => s.tr.replaceSelection(new Slice(Fragment.from(newTable()), 0, 0)).setMeta('uiEvent', 'paste'),
  ],
  [
    'a drop-shaped slice',
    (s) => s.tr.replaceRange(1, 1, new Slice(Fragment.from(newTable()), 0, 0)).setMeta('uiEvent', 'drop'),
  ],
  [
    'a whole-document replace (revision restore, setContent)',
    (s) => s.tr.replaceWith(0, s.doc.content.size, [newTable(), schema.node('paragraph')]),
  ],
  [
    'wrapping a paragraph into a table',
    (s) => {
      const range = s.doc.resolve(1).blockRange()!
      return s.tr.wrap(range, [
        { type: schema.nodes.table },
        { type: schema.nodes.table_row },
        { type: schema.nodes.table_cell },
      ])
    },
  ],
  [
    'a row inserted on its own',
    (s) => s.tr.insert(0, schema.nodes.table_row.create(null, [schema.nodes.table_cell.createAndFill()!])),
  ],
]

describe('the table guard while tables are not allowed', () => {
  it('starts with tables not allowed', () => {
    expect(tablesAllowed(DOC_ID)).toBe(false)
  })

  it.each(entryPaths)('rejects %s', (_name, build) => {
    const view = open()
    expect(applies(view, build)).toBe(false)
    expect(hasTable(view.state.doc)).toBe(false)
  })

  it("rejects another plugin's appended transaction that adds a table", () => {
    const adder = new Plugin({
      appendTransaction(_trs, _old, state) {
        if (hasTable(state.doc) || state.doc.textContent !== 'Intro!') return null
        return state.tr.insert(state.doc.content.size, newTable())
      },
    })
    const view = open([adder])
    view.dispatch(view.state.tr.insertText('!', 6))
    expect(view.state.doc.textContent).toBe('Intro!')
    expect(hasTable(view.state.doc)).toBe(false)
  })

  it('lets ordinary edits through', () => {
    const view = open()
    expect(applies(view, (s) => s.tr.insertText(' text', 6))).toBe(true)
    expect(applies(view, (s) => s.tr.insert(s.doc.content.size, schema.node('paragraph')))).toBe(true)
  })

  it('lets a remote y-sync change with a table through', () => {
    const view = open()
    expect(
      applies(view, (s) =>
        s.tr.insert(s.doc.content.size, newTable()).setMeta(ySyncPluginKey, { isChangeOrigin: true }),
      ),
    ).toBe(true)
    expect(hasTable(view.state.doc)).toBe(true)
    // Typing in a table that arrived remotely is not adding one.
    const cellText = view.state.doc.content.size - 4
    view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, cellText)))
    expect(applies(view, (s) => s.tr.insertText('x'))).toBe(true)
  })

  it('does not treat other y-sync metadata as remote', () => {
    const view = open()
    expect(
      applies(view, (s) =>
        s.tr.insert(s.doc.content.size, newTable()).setMeta(ySyncPluginKey, { snapshot: null, prevSnapshot: null }),
      ),
    ).toBe(false)
  })

  it('pastes an HTML table as text, not nothing', () => {
    const view = open()
    view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, 6)))
    paste(view, 
      '<table><tr><th>Name</th><th>Owner</th></tr><tr><td><strong>VPN</strong></td><td>Ana</td></tr>' +
        '<tr><td></td><td></td></tr><tr><td><ul><li>one</li></ul></td><td>two</td></tr></table>',
    )
    const doc = view.state.doc
    expect(hasTable(doc)).toBe(false)
    const paragraphs: string[] = []
    doc.descendants((node) => {
      if (node.type.name === 'paragraph') paragraphs.push(node.textContent)
      return node.type.name !== 'paragraph'
    })
    expect(paragraphs).toEqual(['Intro', `Name${CELL_SEPARATOR}Owner`, `VPN${CELL_SEPARATOR}Ana`, 'one', 'two'])
    expect(doc.toString()).toContain('strong("VPN")')
    expect(doc.toString()).toContain('bullet_list')
  })

  it('pastes cells copied from a table as text', () => {
    const view = open()
    paste(view, PM_CELLS_HTML)
    expect(hasTable(view.state.doc)).toBe(false)
    expect(view.state.doc.textContent).toContain(`left${CELL_SEPARATOR}right`)
  })
})

describe('the table guard once tables are allowed', () => {
  it.each(entryPaths.filter(([name]) => name !== 'a row inserted on its own'))('lets %s through', (_name, build) => {
    allowTables(DOC_ID)
    const view = open()
    expect(applies(view, build)).toBe(true)
    expect(hasTable(view.state.doc)).toBe(true)
  })

  it('pastes an HTML table as a table', () => {
    allowTables(DOC_ID)
    const view = open()
    paste(view, '<table><tr><th>Name</th></tr><tr><td>VPN</td></tr></table>')
    expect(hasTable(view.state.doc)).toBe(true)
  })

  it('rebuilds the table around cells copied from a row', () => {
    allowTables(DOC_ID)
    const view = open()
    paste(view, PM_CELLS_HTML)
    expect(hasTable(view.state.doc)).toBe(true)
  })

  it('allows one document, not every document', () => {
    allowTables('ws-1_doc-2')
    const view = open()
    expect(applies(view, entryPaths[0]![1])).toBe(false)
  })
})

describe('a table pasted into a comment', () => {
  const parse = (html: string) => {
    const div = document.createElement('div')
    div.innerHTML = tablesToTextHtml(html)
    return DOMParser.fromSchema(commentSchema).parse(div)
  }

  it('arrives as one paragraph per row, keeping marks', () => {
    const doc = parse(
      '<p>Before</p><table><thead><tr><th>Name</th><th>Owner</th></tr></thead>' +
        '<tbody><tr><td><p><em>VPN</em></p></td><td>Ana</td></tr><tr><td> </td><td></td></tr></tbody></table>',
    )
    doc.check()
    expect(doc.childCount).toBe(3)
    expect(doc.child(1).textContent).toBe(`Name${CELL_SEPARATOR}Owner`)
    expect(doc.child(2).textContent).toBe(`VPN${CELL_SEPARATOR}Ana`)
    expect(doc.child(2).firstChild!.marks.map((m) => m.type.name)).toEqual(['em'])
  })

  it('keeps block content in cells as blocks, and nested tables as text', () => {
    const doc = parse(
      '<table><tr><td><ul><li>one</li></ul></td><td>two</td></tr>' +
        '<tr><td><table><tr><td>in</td><td>ner</td></tr></table></td></tr></table>',
    )
    doc.check()
    expect(doc.toJSON().content.map((n: { type: string }) => n.type)).toEqual([
      'bullet_list',
      'paragraph',
      'paragraph',
    ])
    expect(doc.child(2).textContent).toBe(`in${CELL_SEPARATOR}ner`)
  })

  it('leaves HTML without a table untouched', () => {
    const html = '<p>No <strong>table</strong> here</p>'
    expect(tablesToTextHtml(html)).toBe(html)
  })
})

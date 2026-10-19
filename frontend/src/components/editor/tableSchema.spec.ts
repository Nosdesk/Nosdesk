import { describe, expect, it } from 'vitest'
import { DOMParser, DOMSerializer, Node } from 'prosemirror-model'
import { EDITOR_SCHEMA_VERSION, commentSchema, schema } from './schema'

// y-prosemirror deletes, from the shared document, any element it can't build
// into a node, so what the schema accepts and rejects is the contract.

type Json = Record<string, unknown>

const text = (t: string): Json => ({ type: 'text', text: t })
const para = (t?: string): Json => (t ? { type: 'paragraph', content: [text(t)] } : { type: 'paragraph' })
const cell = (content: Json[], attrs?: Json, type = 'table_cell'): Json => ({ type, attrs, content })
const header = (t: string, attrs?: Json) => cell([para(t)], attrs, 'table_header')
const td = (t: string, attrs?: Json) => cell([para(t)], attrs)
const row = (...cells: Json[]): Json => ({ type: 'table_row', content: cells })
const table = (...rows: Json[]): Json => ({ type: 'table', content: rows })
const doc = (...content: Json[]): Json => ({ type: 'doc', content })

const build = (json: Json) => {
  const node = Node.fromJSON(schema, json)
  node.check()
  return node
}

describe('the documentation schema', () => {
  it('is version 2', () => {
    expect(EDITOR_SCHEMA_VERSION).toBe(2)
  })

  it.each([
    ['a header row', doc(table(row(header('Name'), header('Owner')), row(td('VPN'), td('Ana'))))],
    ['a header column', doc(table(row(header('Host'), td('db1')), row(header('Port'), td('5432'))))],
    [
      'colspan and rowspan',
      doc(
        table(
          row(header('Merged', { colspan: 2 }), td('Tall', { rowspan: 2 })),
          row(td('a'), td('b')),
        ),
      ),
    ],
    ['column widths', doc(table(row(td('a', { colwidth: [120] }), td('b', { colspan: 2, colwidth: [80, 200] }))))],
    ['alignment', doc(table(row(td('l', { align: 'left' }), td('c', { align: 'center' }), td('r', { align: 'right' }))))],
    [
      'lists and code in cells',
      doc(
        table(
          row(
            cell([
              para('Steps'),
              { type: 'bullet_list', content: [{ type: 'list_item', content: [para('one')] }] },
              { type: 'ordered_list', content: [{ type: 'list_item', content: [para('two')] }] },
            ]),
            cell([{ type: 'code_block', attrs: { language: 'sh' }, content: [text('make dev')] }]),
          ),
        ),
      ),
    ],
    ['an empty paragraph cell beside other blocks', doc(para('Intro'), table(row(cell([para()]))), para('After'))],
    ['a ragged table (a row short a cell)', doc(table(row(td('a'), td('b')), row(td('c'))))],
    ['a table with no paragraph after it', doc(table(row(td('only'))))],
  ])('accepts %s', (_name, json) => {
    expect(() => build(json)).not.toThrow()
  })

  it.each([
    [
      'a table inside a list',
      doc({ type: 'bullet_list', content: [{ type: 'list_item', content: [para('x'), table(row(td('a')))] }] }),
    ],
    ['a table inside a quote', doc({ type: 'blockquote', content: [table(row(td('a')))] })],
    ['a table inside a cell', doc(table(row(cell([table(row(td('a')))]))))],
    ['a heading in a cell', doc(table(row(cell([{ type: 'heading', attrs: { level: 2 }, content: [text('H')] }]))))],
    ['a quote in a cell', doc(table(row(cell([{ type: 'blockquote', content: [para('q')] }]))))],
    ['an empty cell', doc(table(row(cell([]))))],
    ['a table with no rows', doc({ type: 'table' })],
    ['a cell outside a row', doc(table(td('a') as Json))],
    ['a colwidth that is not an array', doc(table(row(td('a', { colwidth: 120 }))))],
  ])('rejects %s', (_name, json) => {
    expect(() => build(json)).toThrow()
  })

  it('keeps an alignment value it does not know rather than rejecting the cell', () => {
    const node = build(doc(table(row(td('j', { align: 'justify' })))))
    const dom = DOMSerializer.fromSchema(schema).serializeFragment(node.content)
    const wrapper = document.createElement('div')
    wrapper.appendChild(dom)
    expect(wrapper.querySelector('td')!.getAttribute('class')).toBeNull()
  })

  it('renders a scroll wrapper, a colgroup with widths, spans and alignment', () => {
    const node = build(
      doc(
        table(
          row(header('Name', { colwidth: [150] }), header('Notes', { colspan: 2, align: 'center' })),
          row(td('a'), td('b'), td('c', { align: 'right' })),
        ),
      ),
    )
    const wrapper = document.createElement('div')
    wrapper.appendChild(DOMSerializer.fromSchema(schema).serializeFragment(node.content))
    expect(wrapper.innerHTML).toBe(
      '<div class="table-scroll"><table><colgroup><col width="150"><col><col></colgroup><tbody>' +
        '<tr><th data-colwidth="150"><p>Name</p></th><th colspan="2" class="align-center"><p>Notes</p></th></tr>' +
        '<tr><td><p>a</p></td><td><p>b</p></td><td class="align-right"><p>c</p></td></tr>' +
        '</tbody></table></div>',
    )
  })

  it('parses an HTML table: headers anywhere, spans, widths and alignment', () => {
    const html = document.createElement('div')
    html.innerHTML =
      '<table><tbody>' +
      '<tr><th>Host</th><td colspan="2" style="text-align: center">db1</td></tr>' +
      '<tr><th>Port</th><td data-colwidth="90" align="right">5432</td><td class="align-left">tcp</td></tr>' +
      '</tbody></table>'
    const parsed = DOMParser.fromSchema(schema).parse(html)
    parsed.check()
    const tableNode = parsed.firstChild!
    expect(tableNode.type.name).toBe('table')
    const [first, second] = [tableNode.child(0), tableNode.child(1)]
    expect(first.child(0).type.name).toBe('table_header')
    expect(first.child(1).attrs).toMatchObject({ colspan: 2, align: 'center' })
    expect(second.child(0).type.name).toBe('table_header')
    expect(second.child(1).attrs).toMatchObject({ colwidth: [90], align: 'right' })
    expect(second.child(2).attrs).toMatchObject({ align: 'left' })
  })

  it('parses its own output back to the same document', () => {
    const node = build(doc(table(row(header('a', { colwidth: [100] })), row(td('b', { align: 'right' })))))
    const html = document.createElement('div')
    html.appendChild(DOMSerializer.fromSchema(schema).serializeFragment(node.content))
    expect(DOMParser.fromSchema(schema).parse(html).toJSON()).toEqual(node.toJSON())
  })

  it('fills a new cell with an empty paragraph', () => {
    expect(schema.nodes.table_cell.createAndFill()!.firstChild!.type.name).toBe('paragraph')
    expect(schema.nodes.doc.createAndFill()!.firstChild!.type.name).toBe('paragraph')
  })
})

describe('the comment schema', () => {
  it('has no table nodes', () => {
    for (const name of ['table', 'table_row', 'table_cell', 'table_header']) {
      expect(commentSchema.nodes[name]).toBeUndefined()
    }
  })

  it('has every other documentation node, with the same content rules', () => {
    for (const [name, type] of Object.entries(commentSchema.nodes)) {
      expect(schema.nodes[name], name).toBeDefined()
      if (name !== 'doc') expect(schema.nodes[name]!.spec.content, name).toBe(type.spec.content)
    }
    expect(commentSchema.nodes.doc.spec.content).toBe('block+')
  })
})

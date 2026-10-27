/**
 * Tables in and out of the clipboard as text.
 *
 * In: a GFM pipe table, or tab-separated lines as a spreadsheet copies them,
 * pasted as plain text becomes a table. HTML tables need nothing here: the
 * schema parses them.
 *
 * HTML tables are tidied first so that what a cell can't hold (a heading, a
 * quote, a rule, a nested table) doesn't split the table apart.
 *
 * Out: a cell selection copies as tab-separated text beside the HTML table
 * ProseMirror writes, so a spreadsheet gets columns either way.
 */
import { Fragment, Slice } from 'prosemirror-model'
import type { Mark, Node as ProseMirrorNode, Schema } from 'prosemirror-model'
import { Lexer, type Token, type Tokens } from 'marked'
import { TableMap, __pastedCells as pastedCells } from 'prosemirror-tables'
import { parseInertHtml } from '@nosdesk/core/utils/inertHtml'
import { tablesToTextHtml } from './tableText'

type CellAlign = 'left' | 'center' | 'right'

function cell(schema: Schema, header: boolean, inline: ProseMirrorNode[], align: CellAlign | null = null): ProseMirrorNode {
  const paragraph = schema.nodes.paragraph!.create(null, inline)
  return schema.nodes[header ? 'table_header' : 'table_cell']!.create({ align }, paragraph)
}

function tableOf(schema: Schema, rows: ProseMirrorNode[][]): ProseMirrorNode {
  return schema.nodes.table!.create(
    null,
    rows.map((cells) => schema.nodes.table_row!.create(null, cells)),
  )
}

const SAFE_HREF = /^(https?:|mailto:)/i

/** A GFM cell's inline tokens as ProseMirror inline nodes. */
function inlineNodes(schema: Schema, tokens: Token[] | undefined, marks: readonly Mark[] = []): ProseMirrorNode[] {
  const out: ProseMirrorNode[] = []
  const text = (value: string, extra: readonly Mark[] = marks) => {
    if (value) out.push(schema.text(value, extra))
  }
  for (const token of tokens ?? []) {
    switch (token.type) {
      case 'strong':
      case 'em': {
        const mark = schema.marks[token.type === 'strong' ? 'strong' : 'em']!.create()
        out.push(...inlineNodes(schema, (token as Tokens.Strong).tokens, mark.addToSet(marks)))
        break
      }
      case 'codespan':
        text((token as Tokens.Codespan).text, schema.marks.code!.create().addToSet(marks))
        break
      case 'link': {
        const link = token as Tokens.Link
        const inner = link.tokens?.length ? link.tokens : [{ type: 'text', raw: link.text, text: link.text }]
        const linkMarks = SAFE_HREF.test(link.href)
          ? schema.marks.link!.create({ href: link.href, title: link.title ?? null }).addToSet(marks)
          : marks
        out.push(...inlineNodes(schema, inner as Token[], linkMarks))
        break
      }
      case 'br':
        out.push(schema.nodes.hard_break!.create())
        break
      case 'html':
        if (/^<br\s*\/?>$/i.test(token.raw.trim())) out.push(schema.nodes.hard_break!.create())
        else text(token.raw)
        break
      default: {
        const nested = (token as { tokens?: Token[] }).tokens
        if (nested?.length) out.push(...inlineNodes(schema, nested, marks))
        else text((token as { text?: string }).text ?? token.raw)
      }
    }
  }
  return out
}

/** A GFM pipe table as a table node, or null when `text` is anything else. */
export function gfmTable(schema: Schema, text: string): ProseMirrorNode | null {
  if (!text.includes('|')) return null
  const tokens = new Lexer({ gfm: true }).lex(text).filter((t) => t.type !== 'space')
  if (tokens.length !== 1 || tokens[0]!.type !== 'table') return null
  const table = tokens[0] as Tokens.Table
  if (table.header.length < 2) return null
  const align = (i: number) => (table.align[i] ?? null) as CellAlign | null
  const header = table.header.map((c, i) => cell(schema, true, inlineNodes(schema, c.tokens), align(i)))
  const body = table.rows.map((row) =>
    table.header.map((_, i) => cell(schema, false, inlineNodes(schema, row[i]?.tokens), align(i))),
  )
  return tableOf(schema, [header, ...body])
}

/**
 * Tab-separated lines (two or more, each with the same number of tabs) as a
 * table node, or null. No header row: a spreadsheet range has none.
 */
export function tsvTable(schema: Schema, text: string): ProseMirrorNode | null {
  const lines = text.replace(/\r\n?/g, '\n').replace(/\n+$/, '').split('\n')
  if (lines.length < 2) return null
  const tabs = lines[0]!.split('\t').length - 1
  if (tabs < 1 || lines.some((line) => line.split('\t').length - 1 !== tabs)) return null
  return tableOf(
    schema,
    lines.map((line) =>
      line.split('\t').map((value) => cell(schema, false, value.trim() ? [schema.text(value.trim())] : [])),
    ),
  )
}

/** Pasted plain text as a closed slice holding one table, or null. */
export function textToTableSlice(schema: Schema, text: string): Slice | null {
  const table = gfmTable(schema, text) ?? tsvTable(schema, text)
  return table ? new Slice(Fragment.from(table), 0, 0) : null
}

/** A cell's text on one line: blocks joined by spaces, no tabs. */
function cellText(cell: ProseMirrorNode): string {
  const blocks: string[] = []
  cell.descendants((node) => {
    if (!node.isTextblock) return true
    blocks.push(node.textBetween(0, node.content.size, ' ', ' '))
    return false
  })
  return blocks.join(' ').replace(/\s+/g, ' ').trim()
}

/**
 * Copied cells as tab-separated text, one line per row; a merged cell's text
 * sits in its top-left slot and the slots it covers stay empty. Null when the
 * slice holds no cells.
 */
export function cellsToTsv(schema: Schema, slice: Slice): string | null {
  const area = pastedCells(slice)
  if (!area) return null
  const table = schema.nodes.table!.create(
    null,
    area.rows.map((cells) => schema.nodes.table_row!.create(null, cells)),
  )
  const map = TableMap.get(table)
  const seen = new Set<number>()
  const lines: string[] = []
  for (let row = 0; row < map.height; row++) {
    const values: string[] = []
    for (let col = 0; col < map.width; col++) {
      const pos = map.map[row * map.width + col]!
      values.push(seen.has(pos) ? '' : cellText(table.nodeAt(pos)!))
      seen.add(pos)
    }
    lines.push(values.join('\t'))
  }
  return lines.join('\n')
}

/**
 * Pasted HTML with table cells reduced to what a cell holds: headings become
 * bold paragraphs, quotes give up their content, rules go, and a nested
 * table becomes one paragraph per row. Without this the parser closes the
 * table at the first such block and the rest lands outside it. HTML that
 * needs none of it comes back unchanged.
 */
export function tidyPastedTableHtml(html: string): string {
  if (!/<t[dh][\s>]/i.test(html)) return html
  const body = parseInertHtml(html)
  const doc = body.ownerDocument
  let changed = false
  // Innermost first, so an outer cell sees its nested tables already as text.
  for (const nested of Array.from(body.querySelectorAll('td table, th table')).reverse()) {
    const holder = doc.createElement('div')
    holder.innerHTML = tablesToTextHtml(nested.outerHTML)
    nested.replaceWith(...Array.from(holder.childNodes))
    changed = true
  }
  for (const heading of Array.from(body.querySelectorAll(':is(td, th) :is(h1, h2, h3, h4, h5, h6)'))) {
    const strong = doc.createElement('strong')
    strong.append(...Array.from(heading.childNodes))
    const p = doc.createElement('p')
    p.append(strong)
    heading.replaceWith(p)
    changed = true
  }
  for (const quote of Array.from(body.querySelectorAll(':is(td, th) blockquote')).reverse()) {
    quote.replaceWith(...Array.from(quote.childNodes))
    changed = true
  }
  for (const rule of Array.from(body.querySelectorAll(':is(td, th) hr'))) {
    rule.remove()
    changed = true
  }
  return changed ? body.innerHTML : html
}

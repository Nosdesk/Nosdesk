/**
 * Tables turned into plain blocks, for an editor that can't hold one: the
 * comment composer always, the documentation editor until its server allows
 * tables. A pasted table keeps its text rather than being dropped.
 *
 * Each row becomes one paragraph with its cells joined by `CELL_SEPARATOR`
 * when every cell is a single paragraph; otherwise the row's cells follow one
 * another as their own blocks. Rows with no text are left out.
 */
import { Fragment, Slice } from 'prosemirror-model'
import type { Node as ProseMirrorNode } from 'prosemirror-model'
import { parseInertHtml } from '@nosdesk/core/utils/inertHtml'

export const CELL_SEPARATOR = ' · '

function isTableNode(node: ProseMirrorNode): boolean {
  return Boolean(node.type.spec.tableRole)
}

/** Whether `fragment` holds a table, row or cell at any depth. */
export function fragmentHasTable(fragment: Fragment): boolean {
  let found = false
  fragment.descendants((node) => {
    if (found) return false
    if (isTableNode(node)) found = true
    return !found
  })
  return found
}

function rowBlocks(row: ProseMirrorNode): ProseMirrorNode[] {
  const cells: ProseMirrorNode[] = []
  row.forEach((cell) => cells.push(cell))
  const paragraph = row.type.schema.nodes.paragraph
  const lone = cells.map((cell) =>
    cell.childCount === 1 && cell.firstChild!.type === paragraph ? cell.firstChild! : null,
  )
  if (lone.every((p) => p !== null)) {
    if (lone.every((p) => p!.content.size === 0)) return []
    const parts: ProseMirrorNode[] = []
    lone.forEach((p, i) => {
      if (i > 0) parts.push(row.type.schema.text(CELL_SEPARATOR))
      p!.content.forEach((node) => parts.push(node))
    })
    return [paragraph.create(null, parts)]
  }
  return cells.flatMap((cell) => blocksOf(cell.content))
}

function blocksOf(fragment: Fragment): ProseMirrorNode[] {
  const out: ProseMirrorNode[] = []
  fragment.forEach((node) => {
    const role = node.type.spec.tableRole
    if (role === 'table') node.forEach((row) => out.push(...rowBlocks(row)))
    else if (role === 'row') out.push(...rowBlocks(node))
    else if (role === 'cell' || role === 'header_cell') out.push(...blocksOf(node.content))
    else out.push(node)
  })
  return out
}

/**
 * `slice` with its tables, rows and cells replaced by paragraphs. Returned
 * closed: the table's open sides have nothing to join in the result.
 */
export function tablesToTextSlice(slice: Slice): Slice {
  if (!fragmentHasTable(slice.content)) return slice
  return new Slice(Fragment.fromArray(blocksOf(slice.content)), 0, 0)
}

const BLOCK_TAGS = new Set([
  'ADDRESS', 'ARTICLE', 'ASIDE', 'BLOCKQUOTE', 'DIV', 'DL', 'FIGURE', 'FOOTER',
  'H1', 'H2', 'H3', 'H4', 'H5', 'H6', 'HEADER', 'HR', 'LI', 'OL', 'P', 'PRE',
  'SECTION', 'TABLE', 'UL',
])

/** A cell's inline content, or null when it holds blocks. */
function inlineContent(cell: HTMLTableCellElement): ChildNode[] | null {
  const elements = Array.from(cell.children)
  const strayText = Array.from(cell.childNodes).some(
    (node) => node.nodeType === 3 && node.textContent!.trim() !== '',
  )
  if (elements.length === 1 && elements[0]!.tagName === 'P' && !strayText) {
    return Array.from(elements[0]!.childNodes)
  }
  if (elements.some((el) => BLOCK_TAGS.has(el.tagName))) return null
  return Array.from(cell.childNodes)
}

function isEmptyCell(cell: HTMLTableCellElement): boolean {
  return cell.textContent!.trim() === '' && !cell.querySelector('img')
}

/**
 * Pasted HTML with its tables replaced by paragraphs, for an editor whose
 * schema has no tables (ProseMirror would otherwise run the cells together).
 * Parsed off the page, like any pasted HTML.
 */
export function tablesToTextHtml(html: string): string {
  if (!/<table[\s>]/i.test(html)) return html
  const body = parseInertHtml(html)
  const doc = body.ownerDocument
  // Innermost first, so an outer cell sees its nested table as paragraphs.
  for (const table of Array.from(body.querySelectorAll('table')).reverse()) {
    const out = doc.createDocumentFragment()
    for (const row of Array.from(table.rows)) {
      const cells = Array.from(row.cells)
      const inline = cells.map(inlineContent)
      if (inline.every((nodes) => nodes !== null)) {
        if (cells.every(isEmptyCell)) continue
        const p = doc.createElement('p')
        inline.forEach((nodes, i) => {
          if (i > 0) p.append(CELL_SEPARATOR)
          p.append(...nodes!)
        })
        out.append(p)
        continue
      }
      cells.forEach((cell, i) => {
        const nodes = inline[i]
        if (nodes) {
          if (isEmptyCell(cell)) return
          const p = doc.createElement('p')
          p.append(...nodes)
          out.append(p)
        } else {
          out.append(...Array.from(cell.childNodes))
        }
      })
    }
    table.replaceWith(out)
  }
  return body.innerHTML
}

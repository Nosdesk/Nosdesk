import { describe, expect, it } from 'vitest'
import { EditorState, TextSelection } from 'prosemirror-state'
import type { Node } from 'prosemirror-model'
import { setBlockType } from 'prosemirror-commands'
import { schema } from './schema'
import { toggleBlockquote } from './blockquote'

const p = (text: string) => schema.nodes.paragraph.create(null, text ? schema.text(text) : null)

function stateAt(doc: Node, pos: number): EditorState {
  const state = EditorState.create({ doc, schema })
  return state.apply(state.tr.setSelection(TextSelection.create(state.doc, pos)))
}

function run(state: EditorState): EditorState {
  let next = state
  expect(toggleBlockquote(state, (tr) => (next = state.apply(tr)))).toBe(true)
  return next
}

describe('toggleBlockquote', () => {
  it('is not a block type: setBlockType throws for it on an empty page', () => {
    const state = stateAt(schema.nodes.doc.create(null, [p('')]), 1)
    expect(() => setBlockType(schema.nodes.blockquote)(state, () => {})).toThrow(
      'Type given to setBlockType should be a textblock',
    )
  })

  it('wraps the paragraph on an empty page', () => {
    const next = run(stateAt(schema.nodes.doc.create(null, [p('')]), 1))
    expect(next.doc.child(0).type).toBe(schema.nodes.blockquote)
    expect(next.doc.child(0).child(0).type).toBe(schema.nodes.paragraph)
    next.doc.check()
  })

  it('wraps the paragraph holding the caret and keeps its text', () => {
    const next = run(stateAt(schema.nodes.doc.create(null, [p('one'), p('two')]), 7))
    expect(next.doc.child(0).type).toBe(schema.nodes.paragraph)
    expect(next.doc.child(1).type).toBe(schema.nodes.blockquote)
    expect(next.doc.child(1).textContent).toBe('two')
  })

  it('lifts a quoted paragraph back out', () => {
    const doc = schema.nodes.doc.create(null, [schema.nodes.blockquote.create(null, [p('quoted')])])
    const next = run(stateAt(doc, 3))
    expect(next.doc.childCount).toBe(1)
    expect(next.doc.child(0).type).toBe(schema.nodes.paragraph)
    expect(next.doc.child(0).textContent).toBe('quoted')
  })
})

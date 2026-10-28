import { afterEach, describe, expect, it } from 'vitest'
import { EditorState } from 'prosemirror-state'
import { EditorView } from 'prosemirror-view'
import { inputRules, undoInputRule } from 'prosemirror-inputrules'
import { DOMSerializer } from 'prosemirror-model'
import { commentSchema as schema } from '@/components/editor/schema'
import { markInputRules } from '@/components/editor/markInputRules'

let view: EditorView | null = null
afterEach(() => {
  view?.destroy()
  view = null
})

function editor(block: 'paragraph' | 'code_block' = 'paragraph') {
  const doc = schema.node('doc', null, [schema.node(block)])
  view = new EditorView(document.createElement('div'), {
    state: EditorState.create({ doc, plugins: [inputRules({ rules: markInputRules(schema) })] }),
  })
  return view
}

// Types one character at a time, the way input rules see a keyboard.
function type(v: EditorView, text: string) {
  for (const ch of text) {
    const { from, to } = v.state.selection
    const handled = v.someProp('handleTextInput', (f) => f(v, from, to, ch, () => v.state.tr.insertText(ch, from, to)))
    if (!handled) v.dispatch(v.state.tr.insertText(ch, from, to))
  }
}

function html(v: EditorView) {
  const div = document.createElement('div')
  div.appendChild(DOMSerializer.fromSchema(schema).serializeFragment(v.state.doc.content))
  return div.innerHTML
}

describe('markInputRules', () => {
  it.each([
    ['**bold** ', '<p><strong>bold</strong> </p>'],
    ['__bold__ ', '<p><strong>bold</strong> </p>'],
    ['*italic* ', '<p><em>italic</em> </p>'],
    ['_italic_ ', '<p><em>italic</em> </p>'],
    ['`npm ci` ', '<p><code>npm ci</code> </p>'],
    ['see **two words** here', '<p>see <strong>two words</strong> here</p>'],
  ])('formats %j', (typed, expected) => {
    const v = editor()
    type(v, typed)
    expect(html(v)).toBe(expected)
  })

  it('links typed Markdown to a web or mail address', () => {
    const v = editor()
    type(v, 'read [the guide](https://nosdesk.dev/docs) or [mail](mailto:help@nosdesk.dev)')
    expect(html(v)).toBe(
      '<p>read <a href="https://nosdesk.dev/docs">the guide</a> or <a href="mailto:help@nosdesk.dev">mail</a></p>',
    )
  })

  it.each([
    ['snake_case_name', '<p>snake_case_name</p>'],
    ['2*3*4', '<p>2*3*4</p>'],
    ['a * b * c', '<p>a * b * c</p>'],
    ['[x](javascript:alert(1))', '<p>[x](javascript:alert(1))</p>'],
    ['![alt](https://x.y/i.png)', '<p>![alt](https://x.y/i.png)</p>'],
  ])('leaves %j as typed', (typed, expected) => {
    const v = editor()
    type(v, typed)
    expect(html(v)).toBe(expected)
  })

  it('does nothing inside a code block', () => {
    const v = editor('code_block')
    type(v, '**not bold**')
    expect(v.state.doc.textContent).toBe('**not bold**')
  })

  it('gives the Markdown back on Backspace straight after', () => {
    const v = editor()
    type(v, '**bold**')
    expect(html(v)).toBe('<p><strong>bold</strong></p>')
    undoInputRule(v.state, v.dispatch)
    expect(html(v)).toBe('<p>**bold**</p>')
  })
})

/**
 * Inline Markdown typed into an editor becomes formatting as the closing
 * delimiter is typed: `**bold**` or `__bold__`, `*italic*` or `_italic_`,
 * `` `code` ``, and `[text](https://…)`. Backspace straight after undoes
 * it (`undoInputRule`). Rules don't fire inside a code block.
 */
import { InputRule } from 'prosemirror-inputrules'
import type { MarkType, Schema } from 'prosemirror-model'

/** Link destinations a typed `[text](url)` may have. */
const LINK_SCHEMES = /^(https?:\/\/|mailto:)\S+$/i

/**
 * `match[1]` is what precedes the opening delimiter (kept), `match[2]` the
 * text to format. The delimiters are removed.
 */
function markRule(pattern: RegExp, type: MarkType, attrs?: (match: RegExpMatchArray) => Record<string, unknown> | null) {
  return new InputRule(pattern, (state, match, start, end) => {
    const markAttrs = attrs ? attrs(match) : {}
    if (markAttrs === null) return null
    const text = match[2]
    if (!text) return null
    const from = start + match[1].length
    const tr = state.tr.replaceWith(from, end, state.schema.text(text, [type.create(markAttrs)]))
    tr.removeStoredMark(type)
    return tr
  })
}

// The opening delimiter must not follow a word character or itself, so
// snake_case and `2*3*4` stay as typed; the text must not start or end
// with a space.
const inner = (d: string) => `([^${d}\\s](?:[^${d}]*[^${d}\\s])?)`

export function markInputRules(schema: Schema): InputRule[] {
  const { strong, em, code, link } = schema.marks
  const rules: InputRule[] = []
  if (strong) {
    rules.push(markRule(new RegExp(`(^|[^*\\w])\\*\\*${inner('*')}\\*\\*$`), strong))
    rules.push(markRule(new RegExp(`(^|[^_\\w])__${inner('_')}__$`), strong))
  }
  if (em) {
    rules.push(markRule(new RegExp(`(^|[^*\\w])\\*${inner('*')}\\*$`), em))
    rules.push(markRule(new RegExp(`(^|[^_\\w])_${inner('_')}_$`), em))
  }
  if (code) {
    rules.push(markRule(/(^|[^`\w])`([^`]+)`$/, code))
  }
  if (link) {
    rules.push(
      markRule(/(^|[^!])\[([^\]]+)\]\((\S+)\)$/, link, (match) =>
        LINK_SCHEMES.test(match[3]) ? { href: match[3] } : null,
      ),
    )
  }
  return rules
}

/**
 * The toolbar's Blockquote. A blockquote holds blocks rather than text, so
 * it is a wrap and a lift, never `setBlockType` (which throws for it).
 */
import { lift, wrapIn } from 'prosemirror-commands'
import type { Command } from 'prosemirror-state'

/**
 * Wrap the selected blocks in a blockquote, or, when the blockquote is
 * what directly holds them, lift them out of it.
 */
export const toggleBlockquote: Command = (state, dispatch) => {
  const quote = state.schema.nodes.blockquote
  if (!quote) return false
  const { $from, $to } = state.selection
  if ($from.blockRange($to)?.parent.type === quote) return lift(state, dispatch)
  return wrapIn(quote)(state, dispatch)
}

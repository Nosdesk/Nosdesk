/**
 * ProseMirror Twemoji Plugin
 *
 * Renders emoji characters as Twemoji SVGs using inline decorations.
 * The emoji characters remain in the document - only the visual rendering changes.
 */
import { Plugin } from 'prosemirror-state'
import { Decoration, DecorationSet } from 'prosemirror-view'
import twemoji from '@twemoji/api'

const TWEMOJI_BASE = 'https://cdn.jsdelivr.net/gh/jdecked/twemoji@latest/assets/svg/'

// Emoji regex pattern - matches most emoji including ZWJ sequences
const emojiPattern = /(?:\p{Emoji_Presentation}|\p{Extended_Pictographic})(?:\uFE0F)?(?:\u200D(?:\p{Emoji_Presentation}|\p{Extended_Pictographic})(?:\uFE0F)?)*|\p{Emoji}\uFE0F/gu

function getEmojiUrl(emoji: string): string {
  const codepoints = twemoji.convert.toCodePoint(emoji)
  return `${TWEMOJI_BASE}${codepoints}.svg`
}

function findEmojis(doc: any): Decoration[] {
  const decorations: Decoration[] = []

  doc.descendants((node: any, pos: number) => {
    if (!node.isText) return

    const text = node.text || ''
    let match: RegExpExecArray | null

    emojiPattern.lastIndex = 0
    while ((match = emojiPattern.exec(text)) !== null) {
      const emoji = match[0]
      const from = pos + match.index
      const to = from + emoji.length

      decorations.push(
        Decoration.inline(from, to, {
          class: 'twemoji',
          style: `background-image: url("${getEmojiUrl(emoji)}")`
        })
      )
    }
  })

  return decorations
}

export const twemojiPlugin = new Plugin({
  props: {
    decorations(state) {
      const decorations = findEmojis(state.doc)
      return DecorationSet.create(state.doc, decorations)
    }
  }
})

export default twemojiPlugin

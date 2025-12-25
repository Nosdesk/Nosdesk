/**
 * v-twemoji Directive
 *
 * Wraps emoji characters in spans with Twemoji SVG background-images.
 * The original emoji character is preserved (for copy/paste, accessibility, editors)
 * but visually replaced with the SVG via CSS.
 *
 * Usage:
 *   <div v-twemoji>Hello 👋 World 🌍</div>
 */
import twemoji from '@twemoji/api'
import type { Directive, DirectiveBinding } from 'vue'

interface TwemojiDirectiveOptions {
  size?: 'xs' | 'sm' | 'md' | 'lg' | 'xl'
}

const TWEMOJI_BASE = 'https://cdn.jsdelivr.net/gh/jdecked/twemoji@latest/assets/svg/'

// Store observers per element
const observers = new WeakMap<HTMLElement, MutationObserver>()

// Debounce parsing to avoid excessive calls
let parseTimeout: ReturnType<typeof setTimeout> | null = null
const pendingElements = new Set<HTMLElement>()

// Emoji regex pattern - matches most emoji including ZWJ sequences
const emojiPattern = /(?:\p{Emoji_Presentation}|\p{Extended_Pictographic})(?:\uFE0F)?(?:\u200D(?:\p{Emoji_Presentation}|\p{Extended_Pictographic})(?:\uFE0F)?)*|\p{Emoji}\uFE0F/gu

function getEmojiUrl(emoji: string): string {
  const codepoints = twemoji.convert.toCodePoint(emoji)
  return `${TWEMOJI_BASE}${codepoints}.svg`
}

function wrapEmojis(el: HTMLElement) {
  const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT, null)
  const textNodes: Text[] = []

  let node: Text | null
  while ((node = walker.nextNode() as Text | null)) {
    // Skip if already inside a twemoji span, contenteditable, or ProseMirror
    const parent = node.parentElement
    if (parent?.classList.contains('twemoji') ||
        parent?.closest('[contenteditable="true"], .ProseMirror')) {
      continue
    }
    if (emojiPattern.test(node.textContent || '')) {
      textNodes.push(node)
    }
    // Reset regex lastIndex
    emojiPattern.lastIndex = 0
  }

  textNodes.forEach(textNode => {
    const text = textNode.textContent || ''
    const fragment = document.createDocumentFragment()
    let lastIndex = 0
    let match: RegExpExecArray | null

    emojiPattern.lastIndex = 0
    while ((match = emojiPattern.exec(text)) !== null) {
      // Add text before the emoji
      if (match.index > lastIndex) {
        fragment.appendChild(document.createTextNode(text.slice(lastIndex, match.index)))
      }

      // Create span wrapper for emoji
      const emoji = match[0]
      const span = document.createElement('span')
      span.className = 'twemoji'
      span.style.backgroundImage = `url("${getEmojiUrl(emoji)}")`
      span.textContent = emoji
      span.setAttribute('aria-hidden', 'true')
      fragment.appendChild(span)

      lastIndex = match.index + emoji.length
    }

    // Add remaining text after last emoji
    if (lastIndex < text.length) {
      fragment.appendChild(document.createTextNode(text.slice(lastIndex)))
    }

    // Replace the text node with our fragment
    if (fragment.childNodes.length > 0) {
      textNode.parentNode?.replaceChild(fragment, textNode)
    }
  })
}

function parseElement(el: HTMLElement) {
  // Skip if element is inside a contenteditable or ProseMirror area
  if (el.closest('[contenteditable="true"], .ProseMirror')) return
  wrapEmojis(el)
}

function scheduleParse(el: HTMLElement) {
  pendingElements.add(el)

  if (parseTimeout) return

  parseTimeout = setTimeout(() => {
    pendingElements.forEach(element => {
      if (document.contains(element)) {
        parseElement(element)
      }
    })
    pendingElements.clear()
    parseTimeout = null
  }, 10)
}

function setupObserver(el: HTMLElement) {
  const existingObserver = observers.get(el)
  if (existingObserver) {
    existingObserver.disconnect()
  }

  const observer = new MutationObserver((mutations) => {
    const hasRelevantChanges = mutations.some(mutation => {
      // Get the element to check (handle both Element and Text nodes)
      const targetElement = mutation.target.nodeType === Node.ELEMENT_NODE
        ? mutation.target as HTMLElement
        : mutation.target.parentElement

      // Skip mutations inside contenteditable or ProseMirror areas
      if (targetElement?.closest('[contenteditable="true"], .ProseMirror')) {
        return false
      }

      if (mutation.addedNodes.length > 0) {
        return Array.from(mutation.addedNodes).some(node => {
          // Skip twemoji spans we just created
          if (node instanceof HTMLElement && node.classList.contains('twemoji')) {
            return false
          }
          // Check if added node is inside excluded areas
          const nodeElement = node.nodeType === Node.ELEMENT_NODE
            ? node as HTMLElement
            : node.parentElement
          if (nodeElement?.closest('[contenteditable="true"], .ProseMirror')) {
            return false
          }
          return node.nodeType === Node.TEXT_NODE || node.nodeType === Node.ELEMENT_NODE
        })
      }

      if (mutation.type === 'characterData') {
        return true
      }

      return false
    })

    if (hasRelevantChanges) {
      scheduleParse(el)
    }
  })

  observer.observe(el, {
    childList: true,
    subtree: true,
    characterData: true
  })

  observers.set(el, observer)
}

export const vTwemoji: Directive<HTMLElement, TwemojiDirectiveOptions | undefined> = {
  mounted(el) {
    parseElement(el)
    setupObserver(el)
  },
  updated(el) {
    parseElement(el)
  },
  unmounted(el) {
    const observer = observers.get(el)
    if (observer) {
      observer.disconnect()
      observers.delete(el)
    }
    pendingElements.delete(el)
  }
}

export default vTwemoji

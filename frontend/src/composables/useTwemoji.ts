/**
 * Twemoji Composable
 *
 * Provides utilities for parsing and rendering emojis using Twemoji.
 * Emojis are rendered as SVG images for consistent cross-platform display.
 */
import twemoji from '@twemoji/api'

// Use CDN when VITE_TWEMOJI_CDN=true, otherwise serve locally from public/twemoji/
const useCdn = import.meta.env.VITE_TWEMOJI_CDN === 'true'
const CDN_BASE = 'https://cdn.jsdelivr.net/gh/jdecked/twemoji@latest/assets/svg/'
const LOCAL_BASE = '/twemoji/'

export const TWEMOJI_BASE = useCdn ? CDN_BASE : LOCAL_BASE

// Default Twemoji parse options (used by twemoji.parse() for bulk DOM replacement)
const defaultOptions: Parameters<typeof twemoji.parse>[1] = {
  folder: useCdn ? 'svg' : '',
  ext: '.svg',
  base: useCdn ? 'https://cdn.jsdelivr.net/gh/jdecked/twemoji@latest/assets/' : LOCAL_BASE,
  className: 'twemoji'
}

/**
 * Strip VS16 (U+FE0F) variation selectors from an emoji string,
 * UNLESS it contains a ZWJ (U+200D) sequence where VS16 is meaningful.
 * This matches how @twemoji/parser generates codepoints for SVG filenames.
 */
export function removeVS16s(rawEmoji: string): string {
  return rawEmoji.indexOf('\u200D') < 0
    ? rawEmoji.replace(/\uFE0F/g, '')
    : rawEmoji
}

/**
 * Convert an emoji to its Twemoji-compatible codepoint string.
 * Handles VS16 stripping to match actual SVG filenames on the CDN.
 */
export function emojiToCodepoint(emoji: string): string {
  return twemoji.convert.toCodePoint(removeVS16s(emoji))
}

/**
 * Get the Twemoji SVG URL for a single emoji character.
 */
export function getEmojiUrl(emoji: string): string {
  return `${TWEMOJI_BASE}${emojiToCodepoint(emoji)}.svg`
}

export function useTwemoji() {
  /**
   * Parse a string and replace emoji characters with Twemoji img elements
   * Returns HTML string with emoji replaced by <img> tags
   */
  const parseEmoji = (text: string, options?: Partial<typeof defaultOptions>): string => {
    return twemoji.parse(text, {
      ...defaultOptions,
      ...options
    })
  }

  /**
   * Parse a DOM element and replace emoji characters within it
   * Mutates the element in place
   */
  const parseElement = (element: HTMLElement, options?: Partial<typeof defaultOptions>): void => {
    twemoji.parse(element, {
      ...defaultOptions,
      ...options
    })
  }

  /**
   * Convert emoji to codepoint string (for building custom URLs)
   */
  const toCodePoint = (emoji: string): string => {
    return emojiToCodepoint(emoji)
  }

  /**
   * Check if a string contains emoji characters
   */
  const hasEmoji = (text: string): boolean => {
    const emojiRegex = /\p{Emoji_Presentation}|\p{Extended_Pictographic}/gu
    return emojiRegex.test(text)
  }

  return {
    parseEmoji,
    parseElement,
    getEmojiUrl,
    toCodePoint,
    hasEmoji
  }
}

export default useTwemoji

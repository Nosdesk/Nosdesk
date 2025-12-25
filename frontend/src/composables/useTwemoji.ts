/**
 * Twemoji Composable
 *
 * Provides utilities for parsing and rendering emojis using Twemoji.
 * Emojis are rendered as SVG images for consistent cross-platform display.
 */
import twemoji from '@twemoji/api'

// Default Twemoji parse options
const defaultOptions: Parameters<typeof twemoji.parse>[1] = {
  folder: 'svg',
  ext: '.svg',
  base: 'https://cdn.jsdelivr.net/gh/jdecked/twemoji@latest/assets/',
  className: 'twemoji'
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
   * Get the Twemoji SVG URL for a single emoji
   */
  const getEmojiUrl = (emoji: string): string => {
    const codepoints = twemoji.convert.toCodePoint(emoji)
    return `${defaultOptions.base}${defaultOptions.folder}/${codepoints}${defaultOptions.ext}`
  }

  /**
   * Convert emoji to codepoint string (for building custom URLs)
   */
  const toCodePoint = (emoji: string): string => {
    return twemoji.convert.toCodePoint(emoji)
  }

  /**
   * Check if a string contains emoji characters
   */
  const hasEmoji = (text: string): boolean => {
    // Use twemoji's regex pattern to detect emojis
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

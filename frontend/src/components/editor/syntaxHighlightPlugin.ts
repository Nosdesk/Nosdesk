/**
 * ProseMirror Syntax Highlighting Plugin
 *
 * Uses prosemirror-highlight with lowlight (highlight.js) to provide
 * syntax highlighting for code blocks. Colors are applied via CSS
 * variables from the theme system.
 */

import { createHighlightPlugin } from 'prosemirror-highlight'
import { createParser } from 'prosemirror-highlight/lowlight'
import { common, createLowlight } from 'lowlight'

// Create lowlight instance with common languages
// Common includes: javascript, typescript, python, css, html, json, bash, etc.
const lowlight = createLowlight(common)

// Create the parser for prosemirror-highlight
const parser = createParser(lowlight)

/**
 * Creates the syntax highlighting plugin for ProseMirror.
 * This plugin automatically highlights code blocks based on their language attribute.
 */
export const syntaxHighlightPlugin = createHighlightPlugin({ parser })

/**
 * Get list of supported languages
 */
export function getSupportedLanguages(): string[] {
  return lowlight.listLanguages()
}

/**
 * Check if a language is supported
 */
export function isLanguageSupported(language: string): boolean {
  return lowlight.registered(language)
}

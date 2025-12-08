/**
 * Sanitisation composable for Vue 3
 *
 * Provides consistent HTML sanitisation across the application using DOMPurify.
 * Use this composable whenever you need to render user-provided or external HTML content.
 */

import DOMPurify, { type Config } from 'dompurify';

// Configure DOMPurify defaults
const DEFAULT_CONFIG: Config = {
  ALLOWED_TAGS: [
    'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
    'p', 'br', 'hr',
    'ul', 'ol', 'li',
    'blockquote', 'pre', 'code',
    'a', 'strong', 'em', 'u', 's', 'mark', 'sub', 'sup',
    'table', 'thead', 'tbody', 'tr', 'th', 'td',
    'img',
    'div', 'span',
  ],
  ALLOWED_ATTR: [
    'href', 'target', 'rel', 'title', 'alt', 'src',
    'class', 'id',
    'colspan', 'rowspan',
  ],
  ALLOW_DATA_ATTR: false,
  ADD_ATTR: ['target'],
  FORBID_TAGS: ['script', 'style', 'iframe', 'object', 'embed', 'form', 'input'],
  FORBID_ATTR: ['onerror', 'onload', 'onclick', 'onmouseover', 'onfocus', 'onblur'],
};

// SVG-specific config for icon rendering
const SVG_CONFIG: Config = {
  USE_PROFILES: { svg: true, svgFilters: true },
  ALLOWED_TAGS: [
    'svg', 'path', 'circle', 'rect', 'line', 'polyline', 'polygon',
    'ellipse', 'g', 'defs', 'clipPath', 'use', 'symbol', 'text', 'tspan',
  ],
  ALLOWED_ATTR: [
    'viewBox', 'fill', 'stroke', 'stroke-width', 'stroke-linecap', 'stroke-linejoin',
    'd', 'cx', 'cy', 'r', 'rx', 'ry', 'x', 'y', 'x1', 'y1', 'x2', 'y2',
    'width', 'height', 'points', 'transform', 'class', 'id',
    'clip-path', 'clip-rule', 'fill-rule', 'opacity', 'xmlns',
  ],
  ALLOW_DATA_ATTR: false,
};

// Markdown config with code highlighting support
const MARKDOWN_CONFIG: Config = {
  ...DEFAULT_CONFIG,
  ALLOWED_TAGS: [
    ...DEFAULT_CONFIG.ALLOWED_TAGS as string[],
    'input', // for checkboxes in task lists
  ],
  ALLOWED_ATTR: [
    ...DEFAULT_CONFIG.ALLOWED_ATTR as string[],
    'type', 'checked', 'disabled', // for checkboxes
    'data-line', // for code line numbers
  ],
};

export type SanitiseProfile = 'default' | 'svg' | 'markdown' | 'strict';

/**
 * Composable for HTML sanitisation
 */
export function useSanitise() {
  /**
   * Sanitise HTML content with the specified profile
   */
  function sanitise(dirty: string, profile: SanitiseProfile = 'default'): string {
    if (!dirty) return '';

    let config: Config;

    switch (profile) {
      case 'svg':
        config = SVG_CONFIG;
        break;
      case 'markdown':
        config = MARKDOWN_CONFIG;
        break;
      case 'strict':
        // Strip all HTML, return text only
        config = { ALLOWED_TAGS: [], ALLOWED_ATTR: [] };
        break;
      default:
        config = DEFAULT_CONFIG;
    }

    // Add hook to make links safe
    DOMPurify.addHook('afterSanitizeAttributes', (node) => {
      if (node.tagName === 'A') {
        node.setAttribute('rel', 'noopener noreferrer');
        if (!node.hasAttribute('target')) {
          node.setAttribute('target', '_blank');
        }
      }
    });

    const clean = DOMPurify.sanitize(dirty, { ...config, RETURN_TRUSTED_TYPE: false });

    // Remove the hook to avoid affecting other sanitisations
    DOMPurify.removeHook('afterSanitizeAttributes');

    return clean as string;
  }

  /**
   * Sanitise HTML content (default profile)
   */
  function sanitiseHtml(dirty: string): string {
    return sanitise(dirty, 'default');
  }

  /**
   * Sanitise SVG content
   */
  function sanitiseSvg(dirty: string): string {
    return sanitise(dirty, 'svg');
  }

  /**
   * Sanitise markdown-rendered HTML content
   */
  function sanitiseMarkdown(dirty: string): string {
    return sanitise(dirty, 'markdown');
  }

  /**
   * Strip all HTML and return plain text
   */
  function stripHtml(dirty: string): string {
    return sanitise(dirty, 'strict');
  }

  /**
   * Check if content appears to be an SVG
   */
  function isSvg(content: string): boolean {
    if (!content) return false;
    const trimmed = content.trim();
    return trimmed.startsWith('<svg') || trimmed.startsWith('<?xml');
  }

  /**
   * Sanitise content, auto-detecting if it's SVG
   */
  function sanitiseAuto(dirty: string): string {
    if (!dirty) return '';
    return isSvg(dirty) ? sanitiseSvg(dirty) : sanitiseHtml(dirty);
  }

  return {
    sanitise,
    sanitiseHtml,
    sanitiseSvg,
    sanitiseMarkdown,
    stripHtml,
    sanitiseAuto,
    isSvg,
  };
}

// Export standalone functions for use outside Vue components
export const { sanitise, sanitiseHtml, sanitiseSvg, sanitiseMarkdown, stripHtml, sanitiseAuto, isSvg } = useSanitise();

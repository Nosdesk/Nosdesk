/**
 * v-safe-html directive
 *
 * A safe replacement for v-html that sanitises content before rendering.
 * Supports different sanitisation profiles via modifiers.
 *
 * Usage:
 *   v-safe-html="htmlContent"           - Default sanitisation
 *   v-safe-html.svg="svgContent"        - SVG sanitisation
 *   v-safe-html.markdown="mdContent"    - Markdown HTML sanitisation
 *   v-safe-html.strict="content"        - Strip all HTML (text only)
 */

import type { Directive, DirectiveBinding } from 'vue';
import { sanitise, sanitiseSvg, sanitiseMarkdown, stripHtml } from '@/composables/useSanitise';

type SanitiseModifier = 'svg' | 'markdown' | 'strict';

function getSanitisedContent(value: string, modifiers: DirectiveBinding['modifiers']): string {
  if (!value) return '';

  if (modifiers.svg) {
    return sanitiseSvg(value);
  }
  if (modifiers.markdown) {
    return sanitiseMarkdown(value);
  }
  if (modifiers.strict) {
    return stripHtml(value);
  }

  return sanitise(value);
}

export const vSafeHtml: Directive<HTMLElement, string> = {
  mounted(el, binding) {
    el.innerHTML = getSanitisedContent(binding.value, binding.modifiers);
  },
  updated(el, binding) {
    if (binding.value !== binding.oldValue) {
      el.innerHTML = getSanitisedContent(binding.value, binding.modifiers);
    }
  },
};

export default vSafeHtml;

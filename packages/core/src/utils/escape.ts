/**
 * Tiny string-escape utilities. The HTML escaper is used by every
 * caller that drops user-supplied text into `v-html` (search hit
 * highlighters, ticket-card renderers, autolinkers); the regex
 * escaper is used by any caller that builds a RegExp from a string
 * the user typed.
 *
 * Centralising these in one place lets us audit "what does our HTML
 * escaping cover" in a single look. The implementation deliberately
 * covers the same five characters that `OWASP HTML5 Security
 * Cheat Sheet` recommends for HTML body context, which is the
 * superset of what HTML, attribute, and innerHTML contexts each
 * need. If you ever need attribute-context escaping, this is the
 * function; if you need URL or CSS context, please write a separate
 * helper rather than overload this one.
 */

/**
 * Replace the five HTML-significant characters with named or
 * numeric entities. Safe for both HTML body and attribute contexts.
 */
export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

/**
 * Escape regex metacharacters so a user-typed string can be safely
 * embedded inside a `RegExp(...)` constructor. The set matches the
 * standard MDN guidance for "Escaping" under the RegExp page.
 */
export function escapeRegex(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

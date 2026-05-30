/**
 * Wrap matched substrings of any of the given search terms in
 * `<mark>` for HTML rendering. Used by the canned-response admin
 * list and the composer picker to point at "why this result
 * matched."
 *
 * Terms are matched as substrings, case-insensitive, OR-joined.
 * Empty `terms` returns the escaped input verbatim. Output is
 * always HTML-safe: the input is `escapeHtml`-ed first so any
 * angle brackets, quotes, etc. in the source text become inert
 * before the `<mark>` wrapping runs.
 */

import { escapeHtml, escapeRegex } from '@/utils/escape';

export function highlightTerms(text: string, terms: string[]): string {
  if (terms.length === 0) return escapeHtml(text);
  const pattern = new RegExp(`(${terms.map(escapeRegex).join('|')})`, 'gi');
  return escapeHtml(text).replace(pattern, '<mark>$1</mark>');
}

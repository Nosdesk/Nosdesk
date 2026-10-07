/**
 * Read HTML without building it in the page's document. An element made in
 * the page's document starts loading what its markup names (an `<img>`'s
 * `src`, say) as soon as `innerHTML` is set, even while detached. One parsed
 * into a `DOMParser` document doesn't: that document has no browsing context,
 * so nothing loads or runs.
 */

/** The `<body>` of `html` parsed into a document of its own. */
export function parseInertHtml(html: string): HTMLElement {
  return new DOMParser().parseFromString(html, 'text/html').body;
}

/** The text of `html`, trimmed. */
export function htmlText(html: string): string {
  if (!html) return '';
  return parseInertHtml(html).textContent?.trim() ?? '';
}

/** Whether `html` has any text, not just empty tags. */
export function htmlHasText(html: string): boolean {
  return htmlText(html).length > 0;
}

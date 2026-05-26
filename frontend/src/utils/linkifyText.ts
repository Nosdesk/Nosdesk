/**
 * Escape plain text and turn bare http(s) URLs into anchor tags.
 *
 * Used by the `text` render tier (plaintext email replies): the result
 * is rendered with `white-space: pre-wrap` so newlines and spacing are
 * preserved, and passed through `v-safe-html` (DOMPurify) as
 * defence-in-depth even though the input is already escaped here.
 *
 * Escaping happens first, so any markup in the sender's text becomes
 * inert before URLs are linkified. The URL match runs on the escaped
 * string; an `&` inside a URL is already `&amp;`, which is the correct
 * form for an href attribute.
 */

const URL_RE = /https?:\/\/[^\s<>"']+/g;

// Sentence punctuation that commonly trails a URL but isn't part of it,
// e.g. "see https://x.test/page." or "(https://x.test)".
const TRAILING_PUNCT = /[.,;:!?)\]}'"]+$/;

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

export function linkifyText(text: string): string {
  const escaped = escapeHtml(text);
  return escaped.replace(URL_RE, (match) => {
    const trail = TRAILING_PUNCT.exec(match);
    const url = trail ? match.slice(0, match.length - trail[0].length) : match;
    const tail = trail ? trail[0] : '';
    if (!url) return match;
    return `<a href="${url}" target="_blank" rel="noopener noreferrer nofollow">${url}</a>${tail}`;
  });
}

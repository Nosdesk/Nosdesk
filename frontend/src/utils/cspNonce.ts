/**
 * The Content-Security-Policy nonce for this document, when the host injects one.
 *
 * Tauri rewrites the configured CSP at load time to add a per-load nonce. Per the
 * CSP spec, **a nonce makes the browser ignore `'unsafe-inline'`** — so a
 * `<style>` element created at runtime without a nonce is blocked outright even
 * though `tauri.conf.json` lists `style-src 'self' 'unsafe-inline'`. On Android
 * that silently dropped every theme custom property and rendered the app
 * unstyled, which looks exactly like a blank screen:
 *
 *   Applying inline style violates the following Content Security Policy
 *   directive 'style-src 'self' 'unsafe-inline' 'nonce-…''. Note that
 *   'unsafe-inline' is ignored if either a hash or nonce value is present.
 *
 * Web builds inject no nonce, so this returns `''` there and the existing
 * `'unsafe-inline'` path keeps working unchanged.
 *
 * Two sources, because the policy can arrive as a response header (no meta tag
 * to read) or as a meta tag: prefer an already-nonced element, since the `nonce`
 * IDL property still returns the value after browsers hide the content
 * attribute, then fall back to parsing the meta tag.
 */
export function cspNonce(): string {
  const nonced = document.querySelector<HTMLElement>('script[nonce], style[nonce]')
  if (nonced?.nonce) return nonced.nonce

  const meta = document.querySelector('meta[http-equiv="Content-Security-Policy"]')
  return meta?.getAttribute('content')?.match(/'nonce-([^']+)'/)?.[1] ?? ''
}

/**
 * Create a `<style>` element that survives a nonce-based CSP.
 *
 * Use this instead of `document.createElement('style')` anywhere styles are
 * injected at runtime; a bare one is dropped on native builds.
 */
export function createNoncedStyleElement(id?: string): HTMLStyleElement {
  const el = document.createElement('style')
  if (id) el.id = id
  const nonce = cspNonce()
  if (nonce) el.nonce = nonce
  return el
}

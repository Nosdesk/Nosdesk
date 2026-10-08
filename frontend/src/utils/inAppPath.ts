/**
 * Whether `value` is a path inside this app (`/acme/tickets/12?tab=notes`),
 * safe to send the browser back to after sign-in. As written it must be one
 * leading slash and then visible ASCII; with its `%XX` escapes decoded it must
 * hold no backslash and not start with `//`. That rules out other hosts
 * (`//host`, and `/\host`, which browsers read the same way), schemes, and
 * anything a browser would strip or rewrite. The server applies the same rule
 * (`safe_post_login_location`).
 */
export function isInAppPath(value: unknown): value is string {
  if (typeof value !== 'string' || !/^\/[!-~]*$/.test(value)) return false
  const decoded = value.replace(/%([0-9A-Fa-f]{2})/g, (_, hex: string) => String.fromCharCode(parseInt(hex, 16)))
  return !decoded.includes('\\') && !decoded.startsWith('//')
}

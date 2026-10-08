/** Longest return path sign-in follows; a longer one falls back to "/". */
export const MAX_RETURN_PATH_LENGTH = 2048

/** `%XX` escapes decoded byte by byte; malformed ones are left as written. */
const decodeEscapes = (value: string): string =>
  value.replace(/%([0-9A-Fa-f]{2})/g, (_, hex: string) => String.fromCharCode(parseInt(hex, 16)))

/**
 * Whether `value` is a path inside this app (`/acme/tickets/12?tab=notes`),
 * safe to send the browser back to after sign-in. As written it must be one
 * leading slash and then visible ASCII, at most `MAX_RETURN_PATH_LENGTH` long.
 * With its `%XX` escapes decoded it must hold no backslash, and its path part
 * (before any `?` or `#`) no `//` and no `.` or `..` segment, so it can't
 * resolve to another host or start with `//`. The server applies the same rule
 * (`safe_post_login_location`), and both sides test it against
 * `backend/tests/fixtures/in_app_paths.json`.
 */
export function isInAppPath(value: unknown): value is string {
  if (typeof value !== 'string' || value.length > MAX_RETURN_PATH_LENGTH) return false
  if (!/^\/[!-~]*$/.test(value)) return false
  if (decodeEscapes(value).includes('\\')) return false
  const end = value.search(/[?#]/)
  const path = decodeEscapes(end === -1 ? value : value.slice(0, end))
  if (path.includes('//')) return false
  return path
    .slice(1)
    .split('/')
    .every((segment) => segment !== '.' && segment !== '..')
}

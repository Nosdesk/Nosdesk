/**
 * Strip query string and fragment from a URL-shaped string. Optionally
 * mask UUID-looking path segments. Applied to:
 * - `window.location` before it's captured into the event.url field
 * - Each breadcrumb's URL before it lands in the ring buffer
 *
 * The backend also strips query/fragment on the event.url at ingest as
 * defence in depth, but stripping here means OAuth callback fragments
 * (#access_token=), reset tokens (?token=), and invite codes
 * (?invite=) never leave the browser's process memory at all.
 */
const UUID_PATTERN = /[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi

export function scrubUrl(raw: string | undefined | null): string {
  if (!raw) return ''
  let out = String(raw)
  const hashIdx = out.indexOf('#')
  if (hashIdx >= 0) out = out.slice(0, hashIdx)
  const queryIdx = out.indexOf('?')
  if (queryIdx >= 0) out = out.slice(0, queryIdx)
  out = out.replace(UUID_PATTERN, ':uuid')
  return out
}

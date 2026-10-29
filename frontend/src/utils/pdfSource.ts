import type { RouteLocation, RouteLocationRaw } from 'vue-router'

/**
 * Which files the PDF overlay may open from a link, and what to call them.
 *
 * A link names the file by its stored path (`?pdf=/uploads/tickets/7/...`).
 * The server checks access when the file is fetched, so the link carries no
 * permission of its own; these rules only keep the overlay to the app's own
 * ticket files, so a crafted link can't show another site's PDF under
 * Nosdesk's name.
 */

const TICKET_FILE = /^\/(?:uploads|api\/files)\/tickets\/(\d+)\/([^/]+)$/
const TEMP_FILE = /^\/(?:uploads|api\/files)\/temp\/([^/]+)$/

/** True for a ticket or upload file path the overlay may open. */
export function isAllowedPdfPath(path: unknown): path is string {
  if (typeof path !== 'string') return false
  const match = TICKET_FILE.exec(path) ?? TEMP_FILE.exec(path)
  if (!match) return false
  const file = match[match.length - 1]!
  return file !== '.' && file !== '..' && !file.includes('\\')
}

// Stored names start with the upload's uuid: `<uuid>_invoice.pdf`.
const UUID_PREFIX = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}_/i

/** The file's name for display: its stored name without the uuid prefix. */
export function pdfNameFromPath(path: string): string | null {
  const segment = path.split('/').pop() ?? ''
  let name = segment
  try {
    name = decodeURIComponent(segment)
  } catch {
    // A literal "%" in a stored name: show it as stored.
  }
  name = name.replace(UUID_PREFIX, '')
  return name || null
}

/** The ticket a stored ticket file belongs to, by id. */
export function ticketIdFromPdfPath(path: string): number | null {
  const match = TICKET_FILE.exec(path)
  return match ? Number(match[1]) : null
}

/**
 * The stored path behind a viewer `src`: the path itself, its `/api/files`
 * form, or either behind the mobile app's asset scheme.
 */
export function pdfPathFromSource(src: unknown): string | null {
  if (typeof src !== 'string') return null
  let path = src
  const scheme = /^[a-z][a-z0-9+.-]*:\/\/[^/]*/i.exec(src)
  if (scheme) path = src.slice(scheme[0].length)
  path = path.split(/[?#]/)[0] ?? ''
  return isAllowedPdfPath(path) ? path : null
}

/**
 * Where a link from before PDFs opened over their ticket now goes
 * (`/pdf-viewer?src=...&filename=...`): the same file over its ticket, or
 * over the home page for an upload with no ticket. `to.params` carries the
 * workspace slug on Cloud.
 */
export function legacyPdfViewerRedirect(to: RouteLocation): RouteLocationRaw {
  const pdf = pdfPathFromSource(to.query.src)
  if (!pdf) return { name: 'home', params: to.params }
  const query = { pdf, ...(typeof to.query.page === 'string' ? { page: to.query.page } : {}) }
  const ticketId = ticketIdFromPdfPath(pdf)
  return ticketId === null
    ? { name: 'home', params: to.params, query }
    : { name: 'ticket-by-id', params: { ...to.params, id: String(ticketId) }, query }
}

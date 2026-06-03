/**
 * Module-level ring buffer for diagnostic breadcrumbs. Captures the
 * user's recent route changes and API calls so a bug report can
 * include the trail leading up to the submit.
 *
 * Capped at 10 entries; oldest are dropped silently. No dedup, no
 * dropped-count metadata in v1; the full pipeline adds those when it
 * lands post-M5.
 *
 * URL-shaped values pass through `scrubUrl` before being stored, so
 * query strings and fragments never enter the buffer in the first
 * place. The full URL never leaves the browser.
 */
import { scrubUrl } from './scrubUrl'

export type BreadcrumbCategory = 'route' | 'api'

export interface Breadcrumb {
  category: BreadcrumbCategory
  ts: number
  summary: string
}

const MAX_ENTRIES = 10

const buffer: Breadcrumb[] = []

function push(entry: Breadcrumb): void {
  buffer.push(entry)
  if (buffer.length > MAX_ENTRIES) {
    buffer.splice(0, buffer.length - MAX_ENTRIES)
  }
}

export function pushRoute(to: string): void {
  push({ category: 'route', ts: Date.now(), summary: scrubUrl(to) })
}

/** API-call breadcrumb. URL is scrubbed; status optional. */
export function pushApi(method: string, url: string, status?: number): void {
  // Filter routes that shouldn't appear in their own report's trail
  // (they're either noise or the submit itself).
  if (shouldSkipApiUrl(url)) return
  const cleaned = scrubUrl(url)
  const summary = status === undefined ? `${method} ${cleaned}` : `${method} ${cleaned} ${status}`
  push({ category: 'api', ts: Date.now(), summary })
}

/** Snapshot the current ring for inclusion in a bug report payload. */
export function snapshot(): Breadcrumb[] {
  return buffer.slice()
}

/** Test-only reset. */
export function _resetForTests(): void {
  buffer.length = 0
}

function shouldSkipApiUrl(url: string): boolean {
  return url.includes('/bug-reports') || url.includes('/auth/refresh')
}

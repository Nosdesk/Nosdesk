/**
 * Per-tab session identifier for diagnostics correlation.
 *
 * Minted on first read from `crypto.randomUUID()` (UUIDv4) and
 * persisted to `sessionStorage` so a refresh keeps the same id. New
 * tabs naturally get a fresh value because sessionStorage is tab-
 * scoped. No idle rotation in v1.
 *
 * The same value is also sent as the `X-Nosdesk-Trace-Id` header on
 * every authenticated API request (apiConfig.ts) so backend tracing
 * spans pick it up and `grep <session_id>` correlates the FE bug
 * report to the BE request lines.
 */
const STORAGE_KEY = 'nosdesk.diag.session'

function newId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID()
  }
  // Fallback for very old browsers / non-secure contexts. Not
  // cryptographically strong; the value is a correlation hint only,
  // not an authorisation token.
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0
    const v = c === 'x' ? r : (r & 0x3) | 0x8
    return v.toString(16)
  })
}

export function getSessionId(): string {
  try {
    const existing = window.sessionStorage.getItem(STORAGE_KEY)
    if (existing) return existing
    const fresh = newId()
    window.sessionStorage.setItem(STORAGE_KEY, fresh)
    return fresh
  } catch {
    return newId()
  }
}

/**
 * CSRF token access (double-submit cookie).
 *
 * The backend sets a `csrf_token` cookie and requires it echoed in the
 * `X-CSRF-Token` header on state-changing requests. `apiClient` injects
 * the header automatically; transports that deliberately bypass
 * `apiClient` (e.g. the sync queue's raw `fetch`) read the token from
 * here so there's a single source of truth.
 */
export function getCsrfToken(): string | null {
  const match = document.cookie.match(/csrf_token=([^;]+)/)
  return match ? match[1] : null
}

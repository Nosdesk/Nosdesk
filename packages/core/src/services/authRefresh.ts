/**
 * Single source of truth for refreshing the short-lived (15 min)
 * access token.
 *
 * Both the axios response interceptor (`apiConfig.ts`) and the raw-fetch
 * sync runtime (`sync/lifecycle.ts`) hit 401s when the access cookie
 * expires and both need to refresh. They MUST funnel through one
 * deduplicated refresh: the `/api/auth/refresh` endpoint rotates the
 * refresh token, so two concurrent refreshes would race and one would
 * invalidate the other's token, bouncing the user to login. `inFlight`
 * collapses all concurrent callers onto a single POST.
 *
 * Standalone (only depends on axios) so both callers can import it
 * without an import cycle through apiConfig.
 */
import axios from 'axios'
import { apiBaseUrl, transport } from '../transport'

let inFlight: Promise<boolean> | null = null

/**
 * Refresh the access token, coordinating with any refresh already in
 * flight. Resolves `true` when the session was renewed, `false` when the
 * refresh was rejected (session genuinely expired) or the request
 * failed. Never throws. Callers retry their request on `true` and give
 * up on `false` (the axios interceptor owns the redirect-to-login).
 */
export function refreshAccessToken(): Promise<boolean> {
  if (inFlight) return inFlight
  inFlight = (async () => {
    try {
      const res = await axios.post(
        `${apiBaseUrl()}/auth/refresh`,
        {},
        { withCredentials: transport().auth.useCredentials },
      )
      return res.status === 200
    } catch {
      return false
    } finally {
      inFlight = null
    }
  })()
  return inFlight
}

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
import { apiBaseUrl, transport, type RefreshResult } from '../transport'

let inFlight: Promise<RefreshResult> | null = null

/**
 * Refresh the access token, coordinating with any refresh already in
 * flight. Never throws. Only a 401 from the refresh endpoint means the
 * session is over (`rejected`); no answer or any other failure (offline, a
 * 5xx while the server restarts) is `unavailable`, and nobody is signed out
 * for it. Callers go through `refreshSession()`, which acts on a rejection.
 */
export function refreshAccessToken(): Promise<RefreshResult> {
  if (inFlight) return inFlight
  inFlight = (async (): Promise<RefreshResult> => {
    try {
      await axios.post(
        `${apiBaseUrl()}/auth/refresh`,
        {},
        { withCredentials: transport().auth.useCredentials },
      )
      return 'renewed'
    } catch (err) {
      return axios.isAxiosError(err) && err.response?.status === 401 ? 'rejected' : 'unavailable'
    } finally {
      inFlight = null
    }
  })()
  return inFlight
}

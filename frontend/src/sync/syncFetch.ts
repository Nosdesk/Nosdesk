import { apiBaseUrl, transport } from '@nosdesk/core/transport'
import { workspaceHeaders } from '@/services/activeWorkspace'

/**
 * `fetch` for the sync runtime that survives an expired (15 min) access
 * token. Sync uses raw fetch (streaming bootstrap, no axios), so it doesn't
 * inherit the axios client's refresh-on-401. On a 401 it runs the shared,
 * deduplicated refresh once and retries; that single refresh is coordinated
 * with the axios client so the token-rotating endpoint is never hit twice
 * concurrently. On refresh failure the original 401 response is returned and
 * the caller backs off (the axios client owns redirect-to-login for a dead
 * session).
 *
 * Base URL, auth headers (the CSRF middleware requires the double-submit
 * header on a POST), the selection header (empty in host mode) and credential
 * mode all come from the transport seam. `init.body` must be resendable (a
 * string), since a 401 sends it twice.
 */
export async function syncFetch(path: string, init: RequestInit = {}): Promise<Response> {
  const url = `${apiBaseUrl()}${path}`
  const credentials: RequestCredentials = transport().auth.useCredentials ? 'include' : 'omit'
  // Built per attempt: a refresh replaces a bearer client's (mobile's) token,
  // so the retry must carry the new one, not the expired one it just got a
  // 401 for.
  const attempt = () =>
    fetch(url, {
      ...init,
      credentials,
      headers: {
        ...(init.headers as Record<string, string> | undefined),
        ...workspaceHeaders(),
        ...transport().auth.authHeaders(),
      },
    })
  const res = await attempt()
  if (res.status !== 401) return res
  const refreshed = await transport().auth.refresh()
  if (!refreshed) return res
  return attempt()
}

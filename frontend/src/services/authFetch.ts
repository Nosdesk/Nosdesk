import { apiBaseUrl, transport } from '@nosdesk/core/transport'
import { refreshSession } from '@nosdesk/core/services/session'
import { workspaceHeaders } from '@/services/activeWorkspace'

/**
 * `fetch` an API path with the transport's auth, for callers that can't use
 * the axios client (streaming sync, the sync push, file downloads). On a 401
 * it runs the shared refresh once and retries. A rejected refresh has already
 * signed the person out and an unreachable server signs nobody out; either
 * way the original 401 comes back and the caller backs off.
 *
 * Base URL, auth headers (the CSRF middleware requires the double-submit
 * header on a POST), the selection header (empty in host mode) and credential
 * mode all come from the transport seam. `init.body` must be resendable (a
 * string), since a 401 sends it twice.
 */
export async function authFetch(path: string, init: RequestInit = {}): Promise<Response> {
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
  if ((await refreshSession()) !== 'renewed') return res
  return attempt()
}

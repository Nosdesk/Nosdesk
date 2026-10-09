/**
 * One window at a time starts SSO on its own. Every tab that lands on /login
 * in SSO-only mode would otherwise start a flow, and each flow overwrites the
 * single-slot state cookies (the IdP's login CSRF cookie and our OAuth state
 * binding), so every callback but the last fails. The first window claims the
 * start for a minute; others show the button and wait. A click always starts.
 */
const KEY = 'nosdesk:sso-autostart'
const TAB_KEY = 'nosdesk:sso-tab'
const HOLD_MS = 60_000

/** Stable for this tab across the round trip to the IdP and back. */
function tabId(): string {
  let id = sessionStorage.getItem(TAB_KEY)
  if (!id) {
    id = Math.random().toString(36).slice(2)
    sessionStorage.setItem(TAB_KEY, id)
  }
  return id
}

/** Claim the automatic start for this tab. False if another window holds it. */
export function claimSsoAutoStart(now = Date.now()): boolean {
  try {
    const me = tabId()
    const held = JSON.parse(localStorage.getItem(KEY) ?? 'null') as { tab?: string; at?: number } | null
    if (held?.tab && held.tab !== me && typeof held.at === 'number' && now - held.at < HOLD_MS) {
      return false
    }
    localStorage.setItem(KEY, JSON.stringify({ tab: me, at: now }))
  } catch {
    // Storage unavailable: no coordination, start as before.
  }
  return true
}

/** Free the claim, so the next login page can start straight away. */
export function releaseSsoAutoStart(): void {
  try {
    localStorage.removeItem(KEY)
  } catch {
    // ignore
  }
}

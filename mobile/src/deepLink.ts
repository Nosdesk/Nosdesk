/**
 * Ticket deep links (iOS Universal Links / Android App Links).
 *
 * The native plugin delivers the tapped/scanned URL; the frontend decides where
 * it goes (see the handler in App.vue). v1 rule: a ticket link whose host is the
 * server we're connected to opens the ticket in-app (waiting for the session on
 * a cold start / after login); anything else (a different tenant, a server we're
 * not connected to) opens in the system browser.
 */
import { getCurrent, onOpenUrl } from '@tauri-apps/plugin-deep-link'
import { openUrl } from '@tauri-apps/plugin-opener'

/** The URL the app was cold-started with via a deep link, or null. */
export async function getInitialDeepLink(): Promise<string | null> {
  try {
    const urls = await getCurrent()
    return urls?.[0] ?? null
  } catch {
    return null
  }
}

/**
 * Subscribe to deep links opened while the app is running. Resolves to an
 * unlisten function. Best-effort: never throws (a listener failure must not
 * break app startup).
 */
export async function onDeepLink(handler: (url: string) => void): Promise<() => void> {
  try {
    return await onOpenUrl((urls) => {
      for (const u of urls) handler(u)
    })
  } catch {
    return () => {}
  }
}

/**
 * Open a URL in the system browser: the cross-tenant / not-connected fallback
 * for a ticket link the app can't open in place. Best-effort.
 */
export async function openInBrowser(url: string): Promise<void> {
  try {
    await openUrl(url)
  } catch {
    // A failed browser hand-off must not throw.
  }
}

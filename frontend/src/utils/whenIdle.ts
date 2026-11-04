/**
 * Run once the page is idle, or after two seconds at the latest: a page that
 * animates (the sign-in background) may never be idle. Falls back to a short
 * timeout where requestIdleCallback is missing (older Safari).
 */
export function whenIdle(run: () => void): void {
  if (typeof window.requestIdleCallback === 'function') window.requestIdleCallback(run, { timeout: 2000 })
  else setTimeout(run, 200)
}

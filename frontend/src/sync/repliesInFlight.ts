/**
 * Replies shown in the thread that the server hasn't confirmed yet. A reply
 * uploads its files first and is created only once they're in, so a tab
 * closed or reloaded in between loses it. While any are in flight, leaving
 * the page asks first. Navigating inside the app is safe: the send carries on.
 */
let inFlight = 0

export function replyStarted(): void {
  inFlight++
}

export function replyFinished(): void {
  inFlight = Math.max(0, inFlight - 1)
}

/** The `beforeunload` listener: asks before a reload or close loses a reply. */
export function warnBeforeUnload(e: BeforeUnloadEvent): void {
  if (inFlight === 0) return
  e.preventDefault()
  // Legacy browsers need returnValue set to trigger the prompt.
  e.returnValue = ''
}

// Module scope, not per component: the reply outlives the view that sent it.
if (typeof window !== 'undefined') {
  window.addEventListener('beforeunload', warnBeforeUnload)
}

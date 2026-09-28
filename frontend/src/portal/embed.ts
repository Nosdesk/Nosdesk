// Embed mode: the portal inside the help widget's iframe on a customer's site
// (served at `/widget`, see backend `handlers::widget`).
//
// The loader on the host page posts `nosdesk:init` once the iframe loads,
// saying whether the site signs visitors in. We accept messages only from our
// direct parent, learn its origin from that first message, and send
// everything back to exactly that origin.

export const isEmbed = /\/widget\/?$/.test(window.location.pathname)

export interface EmbedInit {
  /** The host page's origin (`https://help.acme.com`). */
  parentOrigin: string
  /** The host page has a `getToken` to sign the visitor in. */
  identity: boolean
}

let parentOrigin: string | null = null
let resolveInit: ((init: EmbedInit) => void) | null = null
const initPromise = new Promise<EmbedInit>((resolve) => {
  resolveInit = resolve
})

if (isEmbed) {
  window.addEventListener('message', (event) => {
    if (event.source !== window.parent) return
    const data = event.data as { type?: string; identity?: unknown } | null
    if (data?.type === 'nosdesk:init' && parentOrigin === null) {
      parentOrigin = event.origin
      resolveInit?.({ parentOrigin: event.origin, identity: data.identity === true })
    }
  })
  // In case the loader's init went out before we were listening.
  window.parent.postMessage({ type: 'nosdesk:hello' }, '*')
}

/** Resolves when the host page introduces itself. */
export function embedInit(): Promise<EmbedInit> {
  return initPromise
}

/** Message the host page (only once it has introduced itself). */
export function postToParent(message: Record<string, unknown>): void {
  if (parentOrigin) window.parent.postMessage(message, parentOrigin)
}

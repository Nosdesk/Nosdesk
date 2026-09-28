// Embed mode: the portal inside a host's frame. Two hosts:
// - the help widget on a customer's site (`/widget`, backend `handlers::widget`);
// - the Microsoft Teams personal tab (`/teams`, see `./teams`).
//
// The widget's loader posts `nosdesk:init` once the iframe loads, saying
// whether the site signs visitors in. We accept messages only from our direct
// parent, learn its origin from that first message, and send everything back
// to exactly that origin.

import { signInTeams, teamsBearer } from './teams'

export type EmbedHost = 'widget' | 'teams'

export const embedHost: EmbedHost | null = /\/widget\/?$/.test(window.location.pathname)
  ? 'widget'
  : /\/teams\/?$/.test(window.location.pathname)
    ? 'teams'
    : null

export const isEmbed = embedHost !== null

/** The embedded portal's first screen. */
export const embedHome = embedHost === 'teams' ? '/teams-status' : '/embed'

export interface EmbedInit {
  /** The host page's origin (`https://help.acme.com`). */
  parentOrigin: string
  /** The host page has a `getToken` to sign the visitor in. */
  identity: boolean
}

let parentOrigin: string | null = null
/** The portal access token for a signed-in visitor, held only in memory. */
let bearer: string | null = null
let pendingToken: ((token: string | null) => void) | null = null
let resolveInit: ((init: EmbedInit) => void) | null = null
const initPromise = new Promise<EmbedInit>((resolve) => {
  resolveInit = resolve
})

if (embedHost === 'widget') {
  window.addEventListener('message', (event) => {
    if (event.source !== window.parent) return
    const data = event.data as { type?: string; identity?: unknown } | null
    if (data?.type === 'nosdesk:init' && parentOrigin === null) {
      parentOrigin = event.origin
      resolveInit?.({ parentOrigin: event.origin, identity: data.identity === true })
    } else if (
      data?.type === 'nosdesk:token' &&
      event.origin === parentOrigin &&
      pendingToken
    ) {
      const token = (data as { token?: unknown }).token
      pendingToken(typeof token === 'string' ? token : null)
      pendingToken = null
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

/** The signed-in person's portal token, if any. */
export function embedBearer(): string | null {
  return embedHost === 'teams' ? teamsBearer() : bearer
}

/** Sign the person in again through the host (their token lapsed). */
export async function signInEmbedded(): Promise<boolean> {
  if (embedHost === 'teams') return (await signInTeams()) === null
  return signInVisitor()
}

/** Ask the host page for a visitor token (its `getToken`); null if it has none
 * or doesn't answer within 15 seconds. */
function requestToken(): Promise<string | null> {
  if (!parentOrigin) return Promise.resolve(null)
  return new Promise((resolve) => {
    pendingToken = resolve
    postToParent({ type: 'nosdesk:token-request' })
    setTimeout(() => {
      if (pendingToken === resolve) {
        pendingToken = null
        resolve(null)
      }
    }, 15_000)
  })
}

let signingIn: Promise<boolean> | null = null

/** Sign the visitor in with a token from the host page. Shared by concurrent
 * callers; `false` when the site can't or won't vouch for them. */
export function signInVisitor(): Promise<boolean> {
  signingIn ??= (async () => {
    const token = await requestToken()
    if (!token || !parentOrigin) return false
    const res = await fetch('/api/portal/auth/widget/session', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ token, parent_origin: parentOrigin }),
    })
    if (!res.ok) return false
    const data = (await res.json()) as { access_token?: string }
    bearer = data.access_token ?? null
    return bearer !== null
  })().finally(() => {
    signingIn = null
  })
  return signingIn
}

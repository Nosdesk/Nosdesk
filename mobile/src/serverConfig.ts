/**
 * Which Nosdesk server the app talks to.
 *
 * Nosdesk is self-hostable, so the app isn't pinned to one host: the user picks
 * a server (the official cloud by default, or their own instance) and it's
 * persisted. The chosen origin feeds the transport seam's base URLs.
 *
 * The origin is not a secret, so it lives in the general KV store (the refresh
 * token, which IS a secret, lives in the keychain, see secureStore.ts).
 */
import { fetch as tauriFetch } from '@tauri-apps/plugin-http'
import { getVersion } from '@tauri-apps/api/app'
import { storage } from '@nosdesk/core/storage'

const SERVER_KEY = 'nosdesk:server-origin'

/**
 * The oldest server this app works with. Bearer sign-in and `/api/config`
 * arrived in 1.1, so a 1.0 server can't serve the app at all.
 */
export const MIN_SERVER_VERSION = '1.1.0'

/** Pre-filled in the connect screen; the official cloud. */
export const DEFAULT_SERVER = 'https://app.nosdesk.com'

/** The persisted server origin (e.g. `https://help.acme.com`), or null. */
export function getStoredServer(): string | null {
  return storage().getItem(SERVER_KEY)
}

export function storeServer(origin: string): void {
  storage().setItem(SERVER_KEY, origin)
}

export function clearStoredServer(): void {
  storage().removeItem(SERVER_KEY)
}

/** REST base for a server origin, e.g. `https://help.acme.com/api`. */
export function apiBaseUrlFor(origin: string): string {
  return `${origin.replace(/\/$/, '')}/api`
}

/** Collab y-websocket base for a server origin (provider appends `/${docId}`). */
export function collabWsBaseUrlFor(origin: string): string {
  const ws = origin.replace(/^http:/i, 'ws:').replace(/^https:/i, 'wss:')
  return `${ws.replace(/\/$/, '')}/api/collaboration/ws`
}

export interface ServerValidation {
  ok: boolean
  /** The normalized origin (scheme + host[:port]) on success. */
  origin?: string
  error?: string
}

/**
 * Normalize + validate a user-entered server URL: it must be HTTPS, actually
 * be a Nosdesk instance (its public setup-status endpoint answers with the
 * expected shape), and run a version this app works with. Uses the native HTTP client so it isn't blocked by the
 * webview's CORS/CSP. Call this from the connect screen before `setServer`.
 */
export async function validateServer(input: string): Promise<ServerValidation> {
  let origin: string
  try {
    // A bare host is the expected input: the connect screen asks for a server
    // address, not a URL, so `help.example.com` is normalised here rather than
    // being pushed onto the user. An explicit scheme is still honoured so a
    // pasted URL works, and http is rejected below.
    const withScheme = /^https?:\/\//i.test(input) ? input : `https://${input.trim()}`
    const url = new URL(withScheme)
    if (url.protocol !== 'https:') {
      // Say why, not just what. The app carries a bearer token and ticket
      // content, so this is a refusal on the user's behalf rather than a
      // formatting rule they got wrong.
      return {
        ok: false,
        error: 'Nosdesk connects over https only, so your tickets and sign-in stay encrypted.',
      }
    }
    origin = url.origin
  } catch {
    return { ok: false, error: 'Enter a valid server URL' }
  }

  try {
    const res = await tauriFetch(`${apiBaseUrlFor(origin)}/auth/setup/status`, { method: 'GET' })
    if (!res.ok) return { ok: false, error: `The server responded ${res.status}` }
    const body = (await res.json().catch(() => null)) as { requires_setup?: unknown } | null
    if (!body || typeof body.requires_setup === 'undefined') {
      return { ok: false, error: "That doesn't look like a Nosdesk server" }
    }
  } catch {
    return { ok: false, error: 'Could not reach that server' }
  }

  const compatibility = await checkCompatibility(origin)
  if (compatibility.status === 'unknown') return { ok: false, error: 'Could not reach that server' }
  if (compatibility.status === 'incompatible') return { ok: false, error: compatibility.message }
  return { ok: true, origin }
}

/** `major.minor.patch` as numbers; a prerelease (`1.1.0-rc.7`) counts as its release. */
function versionParts(version: unknown): [number, number, number] | null {
  if (typeof version !== 'string') return null
  const match = /^(\d+)\.(\d+)\.(\d+)(?:[-+].*)?$/.exec(version)
  return match ? [Number(match[1]), Number(match[2]), Number(match[3])] : null
}

function isOlder(a: [number, number, number], b: [number, number, number]): boolean {
  for (let i = 0; i < 3; i++) {
    if (a[i] !== b[i]) return a[i] < b[i]
  }
  return false
}

export type Compatibility =
  | { status: 'ok' }
  | { status: 'incompatible'; message: string }
  /** The server couldn't be asked: offline, a timeout, a server error. */
  | { status: 'unknown' }

/**
 * Whether this app and the server at `origin` work together. The app is
 * versioned apart from the server, so each side states the oldest version of
 * the other it accepts.
 */
export async function checkCompatibility(origin: string): Promise<Compatibility> {
  let config: { server_version?: unknown; min_app_version?: unknown } | null
  try {
    const res = await tauriFetch(`${apiBaseUrlFor(origin)}/config`, { method: 'GET' })
    // A 1.0 server has no public /api/config: its auth layer answers 401.
    if (res.status === 401 || res.status === 404) config = {}
    else if (!res.ok) return { status: 'unknown' }
    else config = (await res.json().catch(() => null)) as typeof config
  } catch {
    return { status: 'unknown' }
  }
  const server = versionParts(config?.server_version)
  if (!server || isOlder(server, versionParts(MIN_SERVER_VERSION)!)) {
    return {
      status: 'incompatible',
      message: `This server runs an older version of Nosdesk. The app needs Nosdesk ${MIN_SERVER_VERSION.replace(/\.0$/, '')} or later, so ask whoever runs the server to update it.`,
    }
  }
  const needed = versionParts(config?.min_app_version)
  const app = versionParts(await getVersion().catch(() => null))
  if (needed && app && isOlder(app, needed)) {
    return {
      status: 'incompatible',
      message: 'This server needs a newer version of the Nosdesk app. Update the app, then try again.',
    }
  }
  return { status: 'ok' }
}

/**
 * Native OIDC login (RFC 8252): the app is its own public OIDC client.
 *
 * It runs the Authorization-Code + PKCE flow against the connected server's
 * central IdP in the system browser (`ASWebAuthenticationSession` via
 * tauri-plugin-web-auth), exchanges the code itself, then trades the resulting
 * `id_token` for a product bearer session at the server's native-login endpoint
 * (reusing the bearer transport + keychain). The IdP is touched only at login;
 * the app then lives on the product session.
 */
import { fetch as tauriFetch } from '@tauri-apps/plugin-http'
import { authenticate } from 'tauri-plugin-web-auth-api'
import { apiBaseUrl } from '@nosdesk/core/transport'
import { setSession } from './transport'

// Custom-scheme redirect registered for the public client on the IdP. iOS needs
// no Info.plist entry: ASWebAuthenticationSession intercepts this scheme itself.
const REDIRECT_URI = 'nosdesk://auth/callback'
const CALLBACK_SCHEME = 'nosdesk'

interface NativeOidcConfig {
  issuer: string
  client_id: string
  scopes: string
}

interface OidcEndpoints {
  authorization_endpoint: string
  token_endpoint: string
}

// --- PKCE + random (Web Crypto; the webview is a secure context) ---

function base64Url(bytes: Uint8Array): string {
  let s = ''
  for (const b of bytes) s += String.fromCharCode(b)
  return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

function randomString(byteLength: number): string {
  const bytes = new Uint8Array(byteLength)
  crypto.getRandomValues(bytes)
  return base64Url(bytes)
}

async function pkceChallenge(verifier: string): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(verifier))
  return base64Url(new Uint8Array(digest))
}

/** The `nonce` claim from a JWT id_token (decode only; app-side replay check). */
function idTokenNonce(idToken: string): string | undefined {
  const payload = idToken.split('.')[1]
  if (!payload) return undefined
  try {
    return JSON.parse(atob(payload.replace(/-/g, '+').replace(/_/g, '/'))).nonce
  } catch {
    return undefined
  }
}

async function fetchConfig(): Promise<NativeOidcConfig> {
  const res = await tauriFetch(`${apiBaseUrl()}/auth/native-oidc-config`, { method: 'GET' })
  if (!res.ok) throw new Error(`This server doesn't support app sign-in (${res.status})`)
  return (await res.json()) as NativeOidcConfig
}

async function discover(issuer: string): Promise<OidcEndpoints> {
  const url = `${issuer.replace(/\/$/, '')}/.well-known/openid-configuration`
  const res = await tauriFetch(url, { method: 'GET' })
  if (!res.ok) throw new Error(`Identity provider discovery failed (${res.status})`)
  return (await res.json()) as OidcEndpoints
}

function parseCallback(callbackUrl: string): { code: string; state: string } {
  const u = new URL(callbackUrl)
  const error = u.searchParams.get('error')
  if (error) throw new Error(u.searchParams.get('error_description') || error)
  const code = u.searchParams.get('code')
  const state = u.searchParams.get('state')
  if (!code || !state) throw new Error('Sign-in was cancelled')
  return { code, state }
}

/**
 * Run the full native OIDC login against the connected server. On success the
 * session is set (refresh token persisted in the keychain) and the app is
 * authenticated. Throws with a user-presentable message on failure.
 */
export async function loginWithOidc(): Promise<void> {
  const cfg = await fetchConfig()
  const endpoints = await discover(cfg.issuer)

  const verifier = randomString(32)
  const challenge = await pkceChallenge(verifier)
  const state = randomString(16)
  const nonce = randomString(16)

  const authUrl = new URL(endpoints.authorization_endpoint)
  authUrl.search = new URLSearchParams({
    response_type: 'code',
    client_id: cfg.client_id,
    redirect_uri: REDIRECT_URI,
    scope: cfg.scopes,
    state,
    nonce,
    code_challenge: challenge,
    code_challenge_method: 'S256',
  }).toString()

  // System browser; returns the nosdesk://auth/callback URL inline.
  const { callbackUrl } = await authenticate({ url: authUrl.toString(), callbackScheme: CALLBACK_SCHEME })
  const { code, state: returnedState } = parseCallback(callbackUrl)
  if (returnedState !== state) throw new Error('Sign-in failed (state mismatch)')

  // Public-client code exchange at the IdP (PKCE, no secret).
  const tokenRes = await tauriFetch(endpoints.token_endpoint, {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({
      grant_type: 'authorization_code',
      code,
      redirect_uri: REDIRECT_URI,
      client_id: cfg.client_id,
      code_verifier: verifier,
    }).toString(),
  })
  if (!tokenRes.ok) throw new Error(`Sign-in failed at the identity provider (${tokenRes.status})`)
  const tokenData = (await tokenRes.json()) as { id_token?: string }
  if (!tokenData.id_token) throw new Error('Identity provider returned no ID token')
  if (idTokenNonce(tokenData.id_token) !== nonce) throw new Error('Sign-in failed (nonce mismatch)')

  // Trade the id_token for a product bearer session.
  const loginRes = await tauriFetch(`${apiBaseUrl()}/auth/oidc/native-login`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'X-Auth-Mode': 'bearer' },
    body: JSON.stringify({ id_token: tokenData.id_token }),
  })
  if (!loginRes.ok) {
    if (loginRes.status === 403) throw new Error('Your account has no access on this server')
    throw new Error(`Sign-in failed (${loginRes.status})`)
  }
  const session = (await loginRes.json()) as { access_token?: string; refresh_token?: string }
  if (!session.access_token || !session.refresh_token) throw new Error('No session returned')
  await setSession(session.access_token, session.refresh_token)
}

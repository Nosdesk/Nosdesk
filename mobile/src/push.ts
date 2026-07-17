/**
 * Push-notification registration for the mobile shell.
 *
 * After login (session live) the app asks the native `tauri-plugin-push` for
 * notification permission + the platform device token (APNs on iOS, FCM on
 * Android — Android returns null until FCM is provisioned), then POSTs it to
 * `/api/notifications/devices` with the user's bearer. On sign-out the token is
 * revoked. All best-effort: a push-registration failure must never block login
 * or sign-out.
 */
import { invoke } from '@tauri-apps/api/core'
import { getVersion } from '@tauri-apps/api/app'
import {
  registerPushDevice,
  unregisterPushDevice,
  type PushPlatform,
} from '@nosdesk/core/services/notificationService'

/** The last token we registered, so sign-out can revoke exactly it. */
let lastRegisteredToken: string | null = null

/** iOS vs Android from the WebView user-agent (the OS plugin isn't bundled). */
function detectPlatform(): PushPlatform | null {
  const ua = navigator.userAgent
  if (/iPhone|iPad|iPod/i.test(ua)) return 'ios'
  if (/Android/i.test(ua)) return 'android'
  return null
}

/**
 * Request permission, obtain the device token, and register it with the
 * backend. Idempotent server-side, so it's safe to call on every login /
 * app-resume. No-op (resolves) when not on a supported platform or when
 * permission is denied / no token is available yet.
 */
export async function registerForPush(): Promise<void> {
  const platform = detectPlatform()
  if (!platform) {
    console.info('[push] not a mobile platform — skipping')
    return
  }

  try {
    console.info(`[push] requesting notification permission (${platform})`)
    const permission = await invoke<{ granted: boolean }>('plugin:push|request_permission')
    console.info('[push] permission:', permission?.granted)
    if (!permission?.granted) return

    const result = await invoke<{ token: string | null }>('plugin:push|get_token')
    const token = result?.token
    console.info('[push] token obtained:', token ? `yes (len ${token.length})` : 'no')
    if (!token) {
      console.warn('[push] no device token — APNs registration/swizzle did not deliver one')
      return
    }

    let appVersion: string | undefined
    try {
      appVersion = await getVersion()
    } catch {
      appVersion = undefined
    }

    await registerPushDevice(platform, token, appVersion)
    lastRegisteredToken = token
    console.info('[push] device registered with backend')
  } catch (e) {
    console.error('[push] registration failed:', e)
  }
}

/**
 * Revoke this device's push token on sign-out (or server switch). Best-effort;
 * the backend also prunes tokens the provider later rejects.
 */
export async function unregisterForPush(): Promise<void> {
  const token = lastRegisteredToken
  if (!token) return
  lastRegisteredToken = null
  try {
    await unregisterPushDevice(token)
  } catch {
    // Ignore — sign-out must not block on this.
  }
}

/** The PII-free tap payload the native plugin surfaces (camelCase keys match
 *  the Rust `PendingNotification` / Swift `NotificationOpened`). */
interface NotificationOpenedPayload {
  ndType?: string | null
  entityType?: string | null
  entityId?: number | null
  ticketId?: number | null
}

/** Map a tapped notification to an in-app route, or `null` if it has no
 *  meaningful target (just open the app). Extend as more entity types get
 *  their own screens. */
function routeFromPayload(p: NotificationOpenedPayload | null | undefined): string | null {
  if (!p) return null
  if (typeof p.ticketId === 'number' && p.ticketId > 0) return `/tickets/${p.ticketId}`
  if (p.entityType === 'asset' && typeof p.entityId === 'number' && p.entityId > 0) {
    return `/assets/${p.entityId}`
  }
  return null
}

/**
 * Drain the buffered "tapped notification" (read-and-clear) and return the route
 * to deep-link to, or `null` if nothing is pending. The native side buffers the
 * tap; the app calls this on mount (cold-start tap) and on foreground (warm tap).
 *
 * We poll this rather than listen for a plugin event because the Tauri
 * PluginManager event bus does not deliver plugin events to the webview on iOS.
 */
export async function getPendingNotificationRoute(): Promise<string | null> {
  try {
    const pending = await invoke<NotificationOpenedPayload>('plugin:push|get_pending_notification')
    return routeFromPayload(pending)
  } catch {
    return null
  }
}

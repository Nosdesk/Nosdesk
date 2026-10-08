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
import { ticketRoute, ticketRouteById } from '@nosdesk/core/utils/ticketRoutes'
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
 * app-resume.
 *
 * Returns whether a device is now registered. Callers that show the user a
 * result must use it: permission can be granted while the POST fails, and
 * reporting success then would promise notifications that cannot arrive.
 */
export async function registerForPush(): Promise<boolean> {
  const platform = detectPlatform()
  if (!platform) {
    console.info('[push] not a mobile platform — skipping')
    return false
  }

  try {
    console.info(`[push] requesting notification permission (${platform})`)
    const permission = await invoke<{ granted: boolean }>('plugin:push|request_permission')
    console.info('[push] permission:', permission?.granted)
    if (!permission?.granted) return false

    const result = await invoke<{ token: string | null }>('plugin:push|get_token')
    const token = result?.token
    console.info('[push] token obtained:', token ? `yes (len ${token.length})` : 'no')
    if (!token) {
      console.warn('[push] no device token — APNs registration/swizzle did not deliver one')
      return false
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
    return true
  } catch (e) {
    console.error('[push] registration failed:', e)
    return false
  }
}

/** Notification permission as the OS currently sees it. */
export type PushPermission = 'granted' | 'denied' | 'prompt' | 'unsupported'

/**
 * Read permission WITHOUT prompting.
 *
 * The settings screen renders from this, so it must not have the side effect of
 * showing the OS dialog: an Android 13+ prompt would fire on every visit, and on
 * iOS a prompt after a denial resolves false silently and burns the one chance
 * the OS gives.
 */
export async function checkPushPermission(): Promise<PushPermission> {
  if (!detectPlatform()) return 'unsupported'
  try {
    const res = await invoke<{ status: string }>('plugin:push|check_permission')
    const status = res?.status
    return status === 'granted' || status === 'denied' || status === 'prompt'
      ? status
      : 'unsupported'
  } catch (e) {
    console.warn('[push] could not read permission:', e)
    return 'unsupported'
  }
}

/**
 * Open this app's OS notification settings.
 *
 * Once permission is denied neither platform will prompt again, so this is the
 * only route back to granting it.
 */
export async function openPushSettings(): Promise<void> {
  try {
    await invoke('plugin:push|open_settings')
  } catch (e) {
    console.warn('[push] could not open settings:', e)
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
  /** Absent from servers (and relays) older than ticket numbers. */
  ticketNumber?: number | null
}

/** Map a tapped notification to an in-app route, or `null` if it has no
 *  meaningful target (just open the app). Extend as more entity types get
 *  their own screens. */
function routeFromPayload(p: NotificationOpenedPayload | null | undefined): string | null {
  if (!p) return null
  if (typeof p.ticketNumber === 'number' && p.ticketNumber > 0) return ticketRoute(p.ticketNumber)
  // Without a number, the by-id route looks it up.
  if (typeof p.ticketId === 'number' && p.ticketId > 0) return ticketRouteById(p.ticketId)
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

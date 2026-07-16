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
  if (!platform) return

  try {
    const permission = await invoke<{ granted: boolean }>('plugin:push|request_permission')
    if (!permission?.granted) return

    const result = await invoke<{ token: string | null }>('plugin:push|get_token')
    const token = result?.token
    if (!token) return

    let appVersion: string | undefined
    try {
      appVersion = await getVersion()
    } catch {
      appVersion = undefined
    }

    await registerPushDevice(platform, token, appVersion)
    lastRegisteredToken = token
  } catch {
    // Best-effort: never surface a push failure to the login flow.
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

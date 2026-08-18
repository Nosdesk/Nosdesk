/**
 * Reports a human-readable device name so the user's session list can say
 * "iPhone . iOS 18.2" instead of guessing from a webview user-agent.
 *
 * Registered as a core request-header provider, so it rides every request
 * rather than just login. The backend only reads it when creating a session,
 * but that covers login, the OIDC callback, and the legacy refresh path
 * without any of them needing to know about it.
 *
 * A marketing name ("iPhone 15 Pro") is not available: iOS 16+ returns a
 * generic `UIDevice.name` unless the app holds a special entitlement, so
 * platform plus OS version is the most specific honest label.
 */
import { platform, version } from '@tauri-apps/plugin-os'
import { addRequestHeaderProvider } from '@nosdesk/core/transport'
import { logger } from '@nosdesk/core/utils/logger'

/** Matches the backend's `DEVICE_NAME_HEADER`. */
const DEVICE_NAME_HEADER = 'X-Device-Name'

const PLATFORM_LABELS: Record<string, string> = {
  ios: 'iPhone',
  android: 'Android',
  macos: 'Mac',
  windows: 'Windows',
  linux: 'Linux',
}

let cached: string | null = null

/**
 * Resolve once at bootstrap. Both plugin calls are synchronous and cheap, but
 * the value cannot change for the life of the process, so there is no reason
 * to recompute it per request.
 */
export function resolveDeviceName(): string | null {
  if (cached !== null) return cached
  try {
    const os = platform()
    const label = PLATFORM_LABELS[os] ?? os
    const osVersion = version()
    cached = osVersion ? `${label} (${osVersion})` : label
  } catch (error) {
    // Never block a login over a cosmetic label; the session list falls back
    // to deriving one from the user-agent.
    logger.debug('Could not resolve a device name', { error })
    cached = ''
  }
  return cached || null
}

/** Register the header provider. Call once, at bootstrap. */
export function setupDeviceName(): void {
  const name = resolveDeviceName()
  if (!name) return
  addRequestHeaderProvider(() => ({ [DEVICE_NAME_HEADER]: name }))
}

/**
 * The OS the native shell runs on, for the web layer's per-OS behaviour.
 * plugin-os reads a value the shell injects before the page loads, so this
 * is synchronous; it returns null if the plugin isn't there.
 */
import { platform } from '@tauri-apps/plugin-os'

export function osPlatform(): string | null {
  try {
    return platform()
  } catch {
    return null
  }
}

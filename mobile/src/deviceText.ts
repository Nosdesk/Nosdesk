/**
 * The phone's text size and Bold Text settings, from the native
 * `device-text` plugin. Deliberately NOT exported from `index.ts`: the
 * frontend imports this as `@nosdesk/mobile/deviceText` so applying text
 * settings never waits on the bootstrap graph.
 *
 * `scale` is body text relative to the platform default (iOS Dynamic Type body
 * size / 17pt, Android `fontScale`), between 0.5 and 2.0. `bold` is Bold Text.
 * The desktop shell reports `{ scale: 1, bold: false }`.
 */
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

export interface DeviceTextSettings {
  scale: number
  bold: boolean
}

/** Mirrors `CHANGE_EVENT` in `tauri-plugin-device-text/src/lib.rs`. */
const CHANGE_EVENT = 'device-text://change'

export function getDeviceText(): Promise<DeviceTextSettings> {
  return invoke<DeviceTextSettings>('plugin:device-text|get')
}

/**
 * The web layer applies the scale itself from now on. Android stops its
 * WebView scaling text as well, for this and every later WebView; iOS has
 * nothing to stop.
 */
export function followDeviceText(): Promise<void> {
  return invoke<void>('plugin:device-text|follow')
}

/** Calls `handler` whenever the user changes either setting while the app runs. */
export function onDeviceTextChange(
  handler: (settings: DeviceTextSettings) => void,
): Promise<() => void> {
  return listen<DeviceTextSettings>(CHANGE_EVENT, (event) => handler(event.payload))
}

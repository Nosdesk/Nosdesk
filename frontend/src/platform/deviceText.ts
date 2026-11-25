/**
 * The phone's text size and Bold Text, in the native app.
 *
 * Neither app webview follows these settings properly by itself (WKWebView
 * ignores Dynamic Type; Android's WebView scaling is switched off natively),
 * so the native `device-text` plugin reports them and this module puts them
 * on the root element for CSS:
 *
 * - `--device-text-scale` on `<html>`: body text relative to the platform
 *   default, unitless (iOS xLarge is 19/17, about 1.1176). Values below 1 are
 *   passed through; the type tokens decide whether to floor them.
 * - `data-bold-text="true"` on `<html>` while Bold Text is on, absent otherwise.
 *
 * A browser gets neither: browser zoom and font settings already reach rem.
 *
 * `deviceText` holds the current values for the Text size settings page.
 */
import { readonly, shallowRef } from 'vue'
import { isTauriRuntime } from './runtime'

export interface DeviceText {
  /** Body text size relative to the platform default, 0.5 to 2. */
  scale: number
  bold: boolean
}

export const DEVICE_TEXT_SCALE_PROPERTY = '--device-text-scale'
export const BOLD_TEXT_ATTRIBUTE = 'data-bold-text'

const MIN_SCALE = 0.5
const MAX_SCALE = 2

/**
 * How long launch waits for the first read. The launch splash covers the
 * wait, so the first screen draws at the right size; a slow or missing plugin
 * must not hold the app back longer than this.
 */
const FIRST_READ_TIMEOUT_MS = 500

const current = shallowRef<DeviceText | null>(null)

/** The device's text settings. `null` in a browser, or until the first read. */
export const deviceText = readonly(current)

/** A plugin payload as settings, or `null` when it is not one. */
export function parseDeviceText(raw: unknown): DeviceText | null {
  if (typeof raw !== 'object' || raw === null) return null
  const { scale, bold } = raw as { scale?: unknown; bold?: unknown }
  if (typeof scale !== 'number' || !Number.isFinite(scale) || typeof bold !== 'boolean') return null
  return { scale: Math.min(MAX_SCALE, Math.max(MIN_SCALE, scale)), bold }
}

export function applyDeviceText(settings: DeviceText, root: HTMLElement = document.documentElement): void {
  root.style.setProperty(DEVICE_TEXT_SCALE_PROPERTY, String(Math.round(settings.scale * 10000) / 10000))
  if (settings.bold) root.setAttribute(BOLD_TEXT_ATTRIBUTE, 'true')
  else root.removeAttribute(BOLD_TEXT_ATTRIBUTE)
}

function receive(raw: unknown): void {
  const settings = parseDeviceText(raw)
  if (!settings) return
  const previous = current.value
  if (previous && previous.scale === settings.scale && previous.bold === settings.bold) return
  current.value = settings
  applyDeviceText(settings)
}

function withTimeout(work: Promise<unknown>, ms: number): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(resolve, ms)
    const done = () => {
      clearTimeout(timer)
      resolve()
    }
    work.then(done, done)
  })
}

let started = false

/**
 * Read the settings and follow changes. Call once at startup, before the app
 * mounts. Never throws, and waits at most `FIRST_READ_TIMEOUT_MS`.
 */
export async function initDeviceText(): Promise<void> {
  if (started || !isTauriRuntime()) return
  started = true
  try {
    const native = await import('@nosdesk/mobile/deviceText')
    const read = () => native.getDeviceText().then(receive, () => {})
    // Live changes (Control Center's Text Size, a split screen Settings app).
    void native.onDeviceTextChange(receive).catch(() => {})
    // A change made in the Settings app while this one was in the background.
    document.addEventListener('visibilitychange', () => {
      if (document.visibilityState === 'visible') void read()
    })
    await withTimeout(read(), FIRST_READ_TIMEOUT_MS)
  } catch {
    // Plugin absent (desktop dev shell): the app keeps its default size.
  }
}
